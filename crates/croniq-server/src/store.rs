//! Shared store type alias for dependency injection.

use croniq_store::sqlite::SqliteStore;
use croniq_store::traits::Store;
use std::sync::Arc;

/// A type-erased, cloneable store that satisfies all store sub-traits via
/// [`croniq_store::traits::Store`]. The same alias is shared with `croniq-mcp`
/// so the in-process MCP service factory can accept the server's store.
pub type DynStore = Arc<dyn Store + Send + Sync>;

/// Convenience constructor: wrap a SqliteStore as a DynStore.
pub fn sqlite_store(store: SqliteStore) -> DynStore {
    Arc::new(store)
}

/// Convenience constructor: wrap a [`PgStoreHandle`] as a DynStore. The handle
/// drives the synchronous `PgStore` on a dedicated OS thread, so it is safe to
/// call from inside the server's async runtime (see the module docs on
/// [`croniq_store::pg_actor`]). Only available with the `postgres` feature.
///
/// [`PgStoreHandle`]: croniq_store::pg_actor::PgStoreHandle
#[cfg(feature = "postgres")]
pub fn pg_store(handle: croniq_store::pg_actor::PgStoreHandle) -> DynStore {
    Arc::new(handle)
}
