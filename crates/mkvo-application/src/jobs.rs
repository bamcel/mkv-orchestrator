use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use mkvo_contracts::{
    JobAccepted, JobCompletion, JobEvent, JobEventEnvelope, JobKind, JobLogLevel, JobSnapshot,
    JobStatus,
};
use mkvo_domain::{CorrelationId, IdempotencyKey, JobId, PlanId, ResourceAccess, ResourceClaim};
use tokio::sync::{Mutex, Notify, RwLock, broadcast};
use tokio_util::sync::CancellationToken;

use crate::{ApplicationError, ApplicationResult, JobRepository, PortError};

#[derive(Clone, Debug)]
pub struct JobSpec {
    pub kind: JobKind,
    pub idempotency_key: IdempotencyKey,
    pub request_fingerprint: String,
    pub plan_id: Option<PlanId>,
    pub total: u64,
    pub resources: Vec<ResourceClaim>,
}

impl JobSpec {
    pub fn validate(&self) -> ApplicationResult<()> {
        if self.request_fingerprint.trim().is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "job request fingerprint must not be empty".to_owned(),
            ));
        }
        Ok(())
    }
}

pub struct JobSupervisor {
    repository: Arc<dyn JobRepository>,
    registration: Mutex<()>,
    leases: Arc<ResourceLeaseManager>,
    live: RwLock<HashMap<JobId, Arc<LiveJob>>>,
    retained_lines: usize,
    completed_job_retention: Duration,
}

impl JobSupervisor {
    #[must_use]
    pub fn new(repository: Arc<dyn JobRepository>) -> Self {
        Self {
            repository,
            registration: Mutex::new(()),
            leases: Arc::new(ResourceLeaseManager::default()),
            live: RwLock::new(HashMap::new()),
            retained_lines: 500,
            completed_job_retention: Duration::from_secs(300),
        }
    }

