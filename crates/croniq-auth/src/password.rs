//! Password hashing and verification using bcrypt, plus the shared
//! password length policy enforced everywhere a password is accepted
//! (user create, change-password, password-reset confirm, invitation
//! accept, and `croniq init`).

use crate::jwt::AuthError;

const BCRYPT_COST: u32 = 12;

/// Minimum accepted password length in bytes. One shared constant so the
/// API endpoints and the CLI cannot drift apart (issue #428).
pub const PASSWORD_MIN_LEN: usize = 8;

/// Maximum accepted password length in bytes. bcrypt silently truncates
/// input beyond 72 bytes, so characters past that boundary would add no
/// entropy while pretending to — reject them explicitly instead.
pub const PASSWORD_MAX_BYTES: usize = 72;

/// A constant bcrypt hash at the same cost as real credentials
/// ([`BCRYPT_COST`]). [`dummy_verify`] runs against it when no credential
/// row exists so an unknown username costs the same wall-clock time as a
/// wrong password — without this, the fast-path 401 is a timing oracle
/// for username enumeration (issue #428). The preimage is irrelevant:
/// the verification result is discarded.
const DUMMY_HASH: &str = "$2b$12$YXBntkEkvH.iUqbB0dvGWu0jI.OucauI6J6sQtQdopM1sNckRKPoi";

/// Validate the shared password length policy. Returns a human-readable
/// message suitable for CLI output; API handlers typically map any `Err`
/// to `400 Bad Request`.
pub fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < PASSWORD_MIN_LEN {
        return Err(format!(
            "password must be at least {PASSWORD_MIN_LEN} characters"
        ));
    }
    if password.len() > PASSWORD_MAX_BYTES {
        return Err(format!(
            "password must be at most {PASSWORD_MAX_BYTES} bytes — bcrypt ignores anything longer"
        ));
    }
    Ok(())
}

/// Hash a password with bcrypt.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    bcrypt::hash(password, BCRYPT_COST).map_err(|e| AuthError::Store(e.to_string()))
}

/// Verify a password against a bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AuthError> {
    bcrypt::verify(password, hash).map_err(|e| AuthError::Store(e.to_string()))
}

/// Burn one bcrypt verification against [`DUMMY_HASH`] and discard the
/// result. Called on the no-credential and locked-account login paths so
/// their response time matches the wrong-password path.
pub fn dummy_verify(password: &str) {
    let _ = bcrypt::verify(password, DUMMY_HASH);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_round_trip() {
        let hash = hash_password("my-secret-password").unwrap();
        assert!(verify_password("my-secret-password", &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn dummy_hash_is_a_valid_cost_12_bcrypt_hash() {
        // If the constant were malformed, bcrypt::verify would error and
        // dummy_verify would return instantly — silently reopening the
        // timing oracle. Guard the format and the cost.
        assert!(DUMMY_HASH.starts_with("$2b$12$"));
        assert!(bcrypt::verify("any-password", DUMMY_HASH).is_ok());
        dummy_verify("any-password"); // must not panic
    }

    #[test]
    fn validate_password_enforces_shared_bounds() {
        assert!(
            validate_password("1234567").is_err(),
            "7 chars is too short"
        );
        assert!(
            validate_password("12345678").is_ok(),
            "8 chars is the minimum"
        );
        assert!(
            validate_password(&"x".repeat(72)).is_ok(),
            "72 bytes is the maximum"
        );
        let err = validate_password(&"x".repeat(73)).unwrap_err();
        assert!(err.contains("72"), "message names the limit: {err}");
        // Multi-byte input counts in bytes, matching what bcrypt truncates.
        assert!(
            validate_password(&"ä".repeat(37)).is_err(),
            "74 bytes via 2-byte chars"
        );
    }
}
