use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mkvo_application::{
    JobRepository, JournalRecord, MetadataCache, OperationJournal, OperationLog, PlanRepository,
    PortError, RenameHistoryRepository, SettingsRepository,
};
use mkvo_contracts::{JobEventEnvelope, JobSnapshot, LogLevel, LogQuery, OperationLogEntry};
use mkvo_domain::{
    AppSettings, FileFingerprint, IdempotencyKey, JobId, MediaFile, PlanId, RenameBatchId,
    RenameBatchRecord, StoredPlan as DomainStoredPlan,
};
use rusqlite::{OptionalExtension, params};

use crate::{
    CacheFingerprint, CachedMedia, RenameBatch, RenameBatchEntry, SqliteStore, StoreError,
    StoredJob, StoredPlan,
};

const SETTINGS_KEY: &str = "app";

/// Composition-friendly bundle. A single WAL-configured store can be shared by
/// desktop and server hosts and registered for each application repository port.
#[derive(Debug, Clone)]
pub struct SqliteRepositories {
    store: Arc<SqliteStore>,
}

impl SqliteRepositories {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            store: Arc::new(SqliteStore::open(path)?),
        })
    }

    pub fn from_store(store: SqliteStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub fn shared_store(&self) -> Arc<SqliteStore> {
        Arc::clone(&self.store)
    }

    async fn blocking<T, F>(&self, operation: F) -> Result<T, PortError>
    where
        T: Send + 'static,
        F: FnOnce(SqliteStore) -> Result<T, StoreError> + Send + 'static,
    {
        let store = (*self.store).clone();
        tokio::task::spawn_blocking(move || operation(store))
            .await
            .map_err(|error| PortError::Other(format!("SQLite task failed: {error}")))?
            .map_err(store_port_error)
    }
}

#[async_trait]
impl MetadataCache for SqliteRepositories {
    async fn get_valid(
        &self,
        fingerprint: &FileFingerprint,
    ) -> Result<Option<MediaFile>, PortError> {
        let fingerprint = to_cache_fingerprint(fingerprint);
        self.blocking(move |store| {
            store
                .get_valid_media(&fingerprint)?
                .map(|cached| serde_json::from_value(cached.payload).map_err(StoreError::from))
                .transpose()
        })
        .await
    }

    async fn upsert(&self, file: &MediaFile) -> Result<(), PortError> {
        let file = file.clone();
        self.blocking(move |store| {
            store.upsert_media(&CachedMedia {
                fingerprint: to_cache_fingerprint(&file.fingerprint),
                scanned_at_ms: Utc::now().timestamp_millis(),
                payload: serde_json::to_value(file)?,
            })
        })
        .await
    }

    async fn remove(&self, path: &Path) -> Result<bool, PortError> {
        let path = path.to_path_buf();
        self.blocking(move |store| store.remove_media(&path)).await
    }

    async fn remove_under(&self, root: &Path) -> Result<u64, PortError> {
        let root = root.to_path_buf();
        self.blocking(move |store| store.remove_media_under(&root))
            .await
    }

    async fn count(&self) -> Result<u64, PortError> {
        self.blocking(|store| store.cache_count()).await
    }

    async fn list_under(&self, root: &Path) -> Result<Vec<MediaFile>, PortError> {
        let root = root.to_path_buf();
        self.blocking(move |store| {
            store
                .list_media_under(&root)?
                .into_iter()
                .map(|cached| serde_json::from_value(cached.payload).map_err(StoreError::from))
                .collect()
        })
        .await
    }
}

#[async_trait]
impl SettingsRepository for SqliteRepositories {
    async fn load(&self) -> Result<(AppSettings, u64), PortError> {
        self.blocking(|store| {
            Ok(store
                .get_setting_with_revision::<AppSettings>(SETTINGS_KEY)?
                .map_or_else(
                    || (AppSettings::default(), 0),
                    |(_, revision, value)| (value, revision),
                ))
        })
        .await
    }

    async fn save(
        &self,
        settings: &AppSettings,
        expected_revision: Option<u64>,
    ) -> Result<u64, PortError> {
        let settings = settings.clone().normalized();
        self.blocking(move |store| {
            store.save_setting_optimistic(
                SETTINGS_KEY,
                settings.schema_version,
                &settings,
                expected_revision,
            )
        })
        .await
    }
}

#[async_trait]
impl PlanRepository for SqliteRepositories {
    async fn save(&self, plan: &DomainStoredPlan) -> Result<(), PortError> {
        let plan = plan.clone();
        self.blocking(move |store| {
            store.put_plan(&StoredPlan {
                id: plan.metadata.id.to_string(),
                kind: serde_json::to_string(&plan.metadata.kind)?,
                version: plan.metadata.contract_version,
                fingerprint: plan.metadata.fingerprint.clone(),
                payload: serde_json::to_value(&plan)?,
                created_at_ms: plan.metadata.created_at.timestamp_millis(),
                expires_at_ms: Some(plan.metadata.expires_at.timestamp_millis()),
            })
        })
        .await
    }

