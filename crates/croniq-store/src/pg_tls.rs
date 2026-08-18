//! TLS for the PostgreSQL backend (issue #431).
//!
//! `PgStore::connect` used to hand `NoTls` to the driver unconditionally.
//! Against a local or unix-socket database that is fine; against a remote one
//! the connection password and every row the auth tables return — password
//! hashes, TOTP secrets, API-key hashes — crossed the network in cleartext.
//!
//! The connector is rustls-based, deliberately: OpenSSL would put a C toolchain
//! and a system library into a tree that has neither, and `tokio-postgres-rustls`
//! is MIT/Apache-2.0 like everything else here.
//!
//! # Modes
//!
//! Resolution order, highest first:
//!
//! 1. `sslmode=` in the connection string (libpq spelling, so existing
//!    connection strings keep working).
//! 2. `CRONIQ_PG_SSLMODE`.
//! 3. The default: `require` when every host is remote, `prefer` when the
//!    connection is loopback or a unix socket.
//!
//! | Mode      | Behaviour                                                    |
//! |-----------|--------------------------------------------------------------|
//! | `disable` | No TLS. Only sane for a unix socket or a trusted local host.  |
//! | `prefer`  | TLS when the server offers it, cleartext when it does not.    |
//! | `require` | TLS or no connection at all.                                  |
//!
//! # Certificate verification
//!
//! Whenever TLS is used the server certificate is verified — there is no
//! "encrypt but do not check who you are talking to" mode, because that stops
//! exactly zero of the attacks this change is about. This is stricter than
//! libpq, where `sslmode=require` skips verification and only `verify-full`
//! checks the chain; a Croniq `require` behaves like libpq's `verify-full`.
//!
//! Roots come from the platform trust store plus the Mozilla bundle. A private
//! CA (an internal PKI, or Amazon RDS's `rds-ca-…` bundle) is added by pointing
//! `CRONIQ_PG_ROOT_CERT` at a PEM file.

use std::sync::Arc;

use crate::traits::StoreError;

/// Env var naming the effective SSL mode when the connection string does not.
pub const SSLMODE_ENV: &str = "CRONIQ_PG_SSLMODE";

/// Env var pointing at an additional PEM bundle of trusted CA certificates.
pub const ROOT_CERT_ENV: &str = "CRONIQ_PG_ROOT_CERT";

/// The three modes Croniq supports, mirroring the libpq names that overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
}

impl SslMode {
    fn parse(raw: &str) -> Result<Self, StoreError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "disable" => Ok(SslMode::Disable),
            "prefer" | "allow" => Ok(SslMode::Prefer),
            // libpq's verify-ca / verify-full both mean "TLS, and check the
            // certificate", which is what `Require` already does here.
            "require" | "verify-ca" | "verify-full" => Ok(SslMode::Require),
            other => Err(StoreError::Database(format!(
                "unknown sslmode {other:?}: expected disable, prefer or require"
            ))),
        }
    }
}

/// Whether `spec` names an sslmode itself. `postgres::Config::get_ssl_mode`
/// cannot answer this — it returns the driver default when the setting is
/// absent, which is indistinguishable from an explicit `sslmode=prefer`.
fn spec_sets_sslmode(spec: &str) -> bool {
    spec.to_ascii_lowercase().contains("sslmode")
}

/// True when every host in the connection is loopback or a unix socket, i.e.
/// when traffic never leaves the machine.
pub fn is_local_connection(config: &postgres::Config) -> bool {
    let hosts = config.get_hosts();
    if hosts.is_empty() {
        // No host at all means libpq's default, which is a local socket.
        return true;
    }
    hosts.iter().all(|h| match h {
        // `Host::Unix` only exists on unix targets — a socket path never
        // leaves the machine, so it is always local.
        #[cfg(unix)]
        postgres::config::Host::Unix(_) => true,
        postgres::config::Host::Tcp(name) => {
            let name = name.trim_start_matches('[').trim_end_matches(']');
            name.eq_ignore_ascii_case("localhost")
                || name
                    .parse::<std::net::IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false)
        }
    })
}

/// Resolve the effective mode for this connection. See the module docs for the
/// precedence rules.
pub fn resolve_ssl_mode(spec: &str, config: &postgres::Config) -> Result<SslMode, StoreError> {
    if spec_sets_sslmode(spec) {
        return Ok(match config.get_ssl_mode() {
            postgres::config::SslMode::Disable => SslMode::Disable,
            postgres::config::SslMode::Require => SslMode::Require,
            // `Prefer` and any future variant fall here. Preferring is the
            // driver's own default, so this is also the safe landing spot.
            _ => SslMode::Prefer,
        });
    }
    if let Ok(raw) = std::env::var(SSLMODE_ENV)
        && !raw.trim().is_empty()
    {
        return SslMode::parse(&raw);
    }
    // Nothing configured. A remote database gets TLS demanded, because the
    // whole point of #431 is that cleartext across a network is not a default
    // anyone chose. A loopback or unix-socket connection only prefers it, so
    // the common "Postgres on the same host with TLS switched off" setup keeps
    // working untouched.
    Ok(if is_local_connection(config) {
        SslMode::Prefer
    } else {
        SslMode::Require
    })
}

