//! At-rest encryption for sensitive secrets (currently: TOTP seeds).
//!
//! AES-256-GCM with a 32-byte key derived from `CRONIQ_JWT_SECRET` via
//! HKDF-SHA256. Reusing the JWT secret is intentional — anyone with
//! read access to it can already mint admin tokens, so adding a second
//! key store wouldn't raise the bar. The wrap is mainly a defence
//! against DB-only exfiltration (a leaked SQLite snapshot shouldn't
//! immediately leak working 2FA codes).
//!
//! Wrap format on disk: `base64(nonce || ciphertext || tag)` where the
//! nonce is 12 bytes (Aes256Gcm default) and the tag is 16 bytes
//! (appended by GCM).
//!
//! There is deliberately no key identifier in that format. Telling two keys
//! apart is done by trial decryption instead, which is sound here rather than
//! merely convenient: GCM authenticates, so the wrong key fails the tag check
//! rather than yielding plausible garbage. That is what makes
//! [`unwrap_totp_secret_with_previous`] — and the boot-time re-wrap built on
//! it (issue #531) — able to report exactly which key a row is under.

use aes_gcm::aead::{Aead, AeadCore, OsRng};
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;

const TOTP_KEY_INFO: &[u8] = b"croniq-totp-v1";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed (key change or tampered ciphertext)")]
    Decrypt,
    #[error("invalid wrapped format")]
    InvalidFormat,
}

fn derive_totp_key(jwt_secret: &str) -> Key<Aes256Gcm> {
    let hk = Hkdf::<Sha256>::new(None, jwt_secret.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(TOTP_KEY_INFO, &mut okm)
        .expect("HKDF expand for 32 bytes never fails");
    *Key::<Aes256Gcm>::from_slice(&okm)
}

/// Wrap a TOTP seed (or any short secret) with AES-256-GCM.
///
/// Output: base64-encoded `nonce || ciphertext || tag`.
pub fn wrap_totp_secret(jwt_secret: &str, plaintext: &[u8]) -> Result<String, CryptoError> {
    let key = derive_totp_key(jwt_secret);
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| CryptoError::Encrypt)?;

    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ct);
    Ok(B64.encode(out))
}

/// Inverse of [`wrap_totp_secret`]. Returns the raw plaintext bytes.
pub fn unwrap_totp_secret(jwt_secret: &str, wrapped: &str) -> Result<Vec<u8>, CryptoError> {
    let raw = B64
        .decode(wrapped)
        .map_err(|_| CryptoError::InvalidFormat)?;
    if raw.len() < 12 + 16 {
        // Need at least nonce (12) + tag (16); no plaintext allowed.
        return Err(CryptoError::InvalidFormat);
    }
    let (nonce_bytes, ct) = raw.split_at(12);
    let nonce_arr: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| CryptoError::InvalidFormat)?;
    let nonce = Nonce::from(nonce_arr);
    let key = derive_totp_key(jwt_secret);
    let cipher = Aes256Gcm::new(&key);
    cipher.decrypt(&nonce, ct).map_err(|_| CryptoError::Decrypt)
}

/// Which of the two candidate keys a wrapped secret turned out to be under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappedUnder {
    /// The key derived from the current JWT secret — the ordinary case.
    Current,
    /// The key derived from `CRONIQ_JWT_SECRET_PREVIOUS`. The row predates a
    /// rotation and has not been re-wrapped yet; the caller should say so.
    Previous,
}

