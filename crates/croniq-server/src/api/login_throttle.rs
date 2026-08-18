//! In-memory throttling state for the public login surface (issue #428).
//!
//! Three independent guards, all deliberately self-contained (a `Mutex`
//! around a `HashMap`, no external dependencies, no persistence):
//!
//! * **Per-IP sliding window** on `POST /v1/auth/login` and
//!   `POST /v1/auth/login/totp`: at most [`IP_MAX_ATTEMPTS`] attempts per
//!   [`IP_WINDOW`], keyed by the **socket peer address**. `X-Forwarded-For`
//!   is intentionally not parsed — it is attacker-controlled on a directly
//!   exposed server. Deployments behind a reverse proxy see the proxy's
//!   address here and should throttle at the proxy instead. This layer
//!   complements (does not replace) the per-account lockout: the lockout
//!   stops online brute force against one account, the IP window stops one
//!   address from hammering many accounts or from using the lockout itself
//!   as a denial-of-service lever.
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
    ip_attempts: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    mfa_failures: Mutex<HashMap<String, MfaEntry>>,
    consumed_steps: Mutex<HashMap<String, StepEntry>>,
}

impl LoginThrottle {
    /// Record one login attempt from `ip` and report whether it is allowed.
    /// `None` (no `ConnectInfo`, e.g. handler-level tests) always passes —
    /// there is nothing meaningful to key on.
    pub fn allow_ip(&self, ip: Option<IpAddr>) -> bool {
        self.allow_ip_at(ip, Instant::now())
    }

    fn allow_ip_at(&self, ip: Option<IpAddr>, now: Instant) -> bool {
        let Some(ip) = ip else { return true };
        let mut map = self.ip_attempts.lock().unwrap();
        if map.len() > PRUNE_THRESHOLD {
            map.retain(|_, attempts| {
                attempts
                    .last()
                    .is_some_and(|last| now.duration_since(*last) < IP_WINDOW)
            });
        }
        let attempts = map.entry(ip).or_default();
        attempts.retain(|t| now.duration_since(*t) < IP_WINDOW);
        if attempts.len() >= IP_MAX_ATTEMPTS {
            return false;
        }
        attempts.push(now);
        true
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

/// The socket peer address of the request, when the router was served via
/// `into_make_service_with_connect_info::<SocketAddr>()` (as `main.rs`
/// does). `None` when no `ConnectInfo` extension is present — router-level
/// tests and direct handler calls — in which case IP throttling is
/// skipped. Deliberately ignores `X-Forwarded-For` (see module docs).
pub struct ClientIp(pub Option<IpAddr>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.ip()),
        ))
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
