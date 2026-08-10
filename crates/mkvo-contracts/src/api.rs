use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use mkvo_domain::{CONTRACT_VERSION, CorrelationId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiEnvelope<T> {
    pub contract_version: u32,
    pub correlation_id: CorrelationId,
    pub data: T,
}

impl<T> ApiEnvelope<T> {
    #[must_use]
    pub fn new(correlation_id: CorrelationId, data: T) -> Self {
        Self {
            contract_version: CONTRACT_VERSION,
            correlation_id,
            data,
        }
    }
}

pub type ApiResult<T> = Result<ApiEnvelope<T>, ApiError>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    pub correlation_id: CorrelationId,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub details: BTreeMap<String, Value>,
}

impl ApiError {
    #[must_use]
    pub fn new(
        code: ApiErrorCode,
        message: impl Into<String>,
        correlation_id: CorrelationId,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            correlation_id,
            retryable: false,
            details: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn retryable(mut self, value: bool) -> Self {
        self.retryable = value;
        self
    }

    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    InvalidRequest,
    UnauthorizedPath,
    NotFound,
    Conflict,
    PlanExpired,
    PlanStale,
    PlanTampered,
    ToolUnavailable,
    ProviderUnavailable,
    CacheUnavailable,
    JobCanceled,
    JobFailed,
    RateLimited,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub command: String,
    pub resolved_path: String,
    pub available: bool,
    pub version: String,
    /// Why an otherwise-resolved tool is unusable. Without this an unavailable
    /// tool is indistinguishable from a missing one in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRoot {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub name: String,
    pub version: String,
    pub media_root: String,
    pub config_root: String,
    #[serde(default)]
    pub source_roots: Vec<SourceRoot>,
    #[serde(default)]
    pub tools: Vec<ToolStatus>,
    pub contract_version: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemEntryKind {
    Folder,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemEntry {
    pub name: String,
    pub path: String,
    pub kind: FileSystemEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    pub modified_utc: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemResponse {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    #[serde(default)]
    pub entries: Vec<FileSystemEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseFileSystemRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub include_files: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogEntry {
    pub timestamp_utc: DateTime<Utc>,
    pub correlation_id: CorrelationId,
    pub area: String,
    pub level: LogLevel,
    pub message: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Information,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_level: Option<LogLevel>,
    #[serde(default = "default_log_limit")]
    pub limit: usize,
}

const fn default_log_limit() -> usize {
    500
}

/// Hand-written so `LogQuery::default()` agrees with the serde default. A
/// derived `Default` gives `limit: 0`, which silently returns no rows — the
/// serde attribute only applies when deserializing, not to `Default`.
impl Default for LogQuery {
    fn default() -> Self {
        Self {
            area: None,
            minimum_level: None,
            limit: default_log_limit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#[serde(default = "...")]` applies only when deserializing. A derived
    /// `Default` left `limit` at 0, so every caller using `LogQuery::default()`
    /// asked the store for zero rows and the Logs page was always empty.
    #[test]
    fn default_log_query_requests_rows() {
        assert_eq!(LogQuery::default().limit, default_log_limit());
        assert!(LogQuery::default().limit > 0);
    }

    /// The two defaults must agree, or a query built in Rust behaves
    /// differently from the identical query arriving over the wire.
    #[test]
    fn rust_and_wire_defaults_agree() {
        let from_wire: LogQuery = serde_json::from_str("{}").expect("empty query");
        assert_eq!(from_wire, LogQuery::default());
    }
}
