//! In-memory throttling state for the public login surface (issue #428).
//!
//! Three independent guards, all deliberately self-contained (a `Mutex`
//! around a `HashMap`, no external dependencies, no persistence):
//!
//! * **Per-client sliding window** on `POST /v1/auth/login` and
//!   `POST /v1/auth/login/totp`: at most [`IP_MAX_ATTEMPTS`] attempts per
//!   [`IP_WINDOW`] for one (client address, username) pair, and at most
//!   [`IP_TOTAL_MAX_ATTEMPTS`] per client address across all usernames. The
//!   client address is the socket peer, or — only when that peer is listed
//!   in `CRONIQ_TRUSTED_PROXIES` — the first untrusted hop of
//!   `X-Forwarded-For` (see [`TrustedProxies`]). An attempt that ends in a
//!   successful sign-in is refunded, so only failures use the budget.
//!   Behind a reverse proxy that is not declared trusted every request
//!   shares the proxy's address; the per-username key keeps one attacked
//!   account from locking every other user out (review 2026-10-24, A-3).
//!   This layer complements (does not replace) the per-account lockout: the
//!   lockout stops online brute force against one account, the window stops
//!   one address from hammering many accounts.
//! * **Per-`mfa_token` failure counter** for the TOTP step: after
//!   [`MFA_MAX_FAILURES`] wrong codes the token is invalidated — the entry
//!   outlives the token's own 5-minute TTL, so the caller must redo the
//!   password step. Without this, one `mfa_token` allowed unlimited
//!   guesses at the ~3 currently valid 6-digit codes for its lifetime.
//! * **Per-user last-consumed TOTP step**: a verified 6-digit code is
//!   otherwise replayable for the rest of its ±1-step skew window; the
//!   guard rejects any code from a step at or below the last consumed one
//!   (recovery codes are already single-use in the store).
//!
//! State is process-local and lost on restart. That is acceptable: every
//! window here is minutes long, and the guards are hardening layers on
//! top of bcrypt, the account lockout, and single-use recovery codes.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

/// Maximum login attempts allowed per source address within [`IP_WINDOW`].
/// Generous on purpose — it only needs to be far below the ~1M guesses a
/// 6-digit space needs, not tight enough to bother a shared NAT.
pub const IP_MAX_ATTEMPTS: usize = 30;
/// Attempts per client address across all usernames within [`IP_WINDOW`].
/// Bounds username rotation from one address now that the tighter budget
/// above is keyed per (address, username).
pub const IP_TOTAL_MAX_ATTEMPTS: usize = IP_MAX_ATTEMPTS * 10;
/// Usernames are cut to this many characters before they become a map key.
const USERNAME_KEY_MAX: usize = 128;
/// Length of the per-IP sliding window.
pub const IP_WINDOW: Duration = Duration::from_secs(5 * 60);
/// Failed second-factor attempts after which an `mfa_token` is invalidated.
pub const MFA_MAX_FAILURES: u32 = 5;
/// How long an `mfa_token`'s failure entry is retained. Longer than the
/// token's own 5-minute TTL so a blocked token stays blocked for the rest
/// of its life; pruning afterwards keeps the map from growing unbounded.
const MFA_ENTRY_TTL: Duration = Duration::from_secs(10 * 60);
/// How long a user's last-consumed TOTP step is retained. The step only
/// matters within the ±1-step (~90 s) acceptance window; 5 minutes leaves
/// comfortable slack.
const STEP_ENTRY_TTL: Duration = Duration::from_secs(5 * 60);
/// Full-sweep threshold: once a map holds this many keys, stale entries
/// are pruned across the whole map on the next insert.
const PRUNE_THRESHOLD: usize = 1024;

#[derive(Debug)]
struct MfaEntry {
    failures: u32,
    last_seen: Instant,
}

#[derive(Debug)]
struct StepEntry {
    step: u64,
    last_seen: Instant,
}

/// Shared throttling state, one instance per server (`ServerState`).
#[derive(Default)]
pub struct LoginThrottle {
    /// Attempts per (client address, lower-cased username). The TOTP step,
    /// which carries no username, uses the empty string.
    ip_attempts: Mutex<HashMap<(IpAddr, String), Vec<Instant>>>,
    /// Attempts per client address, all usernames together.
    ip_totals: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    /// Peers whose `X-Forwarded-For` is believed (`CRONIQ_TRUSTED_PROXIES`).
    pub trusted_proxies: TrustedProxies,
    mfa_failures: Mutex<HashMap<String, MfaEntry>>,
    consumed_steps: Mutex<HashMap<String, StepEntry>>,
}