    #[must_use]
    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(InMemoryJobRepository::default())))
    }

    #[must_use]
    pub fn with_retained_lines(mut self, retained_lines: usize) -> Self {
        self.retained_lines = retained_lines.max(1);
        self
    }

    /// Keep terminal jobs available for late event subscribers briefly, then
    /// rely on the durable repository for history and idempotent replay.
    #[must_use]
    pub fn with_completed_job_retention(mut self, retention: Duration) -> Self {
        self.completed_job_retention = retention;
        self
    }

    /// Run an application-owned operation while holding the same normalized
    /// read/write resource leases used by supervised background jobs.
    ///
    /// This is intended for short synchronous mutation workflows (for example,
    /// rename apply/undo) that must participate in job-level path exclusion but
    /// do not need their own persisted job record.
    pub async fn with_resource_lease<T, F, Fut>(
        &self,
        resources: &[ResourceClaim],
        cancellation: CancellationToken,
        operation: F,
    ) -> ApplicationResult<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = ApplicationResult<T>>,
    {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::Canceled);
        }
        let lease = self.leases.acquire(resources, cancellation.clone()).await?;
        if cancellation.is_cancelled() {
            drop(lease);
            return Err(ApplicationError::Canceled);
        }
        let result = operation().await;
        drop(lease);
        result
    }

    /// Register and spawn a job. A repeated idempotency key returns the original
    /// job without running `task` a second time.
    pub async fn start<F, Fut>(
        self: &Arc<Self>,
        spec: JobSpec,
        task: F,
    ) -> ApplicationResult<JobAccepted>
    where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = ApplicationResult<JobCompletion>> + Send + 'static,
    {
        spec.validate()?;
        // Registration is intentionally serialized: repository uniqueness and
        // the in-memory idempotency index become visible as one logical step.
        let _registration = self.registration.lock().await;

        if let Some(existing) = self
            .repository
            .find_by_idempotency(&spec.idempotency_key)
            .await?
        {
            if existing.request_fingerprint != spec.request_fingerprint {
                return Err(ApplicationError::Conflict(
                    "The idempotency key is already bound to a different request fingerprint."
                        .to_owned(),
                ));
            }
            return Ok(JobAccepted {
                id: existing.id,
                correlation_id: existing.correlation_id,
                status: existing.status,
                idempotent_replay: true,
            });
        }

        let id = JobId::new();

        let correlation_id = CorrelationId::new();
        let now = Utc::now();
        let snapshot = JobSnapshot {
            id,
            kind: spec.kind,
            status: JobStatus::Queued,
            correlation_id,
            idempotency_key: spec.idempotency_key.clone(),
            request_fingerprint: spec.request_fingerprint.clone(),
            plan_id: spec.plan_id,
            created_utc: now,
            started_utc: None,
            completed_utc: None,
            completed: 0,
            failed: 0,
            skipped: 0,
            total: spec.total,
            current_file: String::new(),
            current_file_percent: 0,
            lines: Vec::new(),
            result: None,
            error: None,
            revision: 0,
        };
        let (events, _) = broadcast::channel(256);
        let live = Arc::new(LiveJob {
            correlation_id,
            snapshot: Mutex::new(snapshot.clone()),
            cancel: CancellationToken::new(),
            events,
            sequence: AtomicU64::new(0),
            repository: Arc::clone(&self.repository),
            retained_lines: self.retained_lines,
        });

        self.repository.insert(&snapshot).await?;
        self.live.write().await.insert(id, Arc::clone(&live));
        live.publish(JobEvent::Snapshot {
            snapshot: snapshot.clone(),
        })
        .await?;

        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            supervisor.run(live, spec.resources, task).await;
        });

        Ok(JobAccepted {
            id,
            correlation_id,
            status: JobStatus::Queued,
            idempotent_replay: false,
        })
    }

    async fn run<F, Fut>(
        self: Arc<Self>,
        live: Arc<LiveJob>,
        resources: Vec<ResourceClaim>,
        task: F,
    ) where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = ApplicationResult<JobCompletion>> + Send + 'static,
    {
        let id = live.snapshot.lock().await.id;
        Arc::clone(&self)
            .run_to_completion(live, resources, task)
            .await;
        tokio::time::sleep(self.completed_job_retention).await;
        self.live.write().await.remove(&id);
    }

    async fn run_to_completion<F, Fut>(
        self: Arc<Self>,
        live: Arc<LiveJob>,
        resources: Vec<ResourceClaim>,
        task: F,
    ) where
        F: FnOnce(JobContext) -> Fut + Send + 'static,
        Fut: Future<Output = ApplicationResult<JobCompletion>> + Send + 'static,
    {
        if !resources.is_empty()
            && live
                .transition(JobStatus::WaitingForResources, None)
                .await
                .is_err()
        {
            return;
        }

        let lease = match self.leases.acquire(&resources, live.cancel.clone()).await {
            Ok(lease) => lease,
            Err(ApplicationError::Canceled) => {
                let _ = live
                    .finish_canceled("Canceled before resources became available")
                    .await;
                return;
            }
            Err(error) => {
                let _ = live.finish_failed(error.to_string()).await;
                return;
            }
        };

        if live.cancel.is_cancelled() {
            let _ = live.finish_canceled("Canceled before execution").await;
            drop(lease);
            return;
        }
        if live.transition(JobStatus::Running, None).await.is_err() {
            drop(lease);
            return;
        }

        let context = JobContext {
            live: Arc::clone(&live),
        };
        let result = task(context).await;
        match result {
            Ok(completion) if live.cancel.is_cancelled() => {
                let _ = live
                    .finish_canceled(
                        completion
                            .message
                            .as_deref()
                            .unwrap_or("Canceled during execution"),
                    )
                    .await;
            }
            Ok(completion) => {
                let _ = live.finish_completed(completion).await;
            }
            Err(ApplicationError::Canceled) => {
                let _ = live.finish_canceled("Canceled").await;
            }
            Err(error) => {
                let _ = live.finish_failed(error.to_string()).await;
            }
        }
        drop(lease);
    }

    pub async fn get(&self, id: JobId) -> ApplicationResult<Option<JobSnapshot>> {
        if let Some(live) = self.live.read().await.get(&id).cloned() {
            return Ok(Some(live.snapshot.lock().await.clone()));
        }
        self.repository.get(id).await.map_err(Into::into)
    }

    pub async fn list_recent(&self, limit: usize) -> ApplicationResult<Vec<JobSnapshot>> {
        self.repository
            .list_recent(limit.clamp(1, 1_000))
            .await
            .map_err(Into::into)
    }

    pub async fn cancel(&self, id: JobId) -> ApplicationResult<JobSnapshot> {
        let live = self
            .live
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| ApplicationError::NotFound(format!("job {id}")))?;
        let snapshot = live.snapshot.lock().await.clone();
        if snapshot.status.is_terminal() {
            return Ok(snapshot);
        }
        live.cancel.cancel();
        live.transition(JobStatus::Canceling, None).await
    }

    pub async fn subscribe(
        &self,
        id: JobId,
    ) -> ApplicationResult<broadcast::Receiver<JobEventEnvelope>> {
        self.live
            .read()
            .await
            .get(&id)
            .map(|job| job.events.subscribe())
            .ok_or_else(|| ApplicationError::NotFound(format!("live job {id}")))
    }
}

