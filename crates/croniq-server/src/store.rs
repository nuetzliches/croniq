//! Shared store type alias for dependency injection.

use std::sync::Arc;
use croniq_store::{
    sqlite::SqliteStore,
    traits::{DeadLetterStore, ExecutionStore, JobStore, RunnerStore},
};

/// A type-erased, cloneable store that satisfies all store traits.
/// Replace `Arc<SqliteStore>` everywhere in croniq-server with this type.
pub type DynStore = Arc<dyn StoreExt + Send + Sync>;

/// Supertrait combining all store capabilities.
pub trait StoreExt: JobStore + ExecutionStore + RunnerStore + DeadLetterStore {}

/// Blanket implementation for SqliteStore (and any future PostgresStore etc.)
impl<T: JobStore + ExecutionStore + RunnerStore + DeadLetterStore> StoreExt for T {}

/// Convenience constructor: wrap a SqliteStore as a DynStore.
pub fn sqlite_store(store: SqliteStore) -> DynStore {
    Arc::new(store)
}