    async fn get(&self, id: PlanId) -> Result<Option<DomainStoredPlan>, PortError> {
        self.blocking(move |store| {
            store
                .get_plan(&id.to_string(), Utc::now().timestamp_millis())?
                .map(|plan| serde_json::from_value(plan.payload).map_err(StoreError::from))
                .transpose()
        })
        .await
    }

    async fn remove_expired(&self, before: DateTime<Utc>) -> Result<u64, PortError> {
        self.blocking(move |store| {
            let connection = store.connection()?;
            let removed = connection.execute(
                "DELETE FROM plans WHERE expires_at_ms IS NOT NULL AND expires_at_ms <= ?1",
                [before.timestamp_millis()],
            )?;
            u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
        })
        .await
    }
}

#[async_trait]
impl JobRepository for SqliteRepositories {
    async fn insert(&self, snapshot: &JobSnapshot) -> Result<(), PortError> {
        save_job(self, snapshot).await
    }

    async fn update(&self, snapshot: &JobSnapshot) -> Result<(), PortError> {
        save_job(self, snapshot).await
    }

    async fn get(&self, id: JobId) -> Result<Option<JobSnapshot>, PortError> {
        self.blocking(move |store| decode_job(store.get_job(&id.to_string())?))
            .await
    }

    async fn find_by_idempotency(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<JobSnapshot>, PortError> {
        let key = key.to_string();
        self.blocking(move |store| decode_job(store.find_job_by_idempotency_key(&key)?))
            .await
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<JobSnapshot>, PortError> {
        self.blocking(move |store| {
            store
                .list_recent_jobs(limit)?
                .into_iter()
                .map(|job| decode_job(Some(job)).map(Option::unwrap))
                .collect()
        })
        .await
    }

    async fn append_event(&self, event: &JobEventEnvelope) -> Result<(), PortError> {
        let event = event.clone();
        self.blocking(move |store| {
            let payload = serde_json::to_string(&event)?;
            let sequence =
                i64::try_from(event.sequence).map_err(|_| StoreError::NumericOverflow)?;
            let connection = store.connection()?;
            connection.execute(
                "INSERT INTO job_events (job_id, sequence, payload_json, emitted_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(job_id, sequence) DO UPDATE SET payload_json=excluded.payload_json,
                 emitted_at_ms=excluded.emitted_at_ms",
                params![
                    event.job_id.to_string(),
                    sequence,
                    payload,
                    event.emitted_utc.timestamp_millis(),
                ],
            )?;
            Ok(())
        })
        .await
    }
}

async fn save_job(
    repositories: &SqliteRepositories,
    snapshot: &JobSnapshot,
) -> Result<(), PortError> {
    let snapshot = snapshot.clone();
    repositories
        .blocking(move |store| {
            store.upsert_job(&StoredJob {
                id: snapshot.id.to_string(),
                kind: serde_json::to_string(&snapshot.kind)?,
                state: serde_json::to_string(&snapshot.status)?,
                request: serde_json::Value::Null,
                result: Some(serde_json::to_value(&snapshot)?),
                idempotency_key: Some(snapshot.idempotency_key.to_string()),
                error: snapshot
                    .error
                    .as_ref()
                    .map(|message| serde_json::Value::String(message.clone())),
                created_at_ms: snapshot.created_utc.timestamp_millis(),
                updated_at_ms: Utc::now().timestamp_millis(),
            })
        })
        .await
}

fn decode_job(job: Option<StoredJob>) -> Result<Option<JobSnapshot>, StoreError> {
    job.map(|job| {
        let payload = job.result.ok_or_else(|| {
            StoreError::Json(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "job snapshot payload is missing",
            )))
        })?;
        serde_json::from_value(payload).map_err(StoreError::from)
    })
    .transpose()
}

#[async_trait]
impl RenameHistoryRepository for SqliteRepositories {
    async fn add(&self, record: &RenameBatchRecord) -> Result<(), PortError> {
        let record = record.clone();
        self.blocking(move |store| {
            let local = RenameBatch {
                id: record.id.to_string(),
                created_at_ms: record.created_at.timestamp_millis(),
                undone_at_ms: record.undone_at.map(|value| value.timestamp_millis()),
                provider: record
                    .provider
                    .map(|provider| serde_json::to_string(&provider))
                    .transpose()?
                    .unwrap_or_default(),
                template: record.template.clone(),
                entries: record
                    .entries
                    .iter()
                    .map(|entry| RenameBatchEntry {
                        original_path: entry.original_path.clone(),
                        renamed_path: entry.renamed_path.clone(),
                    })
                    .collect(),
            };
            store.record_rename_batch(&local)?;
            let payload = serde_json::to_string(&record)?;
            let connection = store.connection()?;
            connection.execute(
                "UPDATE rename_batches SET payload_json = ?2 WHERE id = ?1",
                params![record.id.to_string(), payload],
            )?;
            Ok(())
        })
        .await
    }