struct LiveJob {
    /// Duplicated from the snapshot so a running task can correlate its log
    /// entries without taking the snapshot lock on every write.
    correlation_id: CorrelationId,
    snapshot: Mutex<JobSnapshot>,
    cancel: CancellationToken,
    events: broadcast::Sender<JobEventEnvelope>,
    sequence: AtomicU64,
    repository: Arc<dyn JobRepository>,
    retained_lines: usize,
}

impl LiveJob {
    async fn transition(
        &self,
        status: JobStatus,
        error: Option<String>,
    ) -> ApplicationResult<JobSnapshot> {
        let now = Utc::now();
        let snapshot = self
            .update(|snapshot| {
                if !snapshot.status.can_transition_to(status) {
                    return Err(ApplicationError::Conflict(format!(
                        "invalid job transition {:?} -> {status:?}",
                        snapshot.status
                    )));
                }
                snapshot.status = status;
                if status == JobStatus::Running && snapshot.started_utc.is_none() {
                    snapshot.started_utc = Some(now);
                }
                if status.is_terminal() {
                    snapshot.completed_utc = Some(now);
                    snapshot.current_file.clear();
                    snapshot.current_file_percent = 0;
                }
                if error.is_some() {
                    snapshot.error = error;
                }
                Ok(())
            })
            .await?;
        self.publish(JobEvent::StatusChanged { status }).await?;
        Ok(snapshot)
    }

    async fn update(
        &self,
        mutate: impl FnOnce(&mut JobSnapshot) -> ApplicationResult<()>,
    ) -> ApplicationResult<JobSnapshot> {
        let snapshot = {
            let mut snapshot = self.snapshot.lock().await;
            mutate(&mut snapshot)?;
            snapshot.revision = snapshot.revision.saturating_add(1);
            snapshot.clone()
        };
        self.repository.update(&snapshot).await?;
        Ok(snapshot)
    }

    async fn publish(&self, event: JobEvent) -> ApplicationResult<()> {
        let snapshot = self.snapshot.lock().await;
        let envelope = JobEventEnvelope {
            job_id: snapshot.id,
            correlation_id: snapshot.correlation_id,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            emitted_utc: Utc::now(),
            event,
        };
        drop(snapshot);
        self.repository.append_event(&envelope).await?;
        let _ = self.events.send(envelope);
        Ok(())
    }

    async fn finish_completed(&self, completion: JobCompletion) -> ApplicationResult<JobSnapshot> {
        let result_for_event = completion.result.clone();
        let snapshot = self
            .update(|snapshot| {
                if !snapshot.status.can_transition_to(JobStatus::Completed) {
                    return Err(ApplicationError::Conflict(format!(
                        "cannot complete job in {:?}",
                        snapshot.status
                    )));
                }
                snapshot.status = JobStatus::Completed;
                snapshot.completed_utc = Some(Utc::now());
                snapshot.current_file.clear();
                snapshot.current_file_percent = 0;
                snapshot.result = completion.result;
                if let Some(message) = completion.message {
                    snapshot.lines.push(message);
                }
                Ok(())
            })
            .await?;
        self.publish(JobEvent::Completed {
            summary: snapshot.summary(),
            result: result_for_event,
        })
        .await?;
        Ok(snapshot)
    }

    async fn finish_failed(&self, message: String) -> ApplicationResult<JobSnapshot> {
        let snapshot = self
            .transition(JobStatus::Failed, Some(message.clone()))
            .await?;
        self.publish(JobEvent::Failed { message }).await?;
        Ok(snapshot)
    }

