//! In-memory store — uses SQLite in-memory mode.
//! Convenient alias for tests and development.

use crate::sqlite::SqliteStore;
use crate::traits::StoreError;

/// Create an in-memory store backed by SQLite.
pub fn create_memory_store() -> Result<SqliteStore, StoreError> {
    SqliteStore::in_memory()
}
