//! SQLite persistence for MKVO cache, settings, plans, jobs and rename history.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime},
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

mod adapter;
mod legacy;

pub use adapter::SqliteRepositories;
pub use legacy::{
    LegacyImportError, LegacyImportOutcome, LegacyRenameBatchRecord, import_legacy_settings,
    read_legacy_rename_history,
};

const SCHEMA_VERSION: u32 = 2;
const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SQLite connection lock was poisoned")]
    LockPoisoned,
    #[error("numeric value cannot be represented by SQLite")]
    NumericOverflow,
    #[error("settings revision conflict: expected {expected:?}, actual {actual}")]
    RevisionConflict { expected: Option<u64>, actual: u64 },
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheFingerprint {
    pub path: PathBuf,
    pub file_size: u64,
    pub modified_at_ns: i64,
    pub quick_hash: Option<String>,
    pub tool_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedMedia {
    pub fingerprint: CacheFingerprint,
    pub scanned_at_ms: i64,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredPlan {
    pub id: String,
    pub kind: String,
    pub version: u32,
    pub fingerprint: String,
    pub payload: Value,
    pub created_at_ms: i64,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredJob {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub request: Value,
    pub result: Option<Value>,
    pub idempotency_key: Option<String>,
    pub error: Option<Value>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub job_id: String,
    pub step_index: u32,
    pub step_kind: String,
    pub state: String,
    pub payload: Value,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameBatchEntry {
    pub original_path: PathBuf,
    pub renamed_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameBatch {
    pub id: String,
    pub created_at_ms: i64,
    pub undone_at_ms: Option<i64>,
    pub provider: String,
    pub template: String,
    pub entries: Vec<RenameBatchEntry>,
}

#[derive(Debug, Clone)]
pub struct SqliteStore {
    path: Arc<PathBuf>,
    connection: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        Self::open_with_timeout(path, DEFAULT_BUSY_TIMEOUT)
    }

    pub fn open_with_timeout(path: impl AsRef<Path>, busy_timeout: Duration) -> StoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        }
        let mut connection = Connection::open(&path)?;
        configure_connection(&connection, busy_timeout)?;
        migrate(&mut connection)?;
        Ok(Self {
            path: Arc::new(path),
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn open_in_memory() -> StoreResult<Self> {
        let mut connection = Connection::open_in_memory()?;
        configure_connection(&connection, DEFAULT_BUSY_TIMEOUT)?;
        migrate(&mut connection)?;
        Ok(Self {
            path: Arc::new(PathBuf::from(":memory:")),
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn schema_version(&self) -> StoreResult<u32> {
        let connection = self.connection()?;
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(StoreError::from)
    }

    pub fn journal_mode(&self) -> StoreResult<String> {
        let connection = self.connection()?;
        connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(StoreError::from)
    }

    pub fn cache_count(&self) -> StoreResult<u64> {
        let connection = self.connection()?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM media_cache", [], |row| row.get(0))?;
        u64::try_from(count).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn list_media_under(&self, root: &Path) -> StoreResult<Vec<CachedMedia>> {
        let root = path_text(root).trim_end_matches(['/', '\\']).to_owned();
        let escaped_root = escape_like(&root);
        let slash = format!("{escaped_root}/%");
        let backslash = format!("{escaped_root}\\%");
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT file_path, file_size, modified_at_ns, quick_hash, tool_fingerprint,
                    scanned_at_ms, payload_json
             FROM media_cache
             WHERE file_path = ?1 OR file_path LIKE ?2 ESCAPE '^' OR file_path LIKE ?3 ESCAPE '^'
             ORDER BY file_path",
        )?;
        let rows = statement.query_map(params![root, slash, backslash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (path, size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload) =
                row?;
            Ok(CachedMedia {
                fingerprint: CacheFingerprint {
                    path: PathBuf::from(path),
                    file_size: u64::try_from(size).map_err(|_| StoreError::NumericOverflow)?,
                    modified_at_ns,
                    quick_hash,
                    tool_fingerprint,
                },
                scanned_at_ms,
                payload: serde_json::from_str(&payload)?,
            })
        })
        .collect()
    }

    pub fn get_valid_media(
        &self,
        fingerprint: &CacheFingerprint,
    ) -> StoreResult<Option<CachedMedia>> {
        let path = path_text(&fingerprint.path);
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT file_size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload_json
                 FROM media_cache WHERE file_path = ?1",
                [&path],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload)) =
            row
        else {
            return Ok(None);
        };
        let size_matches = u64::try_from(size).ok() == Some(fingerprint.file_size);
        let quick_hash_matches = fingerprint.quick_hash.is_none()
            || quick_hash.is_none()
            || quick_hash == fingerprint.quick_hash;
        let matches = size_matches
            && modified_at_ns == fingerprint.modified_at_ns
            && quick_hash_matches
            && tool_fingerprint == fingerprint.tool_fingerprint;
        if !matches {
            connection.execute("DELETE FROM media_cache WHERE file_path = ?1", [&path])?;
            return Ok(None);
        }
        Ok(Some(CachedMedia {
            fingerprint: fingerprint.clone(),
            scanned_at_ms,
            payload: serde_json::from_str(&payload)?,
        }))
    }

    pub fn upsert_media(&self, media: &CachedMedia) -> StoreResult<()> {
        let size =
            i64::try_from(media.fingerprint.file_size).map_err(|_| StoreError::NumericOverflow)?;
        let payload = serde_json::to_string(&media.payload)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO media_cache
             (file_path, file_size, modified_at_ns, quick_hash, tool_fingerprint, scanned_at_ms, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(file_path) DO UPDATE SET
               file_size=excluded.file_size,
               modified_at_ns=excluded.modified_at_ns,
               quick_hash=excluded.quick_hash,
               tool_fingerprint=excluded.tool_fingerprint,
               scanned_at_ms=excluded.scanned_at_ms,
               payload_json=excluded.payload_json",
            params![
                path_text(&media.fingerprint.path),
                size,
                media.fingerprint.modified_at_ns,
                media.fingerprint.quick_hash,
                media.fingerprint.tool_fingerprint,
                media.scanned_at_ms,
                payload,
            ],
        )?;
        Ok(())
    }

    pub fn remove_media(&self, path: &Path) -> StoreResult<bool> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "DELETE FROM media_cache WHERE file_path = ?1",
            [path_text(path)],
        )? > 0)
    }

    pub fn remove_media_under(&self, root: &Path) -> StoreResult<u64> {
        let root = path_text(root).trim_end_matches(['/', '\\']).to_owned();
        let escaped_root = escape_like(&root);
        let slash = format!("{escaped_root}/%");
        let backslash = format!("{escaped_root}\\%");
        let connection = self.connection()?;
        let removed = connection.execute(
            "DELETE FROM media_cache WHERE file_path = ?1 OR file_path LIKE ?2 ESCAPE '^' OR file_path LIKE ?3 ESCAPE '^'",
            params![root, slash, backslash],
        )?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn remove_media_older_than(&self, cutoff_ms: i64) -> StoreResult<u64> {
        let connection = self.connection()?;
        let removed = connection.execute(
            "DELETE FROM media_cache WHERE scanned_at_ms < ?1",
            [cutoff_ms],
        )?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn clear_media_cache(&self) -> StoreResult<u64> {
        let connection = self.connection()?;
        let removed = connection.execute("DELETE FROM media_cache", [])?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, version: u32, value: &T) -> StoreResult<()> {
        let value = serde_json::to_string(value)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO settings (key, version, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET version=excluded.version,
             revision=settings.revision + 1, value_json=excluded.value_json,
             updated_at_ms=excluded.updated_at_ms",
            params![key, version, value, now_ms()],
        )?;
        Ok(())
    }

    pub fn get_setting<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> StoreResult<Option<(u32, T)>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT version, value_json FROM settings WHERE key = ?1",
                [key],
                |row| Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        row.map(|(version, json)| Ok((version, serde_json::from_str(&json)?)))
            .transpose()
    }

    pub fn get_setting_with_revision<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> StoreResult<Option<(u32, u64, T)>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT version, revision, value_json FROM settings WHERE key = ?1",
                [key],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(version, revision, json)| {
            let revision = u64::try_from(revision).map_err(|_| StoreError::NumericOverflow)?;
            Ok((version, revision, serde_json::from_str(&json)?))
        })
        .transpose()
    }

    pub fn save_setting_optimistic<T: Serialize>(
        &self,
        key: &str,
        version: u32,
        value: &T,
        expected_revision: Option<u64>,
    ) -> StoreResult<u64> {
        let value = serde_json::to_string(value)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let actual: Option<i64> = transaction
            .query_row(
                "SELECT revision FROM settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        let actual = actual
            .map(|revision| u64::try_from(revision).map_err(|_| StoreError::NumericOverflow))
            .transpose()?
            .unwrap_or(0);
        if let Some(expected) = expected_revision {
            if expected != actual {
                return Err(StoreError::RevisionConflict {
                    expected: Some(expected),
                    actual,
                });
            }
        } else if actual != 0 {
            return Err(StoreError::RevisionConflict {
                expected: None,
                actual,
            });
        }
        let next = actual.checked_add(1).ok_or(StoreError::NumericOverflow)?;
        let next_sql = i64::try_from(next).map_err(|_| StoreError::NumericOverflow)?;
        transaction.execute(
            "INSERT INTO settings (key, version, revision, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key) DO UPDATE SET version=excluded.version, revision=excluded.revision,
             value_json=excluded.value_json, updated_at_ms=excluded.updated_at_ms",
            params![key, version, next_sql, value, now_ms()],
        )?;
        transaction.commit()?;
        Ok(next)
    }

    pub fn put_plan(&self, plan: &StoredPlan) -> StoreResult<()> {
        let payload = serde_json::to_string(&plan.payload)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO plans (id, kind, version, fingerprint, payload_json, created_at_ms, expires_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, version=excluded.version,
             fingerprint=excluded.fingerprint, payload_json=excluded.payload_json,
             created_at_ms=excluded.created_at_ms, expires_at_ms=excluded.expires_at_ms",
            params![
                plan.id,
                plan.kind,
                plan.version,
                plan.fingerprint,
                payload,
                plan.created_at_ms,
                plan.expires_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get_plan(&self, id: &str, at_ms: i64) -> StoreResult<Option<StoredPlan>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT kind, version, fingerprint, payload_json, created_at_ms, expires_at_ms
                 FROM plans WHERE id = ?1 AND (expires_at_ms IS NULL OR expires_at_ms > ?2)",
                params![id, at_ms],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(kind, version, fingerprint, payload, created_at_ms, expires_at_ms)| {
                Ok(StoredPlan {
                    id: id.to_owned(),
                    kind,
                    version,
                    fingerprint,
                    payload: serde_json::from_str(&payload)?,
                    created_at_ms,
                    expires_at_ms,
                })
            },
        )
        .transpose()
    }

    pub fn upsert_job(&self, job: &StoredJob) -> StoreResult<()> {
        let request = serde_json::to_string(&job.request)?;
        let result = job.result.as_ref().map(serde_json::to_string).transpose()?;
        let error = job.error.as_ref().map(serde_json::to_string).transpose()?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO jobs
             (id, kind, state, request_json, result_json, idempotency_key, error_json, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, state=excluded.state,
             request_json=excluded.request_json, result_json=excluded.result_json,
             idempotency_key=excluded.idempotency_key, error_json=excluded.error_json,
             updated_at_ms=excluded.updated_at_ms",
            params![
                job.id,
                job.kind,
                job.state,
                request,
                result,
                job.idempotency_key,
                error,
                job.created_at_ms,
                job.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> StoreResult<Option<StoredJob>> {
        self.query_job("id", id)
    }

    pub fn list_recent_jobs(&self, limit: usize) -> StoreResult<Vec<StoredJob>> {
        let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow)?;
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("SELECT id FROM jobs ORDER BY updated_at_ms DESC LIMIT ?1")?;
        let ids = statement
            .query_map([limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        ids.into_iter()
            .map(|id| {
                self.get_job(&id)?
                    .ok_or_else(|| StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
            })
            .collect()
    }

    pub fn find_job_by_idempotency_key(&self, key: &str) -> StoreResult<Option<StoredJob>> {
        self.query_job("idempotency_key", key)
    }

    fn query_job(&self, column: &str, value: &str) -> StoreResult<Option<StoredJob>> {
        debug_assert!(matches!(column, "id" | "idempotency_key"));
        let sql = format!(
            "SELECT id, kind, state, request_json, result_json, idempotency_key, error_json, created_at_ms, updated_at_ms FROM jobs WHERE {column} = ?1"
        );
        let connection = self.connection()?;
        let row = connection
            .query_row(&sql, [value], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            })
            .optional()?;
        row.map(
            |(
                id,
                kind,
                state,
                request,
                result,
                idempotency_key,
                error,
                created_at_ms,
                updated_at_ms,
            )| {
                Ok(StoredJob {
                    id,
                    kind,
                    state,
                    request: serde_json::from_str(&request)?,
                    result: result.map(|json| serde_json::from_str(&json)).transpose()?,
                    idempotency_key,
                    error: error.map(|json| serde_json::from_str(&json)).transpose()?,
                    created_at_ms,
                    updated_at_ms,
                })
            },
        )
        .transpose()
    }

    pub fn put_journal_entry(&self, entry: &JournalEntry) -> StoreResult<()> {
        let payload = serde_json::to_string(&entry.payload)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO operation_journal
             (job_id, step_index, step_kind, state, payload_json, started_at_ms, completed_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(job_id, step_index) DO UPDATE SET step_kind=excluded.step_kind,
             state=excluded.state, payload_json=excluded.payload_json,
             started_at_ms=excluded.started_at_ms, completed_at_ms=excluded.completed_at_ms",
            params![
                entry.job_id,
                entry.step_index,
                entry.step_kind,
                entry.state,
                payload,
                entry.started_at_ms,
                entry.completed_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn journal_for_job(&self, job_id: &str) -> StoreResult<Vec<JournalEntry>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT step_index, step_kind, state, payload_json, started_at_ms, completed_at_ms
             FROM operation_journal WHERE job_id = ?1 ORDER BY step_index",
        )?;
        let rows = statement.query_map([job_id], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (step_index, step_kind, state, payload, started_at_ms, completed_at_ms) = row?;
            Ok(JournalEntry {
                job_id: job_id.to_owned(),
                step_index,
                step_kind,
                state,
                payload: serde_json::from_str(&payload)?,
                started_at_ms,
                completed_at_ms,
            })
        })
        .collect()
    }

    pub fn record_rename_batch(&self, batch: &RenameBatch) -> StoreResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO rename_batches (id, created_at_ms, undone_at_ms, provider, template, total_files)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET created_at_ms=excluded.created_at_ms,
             undone_at_ms=excluded.undone_at_ms, provider=excluded.provider,
             template=excluded.template, total_files=excluded.total_files",
            params![
                batch.id,
                batch.created_at_ms,
                batch.undone_at_ms,
                batch.provider,
                batch.template,
                batch.entries.len(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM rename_entries WHERE batch_id = ?1",
            [&batch.id],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO rename_entries (batch_id, position, original_path, renamed_path) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (position, entry) in batch.entries.iter().enumerate() {
                insert.execute(params![
                    batch.id,
                    position,
                    path_text(&entry.original_path),
                    path_text(&entry.renamed_path),
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_rename_batches(&self, limit: usize) -> StoreResult<Vec<RenameBatch>> {
        let limit = i64::try_from(limit).map_err(|_| StoreError::NumericOverflow)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at_ms, undone_at_ms, provider, template
             FROM rename_batches ORDER BY created_at_ms DESC LIMIT ?1",
        )?;
        let batch_rows = statement.query_map([limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut batches = Vec::new();
        for row in batch_rows {
            let (id, created_at_ms, undone_at_ms, provider, template) = row?;
            let entries = load_rename_entries(&connection, &id)?;
            batches.push(RenameBatch {
                id,
                created_at_ms,
                undone_at_ms,
                provider,
                template,
                entries,
            });
        }
        Ok(batches)
    }

    pub fn mark_rename_batch_undone(&self, id: &str, undone_at_ms: i64) -> StoreResult<bool> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "UPDATE rename_batches SET undone_at_ms = ?2 WHERE id = ?1 AND undone_at_ms IS NULL",
            params![id, undone_at_ms],
        )? > 0)
    }

    pub fn clear_rename_batches(&self) -> StoreResult<u64> {
        let connection = self.connection()?;
        let removed = connection.execute("DELETE FROM rename_batches", [])?;
        u64::try_from(removed).map_err(|_| StoreError::NumericOverflow)
    }

    pub fn integrity_check(&self) -> StoreResult<bool> {
        let connection = self.connection()?;
        let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        Ok(result == "ok")
    }

    fn connection(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

fn configure_connection(connection: &Connection, busy_timeout: Duration) -> StoreResult<()> {
    connection.busy_timeout(busy_timeout)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> StoreResult<()> {
    let current: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current < 1 {
        let transaction = connection.transaction()?;
        migration_v1(&transaction)?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
    }
    if current < 2 {
        let transaction = connection.transaction()?;
        migration_v2(&transaction)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    Ok(())
}

fn migration_v1(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS media_cache (
            file_path TEXT PRIMARY KEY,
            file_size INTEGER NOT NULL CHECK(file_size >= 0),
            modified_at_ns INTEGER NOT NULL,
            quick_hash TEXT,
            tool_fingerprint TEXT NOT NULL,
            scanned_at_ms INTEGER NOT NULL,
            payload_json TEXT NOT NULL CHECK(json_valid(payload_json))
        );
        CREATE INDEX IF NOT EXISTS idx_media_cache_scanned ON media_cache(scanned_at_ms);

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            version INTEGER NOT NULL CHECK(version > 0),
            value_json TEXT NOT NULL CHECK(json_valid(value_json)),
            updated_at_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS plans (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            version INTEGER NOT NULL CHECK(version > 0),
            fingerprint TEXT NOT NULL,
            payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
            created_at_ms INTEGER NOT NULL,
            expires_at_ms INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_plans_expiry ON plans(expires_at_ms);

        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            state TEXT NOT NULL,
            request_json TEXT NOT NULL CHECK(json_valid(request_json)),
            result_json TEXT CHECK(result_json IS NULL OR json_valid(result_json)),
            idempotency_key TEXT,
            error_json TEXT CHECK(error_json IS NULL OR json_valid(error_json)),
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_idempotency
            ON jobs(idempotency_key) WHERE idempotency_key IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_jobs_state_updated ON jobs(state, updated_at_ms);

        CREATE TABLE IF NOT EXISTS operation_journal (
            job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
            step_index INTEGER NOT NULL CHECK(step_index >= 0),
            step_kind TEXT NOT NULL,
            state TEXT NOT NULL,
            payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
            started_at_ms INTEGER,
            completed_at_ms INTEGER,
            PRIMARY KEY(job_id, step_index)
        );

        CREATE TABLE IF NOT EXISTS rename_batches (
            id TEXT PRIMARY KEY,
            created_at_ms INTEGER NOT NULL,
            undone_at_ms INTEGER,
            provider TEXT NOT NULL,
            template TEXT NOT NULL,
            total_files INTEGER NOT NULL CHECK(total_files >= 0)
        );
        CREATE INDEX IF NOT EXISTS idx_rename_batches_created ON rename_batches(created_at_ms DESC);

        CREATE TABLE IF NOT EXISTS rename_entries (
            batch_id TEXT NOT NULL REFERENCES rename_batches(id) ON DELETE CASCADE,
            position INTEGER NOT NULL CHECK(position >= 0),
            original_path TEXT NOT NULL,
            renamed_path TEXT NOT NULL,
            PRIMARY KEY(batch_id, position)
        );",
    )?;
    Ok(())
}

fn migration_v2(transaction: &Transaction<'_>) -> StoreResult<()> {
    transaction.execute_batch(
        "ALTER TABLE settings ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;

         CREATE TABLE IF NOT EXISTS job_events (
            job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
            sequence INTEGER NOT NULL CHECK(sequence >= 0),
            payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
            emitted_at_ms INTEGER NOT NULL,
            PRIMARY KEY(job_id, sequence)
         );

         CREATE TABLE IF NOT EXISTS mutation_journal (
            idempotency_key TEXT PRIMARY KEY,
            plan_id TEXT NOT NULL,
            step INTEGER NOT NULL CHECK(step >= 0),
            status TEXT NOT NULL,
            payload_json TEXT NOT NULL CHECK(json_valid(payload_json)),
            updated_at_ms INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS operation_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp_ms INTEGER NOT NULL,
            correlation_id TEXT NOT NULL,
            area TEXT NOT NULL,
            level TEXT NOT NULL,
            message TEXT NOT NULL,
            detail TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_operation_logs_time ON operation_logs(timestamp_ms DESC);

         ALTER TABLE rename_batches ADD COLUMN payload_json TEXT
            CHECK(payload_json IS NULL OR json_valid(payload_json));",
    )?;
    Ok(())
}

fn load_rename_entries(
    connection: &Connection,
    batch_id: &str,
) -> StoreResult<Vec<RenameBatchEntry>> {
    let mut statement = connection.prepare(
        "SELECT original_path, renamed_path FROM rename_entries WHERE batch_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map([batch_id], |row| {
        Ok(RenameBatchEntry {
            original_path: PathBuf::from(row.get::<_, String>(0)?),
            renamed_path: PathBuf::from(row.get::<_, String>(1)?),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn escape_like(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "^%")
        .replace('_', "^_")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::UNIX_EPOCH;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new(name: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "mkvo-sqlite-{name}-{}-{stamp}.db",
                std::process::id()
            )))
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{}", self.0.display(), suffix));
            }
        }
    }

    fn fingerprint(path: &str, size: u64) -> CacheFingerprint {
        CacheFingerprint {
            path: PathBuf::from(path),
            file_size: size,
            modified_at_ns: 123,
            quick_hash: Some("quick".to_owned()),
            tool_fingerprint: "mkvmerge-v1".to_owned(),
        }
    }

    #[test]
    fn initializes_wal_schema() {
        let database = TestDatabase::new("schema");
        let store = SqliteStore::open(&database.0).expect("open store");
        assert_eq!(store.schema_version().expect("version"), SCHEMA_VERSION);
        assert_eq!(store.journal_mode().expect("journal mode"), "wal");
        assert!(store.integrity_check().expect("integrity"));
    }

    #[test]
    fn invalidates_stale_cache_entry() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let media = CachedMedia {
            fingerprint: fingerprint("library/a.mkv", 10),
            scanned_at_ms: 1,
            payload: json!({"tracks": 2}),
        };
        store.upsert_media(&media).expect("upsert");
        assert!(
            store
                .get_valid_media(&media.fingerprint)
                .expect("read")
                .is_some()
        );

        let stale = fingerprint("library/a.mkv", 11);
        assert!(store.get_valid_media(&stale).expect("read stale").is_none());
        assert_eq!(store.cache_count().expect("count"), 0);
    }

    #[test]
    fn persists_settings_and_expiring_plans() {
        let store = SqliteStore::open_in_memory().expect("open store");
        store
            .set_setting("app", 1, &json!({"theme":"dark"}))
            .expect("save");
        let (_, value): (u32, Value) = store.get_setting("app").expect("load").expect("setting");
        assert_eq!(value["theme"], "dark");

        let plan = StoredPlan {
            id: "plan-1".to_owned(),
            kind: "rename".to_owned(),
            version: 1,
            fingerprint: "abc".to_owned(),
            payload: json!({"moves": []}),
            created_at_ms: 10,
            expires_at_ms: Some(20),
        };
        store.put_plan(&plan).expect("save plan");
        assert!(store.get_plan("plan-1", 19).expect("load plan").is_some());
        assert!(
            store
                .get_plan("plan-1", 20)
                .expect("expired plan")
                .is_none()
        );
    }

    #[test]
    fn job_idempotency_and_journal_round_trip() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let job = StoredJob {
            id: "job-1".to_owned(),
            kind: "remux".to_owned(),
            state: "running".to_owned(),
            request: json!({"planId":"p1"}),
            result: None,
            idempotency_key: Some("request-1".to_owned()),
            error: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        };
        store.upsert_job(&job).expect("save job");
        assert_eq!(
            store
                .find_job_by_idempotency_key("request-1")
                .expect("lookup"),
            Some(job)
        );

        let journal = JournalEntry {
            job_id: "job-1".to_owned(),
            step_index: 0,
            step_kind: "write_temp".to_owned(),
            state: "completed".to_owned(),
            payload: json!({"temp":"file.part"}),
            started_at_ms: Some(3),
            completed_at_ms: Some(4),
        };
        store.put_journal_entry(&journal).expect("save journal");
        assert_eq!(
            store.journal_for_job("job-1").expect("load journal"),
            vec![journal]
        );
    }

    #[test]
    fn rename_history_is_transactional_and_ordered() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let batch = RenameBatch {
            id: "batch-1".to_owned(),
            created_at_ms: 100,
            undone_at_ms: None,
            provider: "tvdb".to_owned(),
            template: "{series}".to_owned(),
            entries: vec![RenameBatchEntry {
                original_path: PathBuf::from("old.mkv"),
                renamed_path: PathBuf::from("new.mkv"),
            }],
        };
        store.record_rename_batch(&batch).expect("record batch");
        assert_eq!(store.list_rename_batches(10).expect("list"), vec![batch]);
        assert!(
            store
                .mark_rename_batch_undone("batch-1", 200)
                .expect("mark undone")
        );
        assert_eq!(
            store.list_rename_batches(10).expect("list")[0].undone_at_ms,
            Some(200)
        );
    }
}