/// Build the rustls connector, trusting the platform store, the Mozilla
/// bundle, and anything in [`ROOT_CERT_ENV`].
pub fn make_connector() -> Result<tokio_postgres_rustls::MakeRustlsConnect, StoreError> {
    let mut roots = rustls::RootCertStore::empty();

    // Platform trust store first: an internal PKI is normally installed there,
    // so the common corporate case needs no extra configuration.
    match rustls_native_certs::load_native_certs() {
        result if result.certs.is_empty() && !result.errors.is_empty() => {
            return Err(StoreError::Database(format!(
                "could not read the platform certificate store for the Postgres TLS \
                 connection: {:?}. Point {ROOT_CERT_ENV} at a PEM bundle, or set \
                 {SSLMODE_ENV}=disable if this database is genuinely local.",
                result.errors
            )));
        }
        result => {
            for cert in result.certs {
                // A malformed certificate in the OS store is the OS store's
                // problem; skip it rather than refusing to start.
                let _ = roots.add(cert);
            }
        }
    }

    // Public CAs, for managed databases that present a publicly-issued cert.
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    if let Ok(path) = std::env::var(ROOT_CERT_ENV)
        && !path.trim().is_empty()
    {
        let added = load_pem_roots(path.trim(), &mut roots)?;
        tracing_note(format!(
            "loaded {added} additional CA certificate(s) from {path} for the Postgres TLS connection"
        ));
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| StoreError::Database(format!("rustls protocol setup failed: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// Add every certificate in the PEM file at `path` to `roots`, returning how
/// many were added. An unreadable or empty file is an error: the operator
/// asked for these roots, so silently continuing without them would mean
/// failing the handshake later with a far more confusing message.
fn load_pem_roots(path: &str, roots: &mut rustls::RootCertStore) -> Result<usize, StoreError> {
    use rustls_pki_types::pem::PemObject;

    let certs: Vec<_> = rustls_pki_types::CertificateDer::pem_file_iter(path)
        .map_err(|e| {
            StoreError::Database(format!("could not read {ROOT_CERT_ENV} file {path}: {e}"))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            StoreError::Database(format!("could not parse {ROOT_CERT_ENV} file {path}: {e}"))
        })?;

    if certs.is_empty() {
        return Err(StoreError::Database(format!(
            "{ROOT_CERT_ENV} file {path} contains no certificates"
        )));
    }
    let n = certs.len();
    for cert in certs {
        roots
            .add(cert)
            .map_err(|e| StoreError::Database(format!("invalid certificate in {path}: {e}")))?;
    }
    Ok(n)
}

/// croniq-store has no tracing dependency, and adding one for a single startup
/// line is not worth it. Connection setup happens once, on stderr, before the
/// subscriber matters.
fn tracing_note(msg: String) {
    eprintln!("croniq-store: {msg}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(spec: &str) -> postgres::Config {
        spec.parse().expect("valid connection string")
    }

    #[test]
    fn remote_connections_require_tls_by_default() {
        // The regression this closes: a remote database used to be reached
        // over cleartext with no opt-in and no warning.
        let spec = "postgres://croniq:pw@db.internal:5432/croniq";
        assert_eq!(
            resolve_ssl_mode(spec, &cfg(spec)).unwrap(),
            SslMode::Require
        );
    }

    #[test]
    fn local_connections_only_prefer_tls() {
        // Postgres on the same host with TLS switched off is the single most
        // common development setup; demanding TLS there would break it for no
        // security gain.
        for spec in [
            "postgres://croniq@localhost:5432/croniq",
            "postgres://croniq@127.0.0.1:5432/croniq",
            // A socket path only parses as `Host::Unix` on unix; on Windows
            // the driver reads it as a TCP hostname, and treating an arbitrary
            // name as local would be wrong.
            #[cfg(unix)]
            "host=/var/run/postgresql user=croniq dbname=croniq",
        ] {
            assert_eq!(
                resolve_ssl_mode(spec, &cfg(spec)).unwrap(),
                SslMode::Prefer,
                "{spec} should only prefer TLS"
            );
        }
    }

    #[test]
    fn connection_string_sslmode_wins() {
        let spec = "postgres://croniq@db.internal/croniq?sslmode=disable";
        assert_eq!(
            resolve_ssl_mode(spec, &cfg(spec)).unwrap(),
            SslMode::Disable
        );

        let spec = "host=localhost user=croniq dbname=croniq sslmode=require";
        assert_eq!(
            resolve_ssl_mode(spec, &cfg(spec)).unwrap(),
            SslMode::Require
        );
    }

    #[test]
    fn is_local_connection_classifies_hosts() {
        assert!(is_local_connection(&cfg("host=localhost user=u")));
        assert!(is_local_connection(&cfg("host=::1 user=u")));
        assert!(!is_local_connection(&cfg("host=10.0.0.5 user=u")));
        assert!(!is_local_connection(&cfg("host=db.example.com user=u")));
        // Mixed: one remote host is enough to treat the connection as remote.
        assert!(!is_local_connection(&cfg(
            "host=localhost,db.example.com user=u"
        )));
    }

    #[test]
    fn mode_parsing_maps_libpq_names() {
        assert_eq!(SslMode::parse("DISABLE").unwrap(), SslMode::Disable);
        assert_eq!(SslMode::parse(" prefer ").unwrap(), SslMode::Prefer);
        // Croniq always verifies, so the verify-* spellings are just `require`.
        assert_eq!(SslMode::parse("verify-full").unwrap(), SslMode::Require);
        assert!(SslMode::parse("maybe").is_err());
    }

    #[test]
    fn connector_builds_against_the_platform_trust_store() {
        // Catches a broken rustls provider/root-store wiring without needing a
        // live database — the part of the TLS path that is testable offline.
        make_connector().expect("connector builds");
    }
}