    async fn finish_canceled(&self, message: &str) -> ApplicationResult<JobSnapshot> {
        let snapshot = self.transition(JobStatus::Canceled, None).await?;
        self.publish(JobEvent::Canceled {
            message: message.to_owned(),
        })
        .await?;
        Ok(snapshot)
    }
}

#[derive(Clone)]
pub struct JobContext {
    live: Arc<LiveJob>,
}

impl JobContext {
    /// Correlation identifier for this job, so work it performs can be traced
    /// back to the request that started it across logs, events, and errors.
    #[must_use]
    pub fn correlation_id(&self) -> CorrelationId {
        self.live.correlation_id
    }

    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.live.cancel.clone()
    }

    pub fn ensure_not_canceled(&self) -> ApplicationResult<()> {
        if self.live.cancel.is_cancelled() {
            Err(ApplicationError::Canceled)
        } else {
            Ok(())
        }
    }

    pub async fn progress(
        &self,
        completed: u64,
        total: u64,
        current_file: impl Into<String>,
        current_file_percent: u8,
    ) -> ApplicationResult<()> {
        let current_file = current_file.into();
        self.live
            .update(|snapshot| {
                snapshot.completed = completed.min(total);
                snapshot.total = total;
                snapshot.current_file.clone_from(&current_file);
                snapshot.current_file_percent = current_file_percent.min(100);
                Ok(())
            })
            .await?;
        self.live
            .publish(JobEvent::Progress {
                completed: completed.min(total),
                total,
                current_file,
                current_file_percent: current_file_percent.min(100),
            })
            .await
    }

    pub async fn record_completed(&self) -> ApplicationResult<()> {
        self.live
            .update(|snapshot| {
                snapshot.completed = snapshot.completed.saturating_add(1).min(snapshot.total);
                Ok(())
            })
            .await
            .map(|_| ())
    }

    pub async fn record_failed(&self) -> ApplicationResult<()> {
        self.live
            .update(|snapshot| {
                snapshot.failed = snapshot.failed.saturating_add(1);
                Ok(())
            })
            .await
            .map(|_| ())
    }

    pub async fn record_skipped(&self) -> ApplicationResult<()> {
        self.live
            .update(|snapshot| {
                snapshot.skipped = snapshot.skipped.saturating_add(1);
                Ok(())
            })
            .await
            .map(|_| ())
    }

    pub async fn log(&self, level: JobLogLevel, line: impl Into<String>) -> ApplicationResult<()> {
        let line = line.into();
        let limit = self.live.retained_lines;
        self.live
            .update(|snapshot| {
                snapshot.lines.push(line.clone());
                if snapshot.lines.len() > limit {
                    snapshot.lines.drain(0..snapshot.lines.len() - limit);
                }
                Ok(())
            })
            .await?;
        self.live.publish(JobEvent::Log { level, line }).await
    }

    pub async fn emit(&self, event: JobEvent) -> ApplicationResult<()> {
        self.live.publish(event).await
    }
}

#[derive(Default)]
struct ResourceLeaseManager {
    state: StdMutex<BTreeMap<PathBuf, LeaseState>>,
    changed: Notify,
}

#[derive(Clone, Copy, Debug, Default)]
struct LeaseState {
    readers: usize,
    writer: bool,
}

impl ResourceLeaseManager {
    async fn acquire(
        self: &Arc<Self>,
        claims: &[ResourceClaim],
        cancel: CancellationToken,
    ) -> ApplicationResult<ResourceLease> {
        let claims = merge_claims(claims);
        loop {
            let changed = self.changed.notified();
            {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if claims_available(&state, &claims) {
                    for (path, access) in &claims {
                        let lease = state.entry(path.clone()).or_default();
                        match access {
                            ResourceAccess::Read => lease.readers += 1,
                            ResourceAccess::Write => lease.writer = true,
                        }
                    }
                    return Ok(ResourceLease {
                        manager: Arc::clone(self),
                        claims,
                    });
                }
            }
            tokio::select! {
                () = cancel.cancelled() => return Err(ApplicationError::Canceled),
                () = changed => {}
            }
        }
    }

    fn release(&self, claims: &[(PathBuf, ResourceAccess)]) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (path, access) in claims {
            if let Some(lease) = state.get_mut(path) {
                match access {
                    ResourceAccess::Read => lease.readers = lease.readers.saturating_sub(1),
                    ResourceAccess::Write => lease.writer = false,
                }
                if lease.readers == 0 && !lease.writer {
                    state.remove(path);
                }
            }
        }
        drop(state);
        self.changed.notify_waiters();
    }
}

