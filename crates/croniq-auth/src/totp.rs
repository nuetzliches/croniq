//! TOTP/2FA setup, verification, and recovery-code generation.
//!
//! Authenticator-compatible (Google Authenticator, 1Password, Bitwarden,
//! Aegis, etc.): SHA1, 6 digits, 30 s window, ±1 step skew. Setup
//! returns:
//!   - the raw base32 seed (so the user can type it manually if the
//!     QR scan fails) embedded in an `otpauth://` URL
//!   - 10 single-use recovery codes (8 lowercase alphanumerics each)
//!
//! All raw values are emitted **once** at setup time. Persisted state:
//!   - secret_enc: AES-256-GCM wrap of the seed
//!   - recovery_codes.code_hash: SHA-256 of each raw code
//!
//! Step-up flow (handled by the server):
//!   1. POST /v1/auth/login  → if `totp_secrets.enabled = true`, response
//!      is `{ requires_totp: true, mfa_token }` instead of access tokens
//!   2. POST /v1/auth/login/totp { mfa_token, code | recovery_code }
//!      → normal access + refresh tokens

use rand::Rng;
use sha2::{Digest, Sha256};
use thiserror::Error;
use totp_rs::{Algorithm, Secret, TOTP};

const TOTP_ALGO: Algorithm = Algorithm::SHA1;
const TOTP_DIGITS: usize = 6;
const TOTP_STEP_SECS: u64 = 30;
/// Number of steps before/after to accept for clock skew. 1 means the
/// previous and next 30-second windows count as valid — total window
/// ~60-90 s depending on the call moment.
const TOTP_SKEW: u8 = 1;
/// Issuer + account label rendered in the `otpauth://` URL. Most
/// authenticator apps display "Croniq · alex".
const TOTP_ISSUER: &str = "Croniq";

/// Number of recovery codes minted per setup / regenerate.
pub const RECOVERY_CODE_COUNT: usize = 10;
/// Character length of each recovery code.
const RECOVERY_CODE_LEN: usize = 8;
const RECOVERY_CHARSET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789"; // no 0/o/1/i/l

#[derive(Debug, Error)]
pub enum TotpError {
    #[error("invalid base32 secret")]
    InvalidSecret,
    #[error("TOTP construction failed: {0}")]
    Construct(String),
}

/// A fresh TOTP enrolment package. The raw `secret_b32` is shown to the
/// user once (so they can type it manually); the otpauth URL is the
/// scannable QR-code payload.
pub struct TotpEnrolment {
    /// Base32-encoded TOTP seed (the value the user copies into an
    /// authenticator app or scans via QR).
    pub secret_b32: String,
    /// `otpauth://totp/<issuer>:<account>?secret=...&issuer=...`
    pub otpauth_url: String,
    /// Raw 8-char alphanumeric codes for break-glass recovery. Shown
    /// ONCE; only their SHA-256 hashes are persisted via
    /// [`hash_recovery_code`].
    pub recovery_codes: Vec<String>,
}

/// Generate a new TOTP enrolment for the given user. The caller stores
/// the encrypted secret + recovery code hashes; the raw values in the
/// returned struct must be delivered to the user immediately and
/// dropped.
pub fn enroll_user(account: &str) -> Result<TotpEnrolment, TotpError> {
    let secret = Secret::generate_secret();
    let secret_b32 = secret.to_encoded().to_string();

    let totp = TOTP::new(
        TOTP_ALGO,
        TOTP_DIGITS,
        TOTP_SKEW,
        TOTP_STEP_SECS,
        secret.to_bytes().map_err(|_| TotpError::InvalidSecret)?,
    )
    .map_err(|e| TotpError::Construct(e.to_string()))?;

    // Manual otpauth URL — totp-rs has a builder but it requires
    // issuer/account in the constructor, which forces us to recreate
    // the TOTP per-user. Building the URL ourselves keeps the function
    // pure and the issuer label stable across the workspace.
    let otpauth_url = format!(
        "otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits=6&period=30",
        issuer = urlencoding::encode(TOTP_ISSUER),
        account = urlencoding::encode(account),
        secret = secret_b32,
    );

    let recovery_codes = (0..RECOVERY_CODE_COUNT)
        .map(|_| generate_recovery_code())
        .collect();

    // Suppress unused-variable warning if logging is added later.
    let _ = totp;

    Ok(TotpEnrolment {
        secret_b32,
        otpauth_url,
        recovery_codes,
    })
}

