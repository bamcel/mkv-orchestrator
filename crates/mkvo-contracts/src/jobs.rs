use chrono::{DateTime, Utc};
use mkvo_domain::{CorrelationId, IdempotencyKey, JobId, OperationKind, PlanId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::MediaFileDto;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Scan,
    Rename,
    RenameUndo,
    Remux,
    ConvertToMkv,
    MuxSubtitles,
    ExtractSubtitles,
    PropertyEdit,
    LibraryAudit,
    CacheReconcile,
    MediaServerSync,
}

impl From<OperationKind> for JobKind {
    fn from(value: OperationKind) -> Self {
        match value {
            OperationKind::Scan => Self::Scan,
            OperationKind::Rename => Self::Rename,
            OperationKind::RenameUndo => Self::RenameUndo,
            OperationKind::Remux => Self::Remux,
            OperationKind::ConvertToMkv => Self::ConvertToMkv,
            OperationKind::MuxSubtitles => Self::MuxSubtitles,
            OperationKind::ExtractSubtitles => Self::ExtractSubtitles,
            OperationKind::PropertyEdit => Self::PropertyEdit,
            OperationKind::LibraryAudit => Self::LibraryAudit,
            OperationKind::CacheReconcile => Self::CacheReconcile,
            OperationKind::MediaServerSync => Self::MediaServerSync,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum JobStatus {
    Queued,
    WaitingForResources,
    Running,
    Canceling,
    Completed,
    Failed,
    Skipped,
    Canceled,
}

impl JobStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Skipped | Self::Canceled
        )
    }

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        match (self, next) {
            (current, target) if current as u8 == target as u8 => true,
            (
                Self::Queued,
                Self::WaitingForResources
                | Self::Running
                | Self::Canceling
                | Self::Canceled
                | Self::Failed,
            )
            | (
                Self::WaitingForResources,
                Self::Running | Self::Canceling | Self::Canceled | Self::Failed,
            )
            | (
                Self::Running,
                Self::Canceling | Self::Completed | Self::Failed | Self::Skipped | Self::Canceled,
            )
            | (Self::Canceling, Self::Canceled | Self::Failed)
            | (Self::Queued | Self::WaitingForResources, Self::Skipped) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAccepted {
    pub id: JobId,
    pub correlation_id: CorrelationId,
    pub status: JobStatus,
    pub idempotent_replay: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: JobId,
    pub kind: JobKind,
    pub status: JobStatus,
    pub correlation_id: CorrelationId,
    pub idempotency_key: IdempotencyKey,
    pub request_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<PlanId>,
    pub created_utc: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_utc: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_utc: Option<DateTime<Utc>>,
    pub completed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub total: u64,
    pub current_file: String,
    pub current_file_percent: u8,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub revision: u64,
}

impl JobSnapshot {
    #[must_use]
    pub fn summary(&self) -> JobSummary {
        JobSummary {
            total: self.total,
            completed: self.completed,
            failed: self.failed,
            skipped: self.skipped,
            canceled: u64::from(self.status == JobStatus::Canceled),
            pending: self.total.saturating_sub(
                self.completed
                    + self.failed
                    + self.skipped
                    + u64::from(self.status == JobStatus::Canceled),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub total: u64,
    pub completed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub canceled: u64,
    pub pending: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub workflow: JobKind,
    pub started_utc: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_utc: Option<DateTime<Utc>>,
    pub counts: JobSummary,
    #[serde(default)]
    pub jobs: Vec<JobSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEventEnvelope {
    pub job_id: JobId,
    pub correlation_id: CorrelationId,
    pub sequence: u64,
    pub emitted_utc: DateTime<Utc>,
    pub event: JobEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    Snapshot {
        snapshot: JobSnapshot,
    },
    StatusChanged {
        status: JobStatus,
    },
    Progress {
        completed: u64,
        total: u64,
        current_file: String,
        current_file_percent: u8,
    },
    MediaDiscovered {
        file: MediaFileDto,
    },
    Log {
        level: JobLogLevel,
        line: String,
    },
    Completed {
        summary: JobSummary,
        result: Option<Value>,
    },
    Failed {
        message: String,
    },
    Canceled {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobLogLevel {
    Debug,
    Information,
    Warning,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCompletion {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListJobsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<JobKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<JobStatus>,
    #[serde(default = "default_job_limit")]
    pub limit: usize,
}

const fn default_job_limit() -> usize {
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_cannot_restart() {
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Running));
        assert!(JobStatus::Queued.can_transition_to(JobStatus::Running));
        assert!(JobStatus::Running.can_transition_to(JobStatus::Canceling));
    }
}
