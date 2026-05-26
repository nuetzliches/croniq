//! croniq-auth: Authentication and authorization for Croniq.
//!
//! Provides:
//! - JWT token issuance and validation
//! - API key hashing and verification
//! - Password hashing and login flow
//! - Caller context and scope model

pub mod api_key;
pub mod context;
pub mod crypto;
pub mod jwt;
pub mod jwt_secret;
pub mod password;
pub mod totp;

pub use context::{AuthMethod, CallerContext, CallerType, Scope, default_scopes_for_role};
pub use jwt::{JWT_ISSUER, JwtConfig, TokenPair};
// Re-export Role so consumers can use `croniq_auth::Role` without a
// separate `croniq_store` dependency just for the enum.
pub use croniq_store::models::Role;
