use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mkvo_contracts::{JobEventEnvelope, JobSnapshot, LogQuery, OperationLogEntry, ToolStatus};
use mkvo_domain::{
    AppSettings, EpisodeMetadata, FileFingerprint, IdempotencyKey, MediaFile, MediaServerKind,
    MediaServerLibrary, MetadataProvider, PlanId, ProviderSearchResult, RenameBatchId,
    RenameBatchRecord, ResourceClaim, StoredPlan,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::PortError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaEnumerationRequest {
    pub roots: Vec<PathBuf>,
    pub ignored_folder_names: BTreeSet<String>,
    pub supported_extensions: BTreeSet<String>,
}

#[async_trait]
pub trait MediaCatalog: Send + Sync {
    async fn enumerate(
        &self,
        request: &MediaEnumerationRequest,
        cancel: CancellationToken,
    ) -> Result<Vec<FileFingerprint>, PortError>;
}

#[async_trait]
pub trait MediaProbe: Send + Sync {
    async fn inspect(&self, path: &Path, cancel: CancellationToken)
    -> Result<MediaFile, PortError>;
}

#[async_trait]
pub trait MetadataCache: Send + Sync {
    async fn get_valid(
        &self,
        fingerprint: &FileFingerprint,
    ) -> Result<Option<MediaFile>, PortError>;
    async fn upsert(&self, file: &MediaFile) -> Result<(), PortError>;
    async fn remove(&self, path: &Path) -> Result<bool, PortError>;
    async fn remove_under(&self, root: &Path) -> Result<u64, PortError>;
    async fn count(&self) -> Result<u64, PortError>;
    async fn list_under(&self, root: &Path) -> Result<Vec<MediaFile>, PortError>;
}

#[async_trait]
pub trait AuthorizedPathPolicy: Send + Sync {
    async fn authorize_read(&self, path: &Path) -> Result<PathBuf, PortError>;
    async fn authorize_write(&self, path: &Path) -> Result<PathBuf, PortError>;
}

#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn exists(&self, path: &Path) -> Result<bool, PortError>;
    async fn is_directory(&self, path: &Path) -> Result<bool, PortError>;
    async fn fingerprint(&self, path: &Path) -> Result<FileFingerprint, PortError>;
    async fn move_file(&self, source: &Path, target: &Path) -> Result<(), PortError>;
    async fn remove_file(&self, path: &Path) -> Result<(), PortError>;

    /// Report whether a file can actually be opened for the access a mutation
    /// needs.
    ///
    /// Existence is not enough: a media file being read by a media server, or
    /// one marked read-only, passes every other precondition and then fails
    /// partway through an external tool run. Checking during preview turns that
    /// into a blocked row with a reason the user can act on.
    async fn probe_access(
        &self,
        path: &Path,
        access: RequiredAccess,
    ) -> Result<FileAccessState, PortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredAccess {
    Read,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAccessState {
    /// Open succeeded with the requested access.
    Available,
    /// The path does not exist.
    Missing,
    /// Permissions deny the requested access.
    ReadOnly,
    /// Another process holds the file.
    Busy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    pub tool: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<PathBuf>,
    #[serde(default)]
    pub expected_outputs: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub validated_outputs: Vec<PathBuf>,
}

#[async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn status(&self, logical_name: &str) -> Result<ToolStatus, PortError>;
    async fn all_statuses(&self) -> Result<Vec<ToolStatus>, PortError>;
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        invocation: &ToolInvocation,
        cancel: CancellationToken,
    ) -> Result<ToolExecutionResult, PortError>;
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn load(&self) -> Result<(AppSettings, u64), PortError>;
    async fn save(
        &self,
        settings: &AppSettings,
        expected_revision: Option<u64>,
    ) -> Result<u64, PortError>;
}

#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<String>, PortError>;
    async fn set(&self, key: &str, value: &str) -> Result<(), PortError>;
    async fn remove(&self, key: &str) -> Result<(), PortError>;
}