/// [`unwrap_totp_secret`] with a second key to fall back on.
///
/// Rotating the JWT secret rotates the at-rest wrap key with it, which used to
/// mean every enrolled user had to re-enrol (issue #531). Naming the old value
/// in `CRONIQ_JWT_SECRET_PREVIOUS` makes rows written before the rotation
/// readable again, and the returned [`WrappedUnder`] tells the caller whether
/// that fallback was needed so it can re-wrap the row and log the fact.
///
/// The previous key is only ever tried for *unwrapping*. It never signs, never
/// validates a token, and never wraps a new secret — [`wrap_totp_secret`] has
/// no such parameter, so anything written after the rotation is under the
/// current key by construction.
///
/// A malformed wrapper is not a key problem, so [`CryptoError::InvalidFormat`]
/// returns immediately rather than burning a second trial decryption. When
/// both keys fail the tag check the error describes the current key, which is
/// the one the caller is expected to act on.
pub fn unwrap_totp_secret_with_previous(
    current: &str,
    previous: Option<&str>,
    wrapped: &str,
) -> Result<(Vec<u8>, WrappedUnder), CryptoError> {
    match unwrap_totp_secret(current, wrapped) {
        Ok(pt) => Ok((pt, WrappedUnder::Current)),
        Err(CryptoError::InvalidFormat) => Err(CryptoError::InvalidFormat),
        Err(e) => match previous {
            Some(prev) => match unwrap_totp_secret(prev, wrapped) {
                Ok(pt) => Ok((pt, WrappedUnder::Previous)),
                Err(_) => Err(e),
            },
            None => Err(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_round_trip() {
        let plaintext = b"JBSWY3DPEHPK3PXP"; // example TOTP seed
        let wrapped = wrap_totp_secret("jwt-secret", plaintext).unwrap();
        let unwrapped = unwrap_totp_secret("jwt-secret", &wrapped).unwrap();
        assert_eq!(unwrapped, plaintext);
    }

    #[test]
    fn different_secret_fails_to_decrypt() {
        let wrapped = wrap_totp_secret("secret-a", b"plaintext").unwrap();
        let result = unwrap_totp_secret("secret-b", &wrapped);
        assert_eq!(result, Err(CryptoError::Decrypt));
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let mut wrapped = wrap_totp_secret("secret", b"plaintext").unwrap();
        // Flip a bit late in the b64 string — likely lands in ciphertext or tag.
        let last = wrapped.pop().unwrap();
        let flipped = ((last as u8) ^ 1) as char;
        wrapped.push(flipped);
        // Could either be Decrypt (tag mismatch) or InvalidFormat (b64 error);
        // either way it must not silently succeed.
        let result = unwrap_totp_secret("secret", &wrapped);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_ciphertext_is_invalid_format() {
        let result = unwrap_totp_secret("secret", "Y3Jvbmlx"); // 6 bytes, < 12 + 16
        assert_eq!(result, Err(CryptoError::InvalidFormat));
    }

    #[test]
    fn previous_key_unwraps_a_row_written_before_a_rotation() {
        let wrapped = wrap_totp_secret("old-secret", b"JBSWY3DPEHPK3PXP").unwrap();
        let (pt, under) =
            unwrap_totp_secret_with_previous("new-secret", Some("old-secret"), &wrapped).unwrap();
        assert_eq!(pt, b"JBSWY3DPEHPK3PXP");
        assert_eq!(under, WrappedUnder::Previous);
    }

    #[test]
    fn current_key_is_tried_first_and_reported_as_such() {
        let wrapped = wrap_totp_secret("new-secret", b"seed").unwrap();
        let (pt, under) =
            unwrap_totp_secret_with_previous("new-secret", Some("old-secret"), &wrapped).unwrap();
        assert_eq!(pt, b"seed");
        assert_eq!(under, WrappedUnder::Current);
    }

    #[test]
    fn a_row_under_neither_key_reports_the_current_keys_error() {
        let wrapped = wrap_totp_secret("third-secret", b"seed").unwrap();
        let result = unwrap_totp_secret_with_previous("new-secret", Some("old-secret"), &wrapped);
        assert_eq!(result.map(|(pt, _)| pt), Err(CryptoError::Decrypt));
    }

    #[test]
    fn no_previous_key_behaves_exactly_like_the_plain_unwrap() {
        let wrapped = wrap_totp_secret("old-secret", b"seed").unwrap();
        assert_eq!(
            unwrap_totp_secret_with_previous("new-secret", None, &wrapped).map(|(pt, _)| pt),
            Err(CryptoError::Decrypt)
        );
    }

    #[test]
    fn malformed_input_is_a_format_error_under_either_key() {
        // Not a key problem — must not be reported as one, with or without a
        // previous key to fall back on.
        let result = unwrap_totp_secret_with_previous("new", Some("old"), "Y3Jvbmlx");
        assert_eq!(result.map(|(pt, _)| pt), Err(CryptoError::InvalidFormat));
    }

    #[test]
    fn wrapping_never_uses_the_previous_key() {
        // The rotation is one-way: a secret written after it must be readable
        // with the current key alone, or dropping CRONIQ_JWT_SECRET_PREVIOUS
        // would break the very rows the re-wrap just fixed.
        let wrapped = wrap_totp_secret("new-secret", b"seed").unwrap();
        assert_eq!(unwrap_totp_secret("new-secret", &wrapped).unwrap(), b"seed");
        assert_eq!(
            unwrap_totp_secret("old-secret", &wrapped),
            Err(CryptoError::Decrypt)
        );
    }

    #[test]
    fn each_wrap_produces_different_ciphertext() {
        // Same plaintext + key but unique nonce per call.
        let a = wrap_totp_secret("secret", b"same-plaintext").unwrap();
        let b = wrap_totp_secret("secret", b"same-plaintext").unwrap();
        assert_ne!(a, b);
    }
}