impl LoginThrottle {
    /// Record one attempt from `ip` that names no username (the TOTP step)
    /// and report whether it is allowed. See [`LoginThrottle::allow_login`].
    pub fn allow_ip(&self, ip: Option<IpAddr>) -> bool {
        self.allow_login(ip, "")
    }

    #[cfg(test)]
    fn allow_ip_at(&self, ip: Option<IpAddr>, now: Instant) -> bool {
        self.allow_login_at(ip, "", now)
    }

    /// Record one login attempt from `ip` for `username` and report whether
    /// it is allowed. `None` (no `ConnectInfo`, e.g. handler-level tests)
    /// always passes — there is nothing meaningful to key on.
    ///
    /// The attempt is recorded up front, not after the outcome is known, so
    /// a burst of parallel requests cannot all slip in before the first one
    /// fails. A successful sign-in hands it back via
    /// [`LoginThrottle::refund_login`].
    pub fn allow_login(&self, ip: Option<IpAddr>, username: &str) -> bool {
        self.allow_login_at(ip, username, Instant::now())
    }

    fn allow_login_at(&self, ip: Option<IpAddr>, username: &str, now: Instant) -> bool {
        let Some(ip) = ip else { return true };
        let key = (ip, username_key(username));
        let mut pairs = self.ip_attempts.lock().unwrap();
        let mut totals = self.ip_totals.lock().unwrap();
        prune(&mut pairs, now);
        prune(&mut totals, now);
        let pair = pairs.entry(key).or_default();
        pair.retain(|t| now.duration_since(*t) < IP_WINDOW);
        let total = totals.entry(ip).or_default();
        total.retain(|t| now.duration_since(*t) < IP_WINDOW);
        if pair.len() >= IP_MAX_ATTEMPTS || total.len() >= IP_TOTAL_MAX_ATTEMPTS {
            return false;
        }
        pair.push(now);
        total.push(now);
        true
    }

    /// Give back the attempt recorded by [`LoginThrottle::allow_login`] once
    /// it turned into a successful sign-in. Without this, 30 ordinary logins
    /// through one proxy address in five minutes locked everybody out.
    pub fn refund_login(&self, ip: Option<IpAddr>, username: &str) {
        let Some(ip) = ip else { return };
        if let Some(v) = self
            .ip_attempts
            .lock()
            .unwrap()
            .get_mut(&(ip, username_key(username)))
        {
            v.pop();
        }
        if let Some(v) = self.ip_totals.lock().unwrap().get_mut(&ip) {
            v.pop();
        }
    }

    /// Whether this `mfa_token` (keyed by its hash) has burned through its
    /// failure budget and must be treated as invalid.
    pub fn mfa_blocked(&self, token_hash: &str) -> bool {
        self.mfa_blocked_at(token_hash, Instant::now())
    }

    fn mfa_blocked_at(&self, token_hash: &str, now: Instant) -> bool {
        let map = self.mfa_failures.lock().unwrap();
        map.get(token_hash).is_some_and(|e| {
            e.failures >= MFA_MAX_FAILURES && now.duration_since(e.last_seen) < MFA_ENTRY_TTL
        })
    }

    /// Record one failed second-factor attempt for this `mfa_token` and
    /// return the updated failure count.
    pub fn record_mfa_failure(&self, token_hash: &str) -> u32 {
        self.record_mfa_failure_at(token_hash, Instant::now())
    }

    fn record_mfa_failure_at(&self, token_hash: &str, now: Instant) -> u32 {
        let mut map = self.mfa_failures.lock().unwrap();
        if map.len() > PRUNE_THRESHOLD {
            map.retain(|_, e| now.duration_since(e.last_seen) < MFA_ENTRY_TTL);
        }
        let entry = map.entry(token_hash.to_string()).or_insert(MfaEntry {
            failures: 0,
            last_seen: now,
        });
        entry.failures = entry.failures.saturating_add(1);
        entry.last_seen = now;
        entry.failures
    }

