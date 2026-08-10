//! Pure domain types and deterministic rules for MKV Orchestrator.
//!
//! This crate deliberately has no knowledge of Tauri, HTTP, SQLite, or child
//! processes. Paths remain [`std::path::PathBuf`] values until a transport maps
//! them to display strings.

pub mod fingerprint;
pub mod ids;
pub mod library;
pub mod media;
pub mod natural;
pub mod paths;
pub mod plan;
pub mod propedit;
pub mod remux;
pub mod rename;
pub mod settings;

pub use fingerprint::*;
pub use ids::*;
pub use library::*;
pub use media::*;
pub use natural::*;
pub use paths::*;
pub use plan::*;
pub use propedit::*;
pub use remux::*;
pub use rename::*;
pub use settings::*;

/// Version of the serialized Rust/TypeScript contract family.
pub const CONTRACT_VERSION: u32 = 1;
