//! API key + secret token generation, hashing, and verification.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Generate a new random API key. Returns (raw_key, sha256_hash, prefix).
pub fn generate_api_key() -> (String, String, String) {
    let raw = format!("croniq_{}", Uuid::new_v4().as_simple());
    let hash = hash_api_key(&raw);
    let prefix = raw.chars().take(12).collect();
    (raw, hash, prefix)
}

/// Hash an API key with SHA-256 (hex-encoded).
pub fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Hash any secret token (API key, invitation, password-reset). Aliased
/// for callers where "API key" reads wrong; same SHA-256 mechanism.
pub fn hash_token(raw: &str) -> String {
    hash_api_key(raw)
}

/// Generate a generic secret token with the given prefix. Returns
/// `(raw, sha256_hash)`. The raw token is delivered to the user once;
/// only the hash is persisted. Used by invitations (`croniq_inv_`),
/// password resets (`croniq_pwr_`), and personal access tokens
/// (`croniq_pat_`, PR-A4).
pub fn generate_token(prefix: &str) -> (String, String) {
    let raw = format!("{}_{}", prefix, Uuid::new_v4().as_simple());
    let hash = hash_api_key(&raw);
    (raw, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_key() {
        let (raw, hash, prefix) = generate_api_key();
        assert!(raw.starts_with("croniq_"));
        assert_eq!(hash.len(), 64); // SHA-256 hex
        assert_eq!(prefix.len(), 12);
        assert_eq!(hash, hash_api_key(&raw));
    }

    #[test]
    fn hash_is_deterministic() {
        let key = "croniq_test123";
        assert_eq!(hash_api_key(key), hash_api_key(key));
    }
}