#[async_trait]
pub trait PlanRepository: Send + Sync {
    async fn save(&self, plan: &StoredPlan) -> Result<(), PortError>;
    async fn get(&self, id: PlanId) -> Result<Option<StoredPlan>, PortError>;
    async fn remove_expired(&self, before: DateTime<Utc>) -> Result<u64, PortError>;
}

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn insert(&self, snapshot: &JobSnapshot) -> Result<(), PortError>;
    async fn update(&self, snapshot: &JobSnapshot) -> Result<(), PortError>;
    async fn get(&self, id: mkvo_domain::JobId) -> Result<Option<JobSnapshot>, PortError>;
    async fn find_by_idempotency(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<JobSnapshot>, PortError>;
    async fn list_recent(&self, limit: usize) -> Result<Vec<JobSnapshot>, PortError>;
    async fn append_event(&self, event: &JobEventEnvelope) -> Result<(), PortError>;
}

#[async_trait]
pub trait RenameHistoryRepository: Send + Sync {
    async fn add(&self, record: &RenameBatchRecord) -> Result<(), PortError>;
    async fn get(&self, id: RenameBatchId) -> Result<Option<RenameBatchRecord>, PortError>;
    async fn list_recent(&self, limit: usize) -> Result<Vec<RenameBatchRecord>, PortError>;
    async fn mark_undone(&self, id: RenameBatchId, at: DateTime<Utc>) -> Result<(), PortError>;
    async fn clear(&self) -> Result<u64, PortError>;
}

#[async_trait]
pub trait MetadataProviderClient: Send + Sync {
    fn provider(&self) -> MetadataProvider;
    async fn search(
        &self,
        query: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<ProviderSearchResult>, PortError>;
    async fn episodes(
        &self,
        media_id: &str,
        language: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<EpisodeMetadata>, PortError>;
    async fn test(&self, cancel: CancellationToken) -> Result<(), PortError>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaServerConnection {
    pub kind: MediaServerKind,
    pub base_url: String,
    pub credential: String,
}

#[async_trait]
pub trait MediaServerClient: Send + Sync {
    fn kind(&self) -> MediaServerKind;
    async fn test(
        &self,
        connection: &MediaServerConnection,
        cancel: CancellationToken,
    ) -> Result<(), PortError>;
    async fn discover_libraries(
        &self,
        connection: &MediaServerConnection,
        cancel: CancellationToken,
    ) -> Result<Vec<MediaServerLibrary>, PortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchBackendKind {
    Native,
    Polling,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchHealth {
    pub running: bool,
    pub backend: WatchBackendKind,
    pub watched_roots: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_utc: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchChangeKind {
    Created,
    Modified,
    Removed,
    Renamed,
    RescanRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchChange {
    pub kind: WatchChangeKind,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}

#[async_trait]
pub trait WatchBackend: Send + Sync {
    /// Subscribe to transport-neutral filesystem changes. Lagged receivers must
    /// trigger reconciliation because individual changes may have been lost.
    fn subscribe(&self) -> broadcast::Receiver<WatchChange>;
    async fn start(&self, roots: &[PathBuf], force_polling: bool) -> Result<(), PortError>;
    async fn stop(&self) -> Result<(), PortError>;
    async fn health(&self) -> Result<WatchHealth, PortError>;
}

#[async_trait]
pub trait OperationLog: Send + Sync {
    async fn append(&self, entry: &OperationLogEntry) -> Result<(), PortError>;
    async fn query(&self, query: &LogQuery) -> Result<Vec<OperationLogEntry>, PortError>;
    async fn clear(&self) -> Result<u64, PortError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Prepared,
    Running,
    Completed,
    Failed,
    RolledBack,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalRecord {
    pub idempotency_key: IdempotencyKey,
    pub plan_id: PlanId,
    pub step: u64,
    pub status: JournalStatus,
    #[serde(default)]
    pub resources: Vec<ResourceClaim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub updated_utc: DateTime<Utc>,
}

#[async_trait]
pub trait OperationJournal: Send + Sync {
    async fn begin(&self, record: &JournalRecord) -> Result<(), PortError>;
    async fn advance(&self, record: &JournalRecord) -> Result<(), PortError>;
    async fn get(&self, key: &IdempotencyKey) -> Result<Option<JournalRecord>, PortError>;
}