    async fn get(&self, id: RenameBatchId) -> Result<Option<RenameBatchRecord>, PortError> {
        self.blocking(move |store| load_rename_payload(&store, &id.to_string()))
            .await
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<RenameBatchRecord>, PortError> {
        self.blocking(move |store| {
            let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow)?;
            let connection = store.connection()?;
            let mut statement = connection.prepare(
                "SELECT payload_json FROM rename_batches WHERE payload_json IS NOT NULL
                 ORDER BY created_at_ms DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit], |row| row.get::<_, String>(0))?;
            rows.map(|row| {
                let payload = row?;
                serde_json::from_str(&payload).map_err(StoreError::from)
            })
            .collect()
        })
        .await
    }

    async fn mark_undone(&self, id: RenameBatchId, at: DateTime<Utc>) -> Result<(), PortError> {
        let mut record = RenameHistoryRepository::get(self, id)
            .await?
            .ok_or_else(|| PortError::NotFound(format!("rename batch {id}")))?;
        record.undone_at = Some(at);
        RenameHistoryRepository::add(self, &record).await
    }

    async fn clear(&self) -> Result<u64, PortError> {
        self.blocking(|store| store.clear_rename_batches()).await
    }
}

fn load_rename_payload(
    store: &SqliteStore,
    id: &str,
) -> Result<Option<RenameBatchRecord>, StoreError> {
    let connection = store.connection()?;
    let payload = connection
        .query_row(
            "SELECT payload_json FROM rename_batches WHERE id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    payload
        .map(|payload| serde_json::from_str(&payload).map_err(StoreError::from))
        .transpose()
}

#[async_trait]
impl OperationJournal for SqliteRepositories {
    async fn begin(&self, record: &JournalRecord) -> Result<(), PortError> {
        save_journal(self, record).await
    }

    async fn advance(&self, record: &JournalRecord) -> Result<(), PortError> {
        save_journal(self, record).await
    }

    async fn get(&self, key: &IdempotencyKey) -> Result<Option<JournalRecord>, PortError> {
        let key = key.to_string();
        self.blocking(move |store| {
            let connection = store.connection()?;
            let payload = connection
                .query_row(
                    "SELECT payload_json FROM mutation_journal WHERE idempotency_key = ?1",
                    [key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(StoreError::from))
                .transpose()
        })
        .await
    }

    async fn list_incomplete(&self) -> Result<Option<Vec<JournalRecord>>, PortError> {
        self.blocking(|store| {
            let connection = store.connection()?;
            let mut statement = connection.prepare(
                "SELECT payload_json FROM mutation_journal
                 WHERE status IN ('\"prepared\"', '\"running\"')
                 ORDER BY updated_at_ms",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.map(|row| {
                let payload = row?;
                serde_json::from_str(&payload).map_err(StoreError::from)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
        })
        .await
    }
}

async fn save_journal(
    repositories: &SqliteRepositories,
    record: &JournalRecord,
) -> Result<(), PortError> {
    let record = record.clone();
    repositories
        .blocking(move |store| {
            let payload = serde_json::to_string(&record)?;
            let step = i64::try_from(record.step).map_err(|_| StoreError::NumericOverflow)?;
            let connection = store.connection()?;
            connection.execute(
                "INSERT INTO mutation_journal
                 (idempotency_key, plan_id, step, status, payload_json, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(idempotency_key) DO UPDATE SET plan_id=excluded.plan_id,
                 step=excluded.step, status=excluded.status, payload_json=excluded.payload_json,
                 updated_at_ms=excluded.updated_at_ms",
                params![
                    record.idempotency_key.to_string(),
                    record.plan_id.to_string(),
                    step,
                    serde_json::to_string(&record.status)?,
                    payload,
                    record.updated_utc.timestamp_millis(),
                ],
            )?;
            Ok(())
        })
        .await
}

#[async_trait]
impl OperationLog for SqliteRepositories {
    async fn append(&self, entry: &OperationLogEntry) -> Result<(), PortError> {
        let entry = entry.clone();
        self.blocking(move |store| {
            let connection = store.connection()?;
            connection.execute(
                "INSERT INTO operation_logs
                 (timestamp_ms, correlation_id, area, level, message, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    entry.timestamp_utc.timestamp_millis(),
                    entry.correlation_id.to_string(),
                    entry.area,
                    serde_json::to_string(&entry.level)?,
                    entry.message,
                    entry.detail,
                ],
            )?;
            Ok(())
        })
        .await
    }

    async fn query(&self, query: &LogQuery) -> Result<Vec<OperationLogEntry>, PortError> {
        let query = query.clone();
        self.blocking(move |store| {
            let connection = store.connection()?;
            let limit = i64::try_from(query.limit).map_err(|_| StoreError::NumericOverflow)?;
            let mut statement = connection.prepare(
                "SELECT timestamp_ms, correlation_id, area, level, message, detail
                 FROM operation_logs
                 WHERE (?1 IS NULL OR area = ?1)
                 ORDER BY timestamp_ms DESC LIMIT ?2",
            )?;
            let rows = statement.query_map(params![query.area, limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;
            let mut entries = Vec::new();
            for row in rows {
                let (timestamp, correlation, area, level, message, detail) = row?;
                let level: LogLevel = serde_json::from_str(&level)?;
                if query
                    .minimum_level
                    .is_some_and(|minimum| log_rank(level) < log_rank(minimum))
                {
                    continue;
                }
                entries.push(OperationLogEntry {
                    timestamp_utc: DateTime::from_timestamp_millis(timestamp).ok_or_else(|| {
                        StoreError::Json(serde_json::Error::io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "invalid log timestamp",
                        )))
                    })?,
                    correlation_id: correlation.parse().map_err(|error| {
                        StoreError::Json(serde_json::Error::io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            error,
                        )))
                    })?,
                    area,
                    level,
                    message,
                    detail,
                });
            }
            Ok(entries)
        })
        .await
    }

    async fn clear(&self) -> Result<u64, PortError> {
        self.blocking(|store| {
            let connection = store.connection()?;
            let removed = connection.execute("DELETE FROM operation_logs", [])?;
            u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
        })
        .await
    }
}

const fn log_rank(level: LogLevel) -> u8 {
    match level {
        LogLevel::Trace => 0,
        LogLevel::Debug => 1,
        LogLevel::Information => 2,
        LogLevel::Warning => 3,
        LogLevel::Error => 4,
    }
}

fn to_cache_fingerprint(value: &FileFingerprint) -> CacheFingerprint {
    CacheFingerprint {
        path: value.path.clone(),
        file_size: value.size_bytes,
        modified_at_ns: value.modified_at.timestamp_nanos_opt().unwrap_or_else(|| {
            value
                .modified_at
                .timestamp_millis()
                .saturating_mul(1_000_000)
        }),
        quick_hash: value.quick_hash.clone(),
        tool_fingerprint: String::new(),
    }
}

fn store_port_error(error: StoreError) -> PortError {
    match error {
        StoreError::RevisionConflict { expected, actual } => PortError::Conflict(format!(
            "settings revision changed (expected {expected:?}, actual {actual})"
        )),
        StoreError::Sqlite(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
            ) =>
        {
            PortError::unavailable("SQLite database is busy", true)
        }
        error => PortError::Other(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn settings_repository_enforces_revisions() {
        let repositories =
            SqliteRepositories::from_store(SqliteStore::open_in_memory().expect("store"));
        let settings = AppSettings::default();
        let revision = SettingsRepository::save(&repositories, &settings, None)
            .await
            .expect("first save");
        assert_eq!(revision, 1);
        let error = SettingsRepository::save(&repositories, &settings, None)
            .await
            .expect_err("create-only save conflicts");
        assert!(matches!(error, PortError::Conflict(_)));
    }

    #[tokio::test]
    async fn incomplete_journals_are_enumerable_with_item_outcomes() {
        let repositories =
            SqliteRepositories::from_store(SqliteStore::open_in_memory().expect("store"));
        let mut record = JournalRecord {
            idempotency_key: IdempotencyKey::parse("journal-list").unwrap(),
            plan_id: PlanId::new(),
            step: 0,
            status: mkvo_application::JournalStatus::Running,
            resources: Vec::new(),
            items: vec![mkvo_application::JournalItemOutcome {
                key: "episode.mkv".to_owned(),
                status: mkvo_application::JournalItemStatus::Pending,
                detail: None,
            }],
            detail: None,
            updated_utc: Utc::now(),
        };
        OperationJournal::begin(&repositories, &record)
            .await
            .expect("begin");
        let incomplete = OperationJournal::list_incomplete(&repositories)
            .await
            .expect("list")
            .expect("supported");
        assert_eq!(incomplete, [record.clone()]);

        record.status = mkvo_application::JournalStatus::Completed;
        OperationJournal::advance(&repositories, &record)
            .await
            .expect("complete");
        assert!(
            OperationJournal::list_incomplete(&repositories)
                .await
                .expect("list")
                .expect("supported")
                .is_empty()
        );
    }
}
