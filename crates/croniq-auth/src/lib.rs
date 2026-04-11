//! croniq-auth: Authentication and authorization for Croniq.
//!
//! Provides:
//! - JWT token issuance and validation
//! - API key hashing and verification
//! - Password hashing and login flow
//! - Caller context and scope model

pub mod api_key;
pub mod context;
pub mod jwt;
pub mod password;

pub use context::{CallerContext, CallerType, Scope};
pub use jwt::{JwtConfig, TokenPair};