struct ResourceLease {
    manager: Arc<ResourceLeaseManager>,
    claims: Vec<(PathBuf, ResourceAccess)>,
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        self.manager.release(&self.claims);
    }
}

fn merge_claims(claims: &[ResourceClaim]) -> Vec<(PathBuf, ResourceAccess)> {
    let mut merged = BTreeMap::new();
    for claim in claims {
        let path = normalize_resource_path(&claim.path);
        merged
            .entry(path)
            .and_modify(|access| {
                if claim.access == ResourceAccess::Write {
                    *access = ResourceAccess::Write;
                }
            })
            .or_insert(claim.access);
    }
    merged.into_iter().collect()
}

fn claims_available(
    current: &BTreeMap<PathBuf, LeaseState>,
    requested: &[(PathBuf, ResourceAccess)],
) -> bool {
    requested.iter().all(|(path, access)| {
        current.iter().all(|(held_path, held)| {
            if !paths_overlap(path, held_path) {
                return true;
            }
            match access {
                ResourceAccess::Read => !held.writer,
                ResourceAccess::Write => !held.writer && held.readers == 0,
            }
        })
    })
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn normalize_resource_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    #[cfg(windows)]
    {
        PathBuf::from(normalized.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

#[derive(Default)]
pub struct InMemoryJobRepository {
    jobs: RwLock<HashMap<JobId, JobSnapshot>>,
    keys: RwLock<HashMap<IdempotencyKey, JobId>>,
    events: RwLock<HashMap<JobId, Vec<JobEventEnvelope>>>,
}

#[async_trait]
impl JobRepository for InMemoryJobRepository {
    async fn insert(&self, snapshot: &JobSnapshot) -> Result<(), PortError> {
        let mut keys = self.keys.write().await;
        if let Some(existing) = keys.get(&snapshot.idempotency_key)
            && *existing != snapshot.id
        {
            return Err(PortError::Conflict(format!(
                "idempotency key already belongs to job {existing}"
            )));
        }
        keys.insert(snapshot.idempotency_key.clone(), snapshot.id);
        self.jobs
            .write()
            .await
            .insert(snapshot.id, snapshot.clone());
        Ok(())
    }

    async fn update(&self, snapshot: &JobSnapshot) -> Result<(), PortError> {
        let mut jobs = self.jobs.write().await;
        let current = jobs
            .get(&snapshot.id)
            .ok_or_else(|| PortError::NotFound(format!("job {}", snapshot.id)))?;
        if snapshot.revision < current.revision {
            return Err(PortError::Conflict("stale job revision".to_owned()));
        }
        jobs.insert(snapshot.id, snapshot.clone());
        Ok(())
    }

    async fn get(&self, id: JobId) -> Result<Option<JobSnapshot>, PortError> {
        Ok(self.jobs.read().await.get(&id).cloned())
    }

    async fn find_by_idempotency(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<JobSnapshot>, PortError> {
        let id = self.keys.read().await.get(key).copied();
        Ok(match id {
            Some(id) => self.jobs.read().await.get(&id).cloned(),
            None => None,
        })
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<JobSnapshot>, PortError> {
        let mut jobs: Vec<_> = self.jobs.read().await.values().cloned().collect();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.created_utc));
        jobs.truncate(limit);
        Ok(jobs)
    }

    async fn append_event(&self, event: &JobEventEnvelope) -> Result<(), PortError> {
        self.events
            .write()
            .await
            .entry(event.job_id)
            .or_default()
            .push(event.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    fn spec(key: &str, resources: Vec<ResourceClaim>) -> JobSpec {
        JobSpec {
            kind: JobKind::Scan,
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            request_fingerprint: "request-hash".to_owned(),
            plan_id: None,
            total: 1,
            resources,
        }
    }

    #[tokio::test]
    async fn repeated_key_runs_task_once() {
        let supervisor = JobSupervisor::in_memory();
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        let first = supervisor
            .start(spec("same-key", Vec::new()), move |_| async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(JobCompletion::default())
            })
            .await
            .unwrap();
        let second = supervisor
            .start(spec("same-key", Vec::new()), |_| async move {
                panic!("duplicate task must not run");
            })
            .await
            .unwrap();
        assert_eq!(first.id, second.id);
        assert!(second.idempotent_replay);
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn repeated_key_with_different_fingerprint_is_rejected() {
        let supervisor = JobSupervisor::in_memory();
        supervisor
            .start(spec("conflicting-key", Vec::new()), |_| async move {
                Ok(JobCompletion::default())
            })
            .await
            .unwrap();

        let mut conflicting = spec("conflicting-key", Vec::new());
        conflicting.request_fingerprint = "different-request-hash".to_owned();
        let error = supervisor
            .start(conflicting, |_| async move {
                panic!("conflicting task must not run");
            })
            .await
            .unwrap_err();

        assert!(matches!(error, ApplicationError::Conflict(_)));
        assert_eq!(
            error.to_string(),
            "conflict: The idempotency key is already bound to a different request fingerprint."
        );
    }

    #[tokio::test]
    async fn completed_jobs_leave_live_memory_but_remain_durable() {
        let supervisor = Arc::new(
            JobSupervisor::new(Arc::new(InMemoryJobRepository::default()))
                .with_completed_job_retention(Duration::from_millis(5)),
        );
        let accepted = supervisor
            .start(spec("evicted-key", Vec::new()), |_| async move {
                Ok(JobCompletion::default())
            })
            .await
            .unwrap();

        for _ in 0..20 {
            if !supervisor.live.read().await.contains_key(&accepted.id) {
                let snapshot = supervisor.get(accepted.id).await.unwrap().unwrap();
                assert_eq!(snapshot.status, JobStatus::Completed);
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("terminal job was not evicted from live memory");
    }

    #[tokio::test]
    async fn cancellation_reaches_running_task() {
        let supervisor = JobSupervisor::in_memory();
        let accepted = supervisor
            .start(spec("cancel-key", Vec::new()), |context| async move {
                context.cancellation_token().cancelled().await;
                Err(ApplicationError::Canceled)
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        supervisor.cancel(accepted.id).await.unwrap();
        for _ in 0..20 {
            let snapshot = supervisor.get(accepted.id).await.unwrap().unwrap();
            if snapshot.status == JobStatus::Canceled {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("job did not reach canceled state");
    }

    #[tokio::test]
    async fn writer_waits_for_overlapping_reader() {
        let manager = Arc::new(ResourceLeaseManager::default());
        let reader = manager
            .acquire(
                &[ResourceClaim::read("library/show")],
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let writer_manager = Arc::clone(&manager);
        let waiting = tokio::spawn(async move {
            writer_manager
                .acquire(
                    &[ResourceClaim::write("library/show/episode.mkv")],
                    CancellationToken::new(),
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!waiting.is_finished());
        drop(reader);
        assert!(waiting.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn synchronous_operation_uses_supervisor_resource_leases() {
        let supervisor = JobSupervisor::in_memory();
        let reader = supervisor
            .leases
            .acquire(
                &[ResourceClaim::read("library/show")],
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let ran = Arc::new(AtomicUsize::new(0));
        let ran_in_operation = Arc::clone(&ran);
        let waiting_supervisor = Arc::clone(&supervisor);
        let waiting = tokio::spawn(async move {
            waiting_supervisor
                .with_resource_lease(
                    &[ResourceClaim::write("library/show/episode.mkv")],
                    CancellationToken::new(),
                    || async move {
                        ran_in_operation.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        drop(reader);
        waiting.await.unwrap().unwrap();
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn synchronous_operation_honors_cancellation_while_waiting() {
        let supervisor = JobSupervisor::in_memory();
        let writer = supervisor
            .leases
            .acquire(
                &[ResourceClaim::write("library/show")],
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let cancellation = CancellationToken::new();
        let waiting_cancellation = cancellation.clone();
        let waiting_supervisor = Arc::clone(&supervisor);
        let waiting = tokio::spawn(async move {
            waiting_supervisor
                .with_resource_lease(
                    &[ResourceClaim::write("library/show/episode.mkv")],
                    waiting_cancellation,
                    || async {
                        Err::<(), _>(ApplicationError::Internal(
                            "canceled operation unexpectedly ran".to_owned(),
                        ))
                    },
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        cancellation.cancel();
        assert!(matches!(
            waiting.await.unwrap(),
            Err(ApplicationError::Canceled)
        ));
        drop(writer);
    }
}
