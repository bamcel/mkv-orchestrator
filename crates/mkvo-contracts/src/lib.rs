//! Versioned, transport-neutral DTOs shared by the Tauri and HTTP hosts.

pub mod api;
pub mod features;
pub mod jobs;
pub mod media;

pub use api::*;
pub use features::*;
pub use jobs::*;
pub use media::*;

pub use mkvo_domain::{CONTRACT_VERSION, CorrelationId, IdempotencyKey, JobId, PlanId};
