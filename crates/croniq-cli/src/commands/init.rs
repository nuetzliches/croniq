//! `croniq init` — initialize database with admin user and default API client.

use std::path::Path;

use chrono::Utc;
use croniq_auth::api_key::{generate_api_key, hash_api_key};
use croniq_auth::password::hash_password;
use croniq_store::models::{ApiClient, ApiKey, PasswordCredential};
use croniq_store::sqlite::SqliteStore;
use croniq_store::traits::AuthStore;
use miette::{IntoDiagnostic, Result, miette};
use uuid::Uuid;

pub fn init(
    data_dir: &Path,
    username: &str,
    password: Option<&str>,
    api_key_override: Option<&str>,
) -> Result<()> {
    // Prompt for password if not given
    let password = match password {
        Some(p) => p.to_string(),
        None => {
            eprintln!("Enter admin password: ");
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).into_diagnostic()?;
            let p = buf.trim().to_string();
            if p.is_empty() {
                return Err(miette!("Password cannot be empty"));
            }
            p
        }
    };

    // Ensure data dir exists
    std::fs::create_dir_all(data_dir).into_diagnostic()?;
    let db_path = data_dir.join("croniq.db");

    println!("Opening database at {}", db_path.display());
    let store = SqliteStore::open(&db_path)
        .map_err(|e| miette!("Failed to open database: {e}"))?;

    let now = Utc::now();
    let user_id = Uuid::new_v4().to_string();

    // 1. Create admin user credentials
    let pw_hash = hash_password(&password)
        .map_err(|e| miette!("Failed to hash password: {e}"))?;

    store
        .upsert_credentials(&PasswordCredential {
            user_id: user_id.clone(),
            username: username.to_string(),
            password_hash: pw_hash,
            failed_attempts: 0,
            locked_until: None,
            created_at: now,
        })
        .map_err(|e| miette!("Failed to create admin user: {e}"))?;

    println!("Admin user '{}' created.", username);

    // 2. Create default API client with admin scope
    let client_id = Uuid::new_v4().to_string();
    store
        .create_client(&ApiClient {
            client_id: client_id.clone(),
            name: "default".to_string(),
            scopes: vec!["admin".to_string()],
            is_active: true,
            created_at: now,
        })
        .map_err(|e| miette!("Failed to create API client: {e}"))?;

    println!("API client 'default' created (id: {}).", client_id);

    // 3. Generate an API key for the default client (or use the override)
    let (raw_key, key_hash, prefix) = match api_key_override {
        Some(key) => {
            if !key.starts_with("croniq_") {
                return Err(miette!("--api-key must start with 'croniq_'"));
            }
            let hash = hash_api_key(key);
            let prefix = key.chars().take(12).collect();
            (key.to_string(), hash, prefix)
        }
        None => generate_api_key(),
    };
    let key_id = Uuid::new_v4().to_string();

    store
        .create_api_key(&ApiKey {
            key_id,
            client_id,
            key_hash,
            key_prefix: prefix,
            expires_at: None,
            revoked_at: None,
            created_at: now,
        })
        .map_err(|e| miette!("Failed to create API key: {e}"))?;

    println!();
    println!("=== Initialization complete ===");
    println!();
    println!("Admin login:");
    println!("  Username: {}", username);
    println!("  Password: (as provided)");
    println!();
    println!("API Key (save this — it won't be shown again):");
    println!("  {}", raw_key);
    println!();
    println!("Use the API key with:");
    println!("  Authorization: ApiKey {}", raw_key);
    println!();
    println!("Or login via:");
    println!("  POST /v1/auth/login {{\"username\": \"{}\", \"password\": \"...\"}}", username);

    Ok(())
}
