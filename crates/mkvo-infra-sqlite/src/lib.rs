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
mod journal_history;
mod legacy;
mod media_cache;
mod plans_jobs;
mod settings_store;

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

/// Cache rows are keyed by path, and the same file arrives spelled differently
/// from a canonicalized scan than from a filesystem-watch event. Both must
/// resolve to one row or lookups and deletions silently miss.
fn path_text(path: &Path) -> String {
    mkvo_domain::normalized_path_text(path)
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
    fn removes_only_cache_entries_older_than_the_retention_cutoff() {
        let store = SqliteStore::open_in_memory().expect("open store");
        for (path, scanned_at_ms) in [("old.mkv", 100), ("boundary.mkv", 200), ("new.mkv", 300)] {
            store
                .upsert_media(&CachedMedia {
                    fingerprint: fingerprint(path, 10),
                    scanned_at_ms,
                    payload: json!({"tracks": 1}),
                })
                .expect("upsert");
        }

        assert_eq!(store.remove_media_older_than(200).expect("prune"), 1);
        assert_eq!(store.cache_count().expect("count"), 2);
        assert!(
            store
                .get_valid_media(&fingerprint("boundary.mkv", 10))
                .expect("boundary")
                .is_some()
        );
        assert!(
            store
                .get_valid_media(&fingerprint("new.mkv", 10))
                .expect("new")
                .is_some()
        );
    }

    /// A scan writes canonicalized paths (`\\?\C:\...` on Windows) while
    /// filesystem-watch events report the plain form. If those are two keys, a
    /// deletion never matches a cache row and pruning silently does nothing,
    /// leaving the cache advertising files that no longer exist.
    #[test]
    fn cache_rows_are_reachable_by_either_windows_path_spelling() {
        let store = SqliteStore::open_in_memory().expect("open store");
        let media = CachedMedia {
            fingerprint: fingerprint(r"\\?\C:\media\Show\a.mkv", 10),
            scanned_at_ms: 1,
            payload: json!({"tracks": 2}),
        };
        store.upsert_media(&media).expect("upsert");
        assert_eq!(store.cache_count().expect("count"), 1);

        // The watcher reports the plain spelling of the same file.
        assert!(
            store
                .remove_media(Path::new(r"C:\media\Show\a.mkv"))
                .expect("remove"),
            "a plain-form path must match the canonical row it was stored under"
        );
        assert_eq!(store.cache_count().expect("count"), 0);
    }

    /// Subtree pruning has the same two-spelling problem: a deleted folder is
    /// reported plainly but its rows were written canonically.
    #[test]
    fn subtree_pruning_matches_either_windows_path_spelling() {
        let store = SqliteStore::open_in_memory().expect("open store");
        for name in ["a.mkv", "b.mkv"] {
            store
                .upsert_media(&CachedMedia {
                    fingerprint: fingerprint(&format!(r"\\?\C:\media\Show\{name}"), 10),
                    scanned_at_ms: 1,
                    payload: json!({"tracks": 1}),
                })
                .expect("upsert");
        }
        assert_eq!(store.cache_count().expect("count"), 2);

        let removed = store
            .remove_media_under(Path::new(r"C:\media\Show"))
            .expect("remove under");
        assert_eq!(removed, 2);
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
