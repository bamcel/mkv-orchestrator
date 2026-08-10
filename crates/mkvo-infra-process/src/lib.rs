//! Safe external-tool execution and media probing for MKVO.
//!
//! This crate deliberately exposes structured tool identities and argument lists.
//! It never accepts a shell command string and never invokes a platform shell.

mod adapter;
mod probe;
mod runner;
mod tools;

pub use adapter::ProcessToolExecutor;
pub use probe::{
    AttachmentMetadata, ContainerMetadata, FfprobeDocument, MediaProbe, MediaScanAdapter,
    MkvMergeDocument, ProbeError, ScannedMedia, TrackMetadata, TrackType, parse_ffprobe_json,
    parse_mkvmerge_json,
};
pub use runner::{
    OutputStream, ProcessError, ProcessEvent, ProcessOutput, ProcessRunner, ProcessSpec,
};
pub use tools::{ResolvedTool, ToolKind, ToolRegistry, ToolRegistryBuilder, ToolStatus};
