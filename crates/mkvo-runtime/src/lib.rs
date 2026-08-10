//! Transport-neutral host runtime shared by the Tauri and HTTP frontends.

pub mod compat;
mod composition;
mod error;
mod runtime;
mod watch;
mod workflows;

pub use composition::{
    BrowseScope, FileSecretStore, KeyringSecretStore, MemorySecretStore, MkvoRuntimeBuilder,
    RuntimeConfig, RuntimeDependencies,
};
pub use error::{RuntimeError, RuntimeResult};
pub use runtime::{
    LegacyMigrationReport, LegacyMigrationStatus, LogExport, MkvoRuntime, RecentJobsResponse,
    RecoveryDisposition, StartupRecoveryItem, StartupRecoveryReport,
};
pub use watch::WatchStartReport;

/// Canonical shared DTOs for hosts that do not need the legacy projections.
pub use mkvo_contracts as contracts;
