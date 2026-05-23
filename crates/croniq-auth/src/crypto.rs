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
    fn each_wrap_produces_different_ciphertext() {
        // Same plaintext + key but unique nonce per call.
        let a = wrap_totp_secret("secret", b"same-plaintext").unwrap();
        let b = wrap_totp_secret("secret", b"same-plaintext").unwrap();
        assert_ne!(a, b);
    }
}