    /// Drop the failure entry after a successful second factor, so the map
    /// does not accumulate one key per completed login.
    pub fn clear_mfa(&self, token_hash: &str) {
        self.mfa_failures.lock().unwrap().remove(token_hash);
    }

    /// Try to consume TOTP time step `step` for `user_id`. Returns `false`
    /// when the step is at or below the last consumed one — i.e. the code
    /// (or an older one) has already been spent and this is a replay.
    pub fn consume_totp_step(&self, user_id: &str, step: u64) -> bool {
        self.consume_totp_step_at(user_id, step, Instant::now())
    }

    fn consume_totp_step_at(&self, user_id: &str, step: u64, now: Instant) -> bool {
        let mut map = self.consumed_steps.lock().unwrap();
        if map.len() > PRUNE_THRESHOLD {
            map.retain(|_, e| now.duration_since(e.last_seen) < STEP_ENTRY_TTL);
        }
        match map.get_mut(user_id) {
            Some(entry) => {
                let fresh =
                    step > entry.step || now.duration_since(entry.last_seen) >= STEP_ENTRY_TTL;
                if fresh {
                    entry.step = step;
                    entry.last_seen = now;
                }
                fresh
            }
            None => {
                map.insert(
                    user_id.to_string(),
                    StepEntry {
                        step,
                        last_seen: now,
                    },
                );
                true
            }
        }
    }
}

fn username_key(username: &str) -> String {
    username
        .trim()
        .chars()
        .take(USERNAME_KEY_MAX)
        .collect::<String>()
        .to_lowercase()
}

fn prune<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Vec<Instant>>, now: Instant) {
    if map.len() > PRUNE_THRESHOLD {
        map.retain(|_, attempts| {
            attempts
                .last()
                .is_some_and(|last| now.duration_since(*last) < IP_WINDOW)
        });
    }
}

/// Reverse proxies whose `X-Forwarded-For` header the login throttle
/// believes, from `CRONIQ_TRUSTED_PROXIES` (comma-separated addresses or
/// CIDR ranges, e.g. `10.0.0.0/8, 127.0.0.1`). Empty by default: on a
/// directly exposed server the header is attacker-controlled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<(IpAddr, u8)>);

impl TrustedProxies {
    pub const ENV: &'static str = "CRONIQ_TRUSTED_PROXIES";

    /// Parse the env value. Errors name the offending entry.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut nets = Vec::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (addr, len) = match entry.split_once('/') {
                Some((a, l)) => (a, Some(l)),
                None => (entry, None),
            };
            let addr: IpAddr = addr.parse().map_err(|_| {
                format!(
                    "{}: '{entry}' is not an IP address or CIDR range",
                    Self::ENV
                )
            })?;
            let addr = addr.to_canonical();
            let max = if addr.is_ipv4() { 32 } else { 128 };
            let len = match len {
                None => max,
                Some(l) => l.parse::<u8>().ok().filter(|l| *l <= max).ok_or_else(|| {
                    format!("{}: '{entry}' has an invalid prefix length", Self::ENV)
                })?,
            };
            nets.push((addr, len));
        }
        Ok(Self(nets))
    }

    /// Read [`TrustedProxies::ENV`]; unset or empty means no trusted proxy.
    pub fn from_env() -> Result<Self, String> {
        match std::env::var(Self::ENV) {
            Ok(v) => Self::parse(&v),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("{} is not valid Unicode", Self::ENV))
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        self.0.iter().any(|(net, len)| match (net, ip) {
            (IpAddr::V4(n), IpAddr::V4(i)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(*len)).unwrap_or(0);
                u32::from(*n) & mask == u32::from(i) & mask
            }
            (IpAddr::V6(n), IpAddr::V6(i)) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(*len)).unwrap_or(0);
                u128::from(*n) & mask == u128::from(i) & mask
            }
            _ => false,
        })
    }

    /// The client address for a request from `peer` carrying these
    /// `X-Forwarded-For` values. Only a trusted peer's header is read, and
    /// it is walked right to left past further trusted hops, so the result
    /// is the nearest address no trusted proxy accounts for. An entry that
    /// does not parse stops the walk at the hop that wrote it.
    pub fn client_ip<'a>(
        &self,
        peer: IpAddr,
        forwarded_for: impl Iterator<Item = &'a str>,
    ) -> IpAddr {
        if !self.contains(peer) {
            return peer;
        }
        let hops: Vec<&str> = forwarded_for
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .collect();
        let mut client = peer;
        for hop in hops.iter().rev() {
            let Some(ip) = parse_hop(hop) else {
                return client;
            };
            client = ip;
            if !self.contains(ip) {
                return ip;
            }
        }
        client
    }
}