/// Verify a 6-digit TOTP code against the user's stored seed
/// (base32-encoded after AES-GCM unwrap).
pub fn verify_code(secret_b32: &str, code: &str) -> Result<bool, TotpError> {
    if code.len() != TOTP_DIGITS || !code.chars().all(|c| c.is_ascii_digit()) {
        return Ok(false);
    }
    let secret = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|_| TotpError::InvalidSecret)?;
    let totp = TOTP::new(TOTP_ALGO, TOTP_DIGITS, TOTP_SKEW, TOTP_STEP_SECS, secret)
        .map_err(|e| TotpError::Construct(e.to_string()))?;
    totp.check_current(code)
        .map_err(|e| TotpError::Construct(e.to_string()))
}

/// Generate a single 8-char lowercase alphanumeric recovery code.
/// Exposed publicly so the regenerate endpoint can mint a fresh set
/// without going through full `enroll_user` (which also re-generates
/// the TOTP secret).
pub fn generate_recovery_code() -> String {
    let mut rng = rand::rng();
    (0..RECOVERY_CODE_LEN)
        .map(|_| {
            let idx = rng.random_range(0..RECOVERY_CHARSET.len());
            RECOVERY_CHARSET[idx] as char
        })
        .collect()
}

/// Generate a fresh set of [`RECOVERY_CODE_COUNT`] recovery codes.
pub fn generate_recovery_codes() -> Vec<String> {
    (0..RECOVERY_CODE_COUNT)
        .map(|_| generate_recovery_code())
        .collect()
}

/// Hash a recovery code for storage. Same SHA-256-hex pattern as API
/// keys / invitations / password resets. Trims and lowercases first so
/// `XYZ123ab` and `xyz123ab` match (paste-from-PDF UX).
pub fn hash_recovery_code(raw: &str) -> String {
    let normalised = raw.trim().to_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(normalised.as_bytes());
    hex::encode(hasher.finalize())
}

// (The MFA-step JWT is signed with the same JWT secret + a distinct
// `purpose: "mfa"` claim; no separate blacklist is needed. Server code
// builds it via `croniq_auth::jwt::issue_token_pair` with a tiny TTL.)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrolment_emits_otpauth_url_with_seed() {
        let e = enroll_user("alex@example.org").unwrap();
        assert!(e.otpauth_url.starts_with("otpauth://totp/Croniq:"));
        assert!(e.otpauth_url.contains(&format!("secret={}", e.secret_b32)));
        assert!(e.otpauth_url.contains("issuer=Croniq"));
    }

    #[test]
    fn enrolment_mints_ten_recovery_codes() {
        let e = enroll_user("alex").unwrap();
        assert_eq!(e.recovery_codes.len(), RECOVERY_CODE_COUNT);
        for code in &e.recovery_codes {
            assert_eq!(code.len(), RECOVERY_CODE_LEN);
            assert!(code.chars().all(|c| RECOVERY_CHARSET.contains(&(c as u8))));
        }
        // No duplicates (very unlikely but worth catching).
        let mut sorted = e.recovery_codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), RECOVERY_CODE_COUNT);
    }

    #[test]
    fn verify_rejects_wrong_format() {
        let e = enroll_user("a").unwrap();
        assert!(!verify_code(&e.secret_b32, "12345").unwrap()); // too short
        assert!(!verify_code(&e.secret_b32, "1234567").unwrap()); // too long
        assert!(!verify_code(&e.secret_b32, "abcdef").unwrap()); // not digits
    }

    #[test]
    fn verify_accepts_current_code() {
        // Generate a TOTP, compute the current code via the same lib,
        // then verify it. Round-trip without exposing the internal
        // clock.
        let e = enroll_user("a").unwrap();
        let secret = Secret::Encoded(e.secret_b32.clone()).to_bytes().unwrap();
        let totp = TOTP::new(TOTP_ALGO, TOTP_DIGITS, TOTP_SKEW, TOTP_STEP_SECS, secret).unwrap();
        let code = totp.generate_current().unwrap();
        assert!(verify_code(&e.secret_b32, &code).unwrap());
    }

    #[test]
    fn hash_recovery_code_is_case_insensitive() {
        assert_eq!(
            hash_recovery_code("ABC123de"),
            hash_recovery_code("abc123de")
        );
        assert_eq!(
            hash_recovery_code(" abc123de "),
            hash_recovery_code("abc123de")
        );
    }
}
