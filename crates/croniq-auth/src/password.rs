//! Password hashing and verification using bcrypt.

use crate::jwt::AuthError;

const BCRYPT_COST: u32 = 12;

/// Hash a password with bcrypt.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    bcrypt::hash(password, BCRYPT_COST).map_err(|e| AuthError::Store(e.to_string()))
}

/// Verify a password against a bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AuthError> {
    bcrypt::verify(password, hash).map_err(|e| AuthError::Store(e.to_string()))
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
}