fn parse_hop(hop: &str) -> Option<IpAddr> {
    hop.parse::<IpAddr>()
        .or_else(|_| hop.parse::<SocketAddr>().map(|s| s.ip()))
        .ok()
        .map(|ip| ip.to_canonical())
}

/// The client address of the request: the socket peer, when the router was
/// served via `into_make_service_with_connect_info::<SocketAddr>()` (as
/// `main.rs` does), or the forwarded client when that peer is a trusted
/// proxy (see [`TrustedProxies`]). `None` when no `ConnectInfo` extension
/// is present — router-level tests and direct handler calls — in which
/// case IP throttling is skipped.
pub struct ClientIp(pub Option<IpAddr>);

impl FromRequestParts<std::sync::Arc<super::ServerState>> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &std::sync::Arc<super::ServerState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(peer) = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip())
        else {
            return Ok(ClientIp(None));
        };
        let xff = parts
            .headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok());
        Ok(ClientIp(Some(
            state.login_throttle.trusted_proxies.client_ip(peer, xff),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> Option<IpAddr> {
        Some(IpAddr::from([203, 0, 113, last]))
    }

    #[test]
    fn ip_window_allows_the_budget_then_refuses() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        for i in 0..IP_MAX_ATTEMPTS {
            assert!(t.allow_ip_at(ip(1), now), "attempt {i} is within budget");
        }
        assert!(!t.allow_ip_at(ip(1), now), "attempt 31 must be refused");
        // A different address is unaffected.
        assert!(t.allow_ip_at(ip(2), now));
    }

    #[test]
    fn ip_window_slides_open_again() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        for _ in 0..IP_MAX_ATTEMPTS {
            assert!(t.allow_ip_at(ip(1), now));
        }
        assert!(!t.allow_ip_at(ip(1), now));
        // Once the earlier attempts fall out of the window, the address
        // may try again.
        let later = now + IP_WINDOW + Duration::from_secs(1);
        assert!(t.allow_ip_at(ip(1), later));
    }

    /// Review 2026-10-24, A-3: behind one proxy address, an attacked
    /// account must not lock the other users out, and successful sign-ins
    /// must not use up the budget.
    #[test]
    fn per_username_budget_and_refund_on_success() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        for _ in 0..IP_MAX_ATTEMPTS {
            assert!(t.allow_login_at(ip(1), "victim", now));
        }
        assert!(!t.allow_login_at(ip(1), "victim", now));
        assert!(
            !t.allow_login_at(ip(1), "VICTIM ", now),
            "key is normalised"
        );
        assert!(
            t.allow_login_at(ip(1), "alice", now),
            "other users unaffected"
        );

        // A hundred successful logins through the same address cost nothing.
        for _ in 0..100 {
            assert!(t.allow_login_at(ip(1), "bob", now));
            t.refund_login(ip(1), "bob");
        }
        assert!(t.allow_login_at(ip(1), "bob", now));

        // Rotating usernames is bounded by the per-address total.
        let t = LoginThrottle::default();
        let mut allowed = 0;
        for i in 0..(IP_TOTAL_MAX_ATTEMPTS + 10) {
            if t.allow_login_at(ip(2), &format!("user{i}"), now) {
                allowed += 1;
            }
        }
        assert_eq!(allowed, IP_TOTAL_MAX_ATTEMPTS);
    }

    #[test]
    fn trusted_proxies_parse_and_match() {
        let tp = TrustedProxies::parse(" 10.0.0.0/8, 127.0.0.1 ,fd00::/8").unwrap();
        assert!(tp.contains("10.1.2.3".parse().unwrap()));
        assert!(tp.contains("127.0.0.1".parse().unwrap()));
        assert!(
            tp.contains("::ffff:10.9.9.9".parse().unwrap()),
            "v4-mapped peer"
        );
        assert!(tp.contains("fd12::1".parse().unwrap()));
        assert!(!tp.contains("11.0.0.1".parse().unwrap()));
        assert!(!tp.contains("127.0.0.2".parse().unwrap()));
        assert!(TrustedProxies::parse("").unwrap().is_empty());
        assert!(TrustedProxies::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxies::parse("proxy.local").is_err());
        assert!(
            TrustedProxies::parse("0.0.0.0/0")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
    }

    #[test]
    fn forwarded_for_is_read_only_from_trusted_peers() {
        let tp = TrustedProxies::parse("10.0.0.0/8").unwrap();
        let peer_proxy: IpAddr = "10.0.0.5".parse().unwrap();
        let peer_direct: IpAddr = "198.51.100.7".parse().unwrap();
        let client: IpAddr = "203.0.113.9".parse().unwrap();

        // Direct, untrusted peer: the header is ignored.
        assert_eq!(
            tp.client_ip(peer_direct, ["203.0.113.9"].into_iter()),
            peer_direct
        );
        // Trusted proxy: the rightmost untrusted hop wins, so a spoofed
        // left-hand entry the client sent itself is not believed.
        assert_eq!(
            tp.client_ip(peer_proxy, ["1.2.3.4, 203.0.113.9, 10.0.0.9"].into_iter()),
            client
        );
        // Several header lines are one list.
        assert_eq!(
            tp.client_ip(peer_proxy, ["1.2.3.4", "203.0.113.9:4711"].into_iter()),
            client
        );
        // No header, or garbage: fall back to the proxy itself.
        assert_eq!(tp.client_ip(peer_proxy, std::iter::empty()), peer_proxy);
        assert_eq!(tp.client_ip(peer_proxy, ["junk"].into_iter()), peer_proxy);
        // Nothing trusted: never read.
        assert_eq!(
            TrustedProxies::default().client_ip(peer_proxy, ["203.0.113.9"].into_iter()),
            peer_proxy
        );
    }

    #[test]
    fn missing_peer_address_is_never_throttled() {
        let t = LoginThrottle::default();
        for _ in 0..(IP_MAX_ATTEMPTS * 2) {
            assert!(t.allow_ip(None));
        }
    }

    #[test]
    fn mfa_failures_block_at_the_threshold_and_expire() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        for i in 1..MFA_MAX_FAILURES {
            assert_eq!(t.record_mfa_failure_at("tok", now), i);
            assert!(!t.mfa_blocked_at("tok", now), "not yet blocked at {i}");
        }
        assert_eq!(t.record_mfa_failure_at("tok", now), MFA_MAX_FAILURES);
        assert!(t.mfa_blocked_at("tok", now), "blocked at the threshold");
        // Another token is independent.
        assert!(!t.mfa_blocked_at("other", now));
        // The entry expires after its TTL (the token itself is long dead
        // by then).
        let later = now + MFA_ENTRY_TTL + Duration::from_secs(1);
        assert!(!t.mfa_blocked_at("tok", later));
    }

    #[test]
    fn clear_mfa_drops_the_counter() {
        let t = LoginThrottle::default();
        for _ in 0..MFA_MAX_FAILURES {
            t.record_mfa_failure("tok");
        }
        assert!(t.mfa_blocked("tok"));
        t.clear_mfa("tok");
        assert!(!t.mfa_blocked("tok"));
    }

    #[test]
    fn totp_steps_are_single_use_and_monotonic() {
        let t = LoginThrottle::default();
        let now = Instant::now();
        assert!(t.consume_totp_step_at("u1", 100, now), "first use is fresh");
        assert!(!t.consume_totp_step_at("u1", 100, now), "same step replays");
        assert!(
            !t.consume_totp_step_at("u1", 99, now),
            "older step (skew window) replays"
        );
        assert!(t.consume_totp_step_at("u1", 101, now), "next step is fresh");
        // A different user is tracked independently.
        assert!(t.consume_totp_step_at("u2", 100, now));
        // After the retention window the record is stale and any step is
        // accepted again (the code itself expired long before).
        let later = now + STEP_ENTRY_TTL + Duration::from_secs(1);
        assert!(t.consume_totp_step_at("u1", 101, later));
    }
}
