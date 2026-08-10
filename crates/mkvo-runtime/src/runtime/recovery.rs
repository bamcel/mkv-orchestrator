use mkvo_application::{JournalRecord, JournalStatus};
use mkvo_contracts::{JobKind, JobSnapshot, JobStatus};

use super::RecoveryDisposition;

pub(super) fn classify_recovery(
    snapshot: &JobSnapshot,
    journal: Option<&JournalRecord>,
) -> (RecoveryDisposition, String) {
    if let Some(journal) = journal {
        return match journal.status {
            JournalStatus::Completed => (
                RecoveryDisposition::Completed,
                format!(
                    "the mutation journal completed at step {}; the stale job record was reconciled",
                    journal.step
                ),
            ),
            JournalStatus::RolledBack => (
                RecoveryDisposition::CleanRetry,
                format!(
                    "the interrupted operation was rolled back at step {}; it is safe to build a new plan and retry",
                    journal.step
                ),
            ),
            JournalStatus::Prepared if journal.step == 0 => (
                RecoveryDisposition::CleanRetry,
                "the journal was prepared but no mutation step completed; build a new plan and retry"
                    .to_owned(),
            ),
            JournalStatus::Prepared | JournalStatus::Running | JournalStatus::Failed => (
                RecoveryDisposition::ManualReview,
                format!(
                    "the mutation journal stopped in {:?} at step {}; inspect its resources before retrying",
                    journal.status, journal.step
                ),
            ),
        };
    }

    if matches!(
        snapshot.status,
        JobStatus::Queued | JobStatus::WaitingForResources
    ) || matches!(
        snapshot.kind,
        JobKind::Scan | JobKind::LibraryAudit | JobKind::CacheReconcile
    ) {
        (
            RecoveryDisposition::CleanRetry,
            "no mutation journal exists and the job had not acquired mutation resources; retry with a new idempotency key"
                .to_owned(),
        )
    } else {
        (
            RecoveryDisposition::ManualReview,
            "a running mutating job has no durable journal; inspect source, staged, backup, and target paths before retrying"
                .to_owned(),
        )
    }
}
