//! Shared store type alias for dependency injection.

use std::sync::Arc;
use croniq_store::{
    sqlite::SqliteStore,
    traits::{
        AuthStore, CalendarDefinitionStore, DeadLetterStore, ExecutionLogStore,
        ExecutionStore, JobDefinitionStore, JobStore, RunnerStore, TriggerDefinitionStore,
    },
};

/// A type-erased, cloneable store that satisfies all store traits.
pub type DynStore = Arc<dyn StoreExt + Send + Sync>;

/// Supertrait combining all store capabilities.
pub trait StoreExt:
    JobStore + ExecutionStore + RunnerStore + DeadLetterStore + AuthStore
    + JobDefinitionStore + TriggerDefinitionStore + CalendarDefinitionStore + ExecutionLogStore {}

impl<T> StoreExt for T where T:
    JobStore + ExecutionStore + RunnerStore + DeadLetterStore + AuthStore
    + JobDefinitionStore + TriggerDefinitionStore + CalendarDefinitionStore + ExecutionLogStore {}

/// Convenience constructor: wrap a SqliteStore as a DynStore.
pub fn sqlite_store(store: SqliteStore) -> DynStore {
    Arc::new(store)
}
