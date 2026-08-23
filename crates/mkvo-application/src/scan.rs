use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use futures::{StreamExt, stream};
use mkvo_contracts::{JobEvent, MediaFileDto, ScanRequest, ScanResponse, ScanSummary};
use mkvo_domain::MediaFile;
use tokio_util::sync::CancellationToken;

use crate::{
    ApplicationError, ApplicationResult, AuthorizedPathPolicy, JobContext, MediaCatalog,
    MediaEnumerationRequest, MediaProbe, MetadataCache,
};

const MEDIA_CACHE_RETENTION_DAYS: i64 = 7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanSkip {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScanOutcome {
    pub files: Vec<MediaFile>,
    pub skipped: Vec<ScanSkip>,
    pub summary: ScanSummary,
}

impl ScanOutcome {
    #[must_use]
    pub fn into_response(self) -> ScanResponse {
        ScanResponse {
            files: self.files.iter().map(MediaFileDto::from).collect(),
            skipped: self
                .skipped
                .into_iter()
                .map(|skip| format!("{}: {}", skip.path.display(), skip.reason))
                .collect(),
            summary: self.summary,
        }
    }
}

#[async_trait]
pub trait ScanEventSink: Send + Sync {
    async fn file_discovered(&self, file: &MediaFile) -> ApplicationResult<()>;
    async fn progress(&self, completed: u64, total: u64, current: &str) -> ApplicationResult<()>;
    async fn skipped(&self, path: &str, reason: &str) -> ApplicationResult<()>;
}

pub struct NoopScanEventSink;

#[async_trait]
impl ScanEventSink for NoopScanEventSink {
    async fn file_discovered(&self, _file: &MediaFile) -> ApplicationResult<()> {
        Ok(())
    }

    async fn progress(
        &self,
        _completed: u64,
        _total: u64,
        _current: &str,
    ) -> ApplicationResult<()> {
        Ok(())
    }

    async fn skipped(&self, _path: &str, _reason: &str) -> ApplicationResult<()> {
        Ok(())
    }
}

#[async_trait]
impl ScanEventSink for JobContext {
    async fn file_discovered(&self, file: &MediaFile) -> ApplicationResult<()> {
        self.emit(JobEvent::MediaDiscovered {
            file: MediaFileDto::from(file),
        })
        .await
    }

    async fn progress(&self, completed: u64, total: u64, current: &str) -> ApplicationResult<()> {
        JobContext::progress(self, completed, total, current, 100).await
    }

    async fn skipped(&self, path: &str, reason: &str) -> ApplicationResult<()> {
        self.record_skipped().await?;
        self.log(
            mkvo_contracts::JobLogLevel::Warning,
            format!("Skipped {path}: {reason}"),
        )
        .await
    }
}

pub struct ScanService {
    catalog: Arc<dyn MediaCatalog>,
    probe: Arc<dyn MediaProbe>,
    cache: Arc<dyn MetadataCache>,
    paths: Arc<dyn AuthorizedPathPolicy>,
    default_workers: usize,
}

impl ScanService {
    #[must_use]
    pub fn new(
        catalog: Arc<dyn MediaCatalog>,
        probe: Arc<dyn MediaProbe>,
        cache: Arc<dyn MetadataCache>,
        paths: Arc<dyn AuthorizedPathPolicy>,
        default_workers: usize,
    ) -> Self {
        Self {
            catalog,
            probe,
            cache,
            paths,
            default_workers: default_workers.clamp(1, 8),
        }
    }

    pub async fn scan(
        &self,
        request: &ScanRequest,
        cancel: CancellationToken,
        sink: &dyn ScanEventSink,
    ) -> ApplicationResult<ScanOutcome> {
        let requested_sources = request.all_sources();
        if requested_sources.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "at least one scan source is required".to_owned(),
            ));
        }

        let mut roots = Vec::with_capacity(requested_sources.len());
        for source in requested_sources {
            if source.trim().is_empty() {
                continue;
            }
            let authorized = self
                .paths
                .authorize_read(PathBuf::from(source).as_path())
                .await?;
            roots.push(authorized);
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "scan sources were blank".to_owned(),
            ));
        }

        // Temporary working directories are the common scan source. Prune old
        // probe payloads before looking up this scan so moved-away files cannot
        // accumulate indefinitely in the persistent SQLite database.
        let cache_cutoff = Utc::now() - TimeDelta::days(MEDIA_CACHE_RETENTION_DAYS);
        self.cache.remove_older_than(cache_cutoff).await?;

        let enumeration = MediaEnumerationRequest {
            roots,
            ignored_folder_names: request
                .ignored_folder_names
                .iter()
                .map(|value| value.trim().to_lowercase())
                .filter(|value| !value.is_empty())
                .collect(),
            supported_extensions: ["mkv", "mka", "webm", "mp4", "m4v"]
                .into_iter()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>(),
        };
        let fingerprints = self.catalog.enumerate(&enumeration, cancel.clone()).await?;
        let total = u64::try_from(fingerprints.len()).unwrap_or(u64::MAX);
        let workers = request
            .max_workers
            .unwrap_or(self.default_workers)
            .clamp(1, 8);

        let cache = Arc::clone(&self.cache);
        let probe = Arc::clone(&self.probe);
        let force_refresh = request.force_refresh;
        let child_cancel = cancel.clone();
        let mut work = stream::iter(fingerprints.into_iter().map(move |fingerprint| {
            let cache = Arc::clone(&cache);
            let probe = Arc::clone(&probe);
            let cancel = child_cancel.clone();
            async move {
                if cancel.is_cancelled() {
                    return Err((fingerprint.path.clone(), ApplicationError::Canceled));
                }
                if !force_refresh {
                    match cache.get_valid(&fingerprint).await {
                        Ok(Some(file)) => return Ok((file, true)),
                        Ok(None) => {}
                        Err(error) => return Err((fingerprint.path.clone(), error.into())),
                    }
                }
                match probe.inspect(&fingerprint.path, cancel).await {
                    Ok(mut file) => {
                        file.fingerprint = fingerprint;
                        if let Err(error) = cache.upsert(&file).await {
                            return Err((file.path.clone(), error.into()));
                        }
                        Ok((file, false))
                    }
                    Err(error) => Err((fingerprint.path.clone(), error.into())),
                }
            }
        }))
        .buffer_unordered(workers);

        let mut outcome = ScanOutcome::default();
        while let Some(result) = work.next().await {
            if cancel.is_cancelled() {
                return Err(ApplicationError::Canceled);
            }
            match result {
                Ok((file, cached)) => {
                    outcome.summary.total += 1;
                    if cached {
                        outcome.summary.cached += 1;
                    }
                    let extension = file.extension();
                    if extension.eq_ignore_ascii_case("mkv") {
                        outcome.summary.mkv += 1;
                    } else if extension.eq_ignore_ascii_case("mp4")
                        || extension.eq_ignore_ascii_case("m4v")
                    {
                        outcome.summary.mp4 += 1;
                    }
                    sink.file_discovered(&file).await?;
                    let completed = outcome.summary.total + outcome.summary.failed;
                    sink.progress(completed, total, &file.file_name()).await?;
                    outcome.files.push(file);
                }
                Err((_path, ApplicationError::Canceled)) => {
                    return Err(ApplicationError::Canceled);
                }
                Err((path, error)) => {
                    outcome.summary.failed += 1;
                    let reason = error.to_string();
                    sink.skipped(&path.to_string_lossy(), &reason).await?;
                    let completed = outcome.summary.total + outcome.summary.failed;
                    sink.progress(completed, total, &path.to_string_lossy())
                        .await?;
                    outcome.skipped.push(ScanSkip { path, reason });
                }
            }
        }
        outcome
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use async_trait::async_trait;
    use chrono::Utc;
    use mkvo_domain::{ContainerMetadata, FileFingerprint, MediaStatus};
    use tokio::sync::RwLock;

    use super::*;
    use crate::PortError;

    struct Catalog(Vec<FileFingerprint>);
    #[async_trait]
    impl MediaCatalog for Catalog {
        async fn enumerate(
            &self,
            _request: &MediaEnumerationRequest,
            _cancel: CancellationToken,
        ) -> Result<Vec<FileFingerprint>, PortError> {
            Ok(self.0.clone())
        }
    }

    struct Probe;
    #[async_trait]
    impl MediaProbe for Probe {
        async fn inspect(
            &self,
            path: &Path,
            _cancel: CancellationToken,
        ) -> Result<MediaFile, PortError> {
            Ok(media(path))
        }
    }

    #[derive(Default)]
    struct Cache {
        files: RwLock<HashMap<PathBuf, MediaFile>>,
        prune_cutoffs: RwLock<Vec<chrono::DateTime<Utc>>>,
    }
    #[async_trait]
    impl MetadataCache for Cache {
        async fn get_valid(
            &self,
            fingerprint: &FileFingerprint,
        ) -> Result<Option<MediaFile>, PortError> {
            Ok(self.files.read().await.get(&fingerprint.path).cloned())
        }
        async fn upsert(&self, file: &MediaFile) -> Result<(), PortError> {
            self.files
                .write()
                .await
                .insert(file.path.clone(), file.clone());
            Ok(())
        }
        async fn remove(&self, path: &Path) -> Result<bool, PortError> {
            Ok(self.files.write().await.remove(path).is_some())
        }
        async fn remove_under(&self, _root: &Path) -> Result<u64, PortError> {
            Ok(0)
        }
        async fn remove_older_than(&self, cutoff: chrono::DateTime<Utc>) -> Result<u64, PortError> {
            self.prune_cutoffs.write().await.push(cutoff);
            Ok(0)
        }
        async fn count(&self) -> Result<u64, PortError> {
            Ok(self.files.read().await.len() as u64)
        }
        async fn list_under(&self, _root: &Path) -> Result<Vec<MediaFile>, PortError> {
            Ok(self.files.read().await.values().cloned().collect())
        }
    }

    struct Paths;
    #[async_trait]
    impl AuthorizedPathPolicy for Paths {
        async fn authorize_read(&self, path: &Path) -> Result<PathBuf, PortError> {
            Ok(path.to_owned())
        }
        async fn authorize_write(&self, path: &Path) -> Result<PathBuf, PortError> {
            Ok(path.to_owned())
        }
    }

    fn fingerprint(path: &str) -> FileFingerprint {
        FileFingerprint {
            path: PathBuf::from(path),
            size_bytes: 10,
            modified_at: Utc::now(),
            quick_hash: None,
        }
    }

    fn media(path: &Path) -> MediaFile {
        MediaFile {
            path: path.to_owned(),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: fingerprint(&path.to_string_lossy()),
            container: ContainerMetadata::default(),
            tracks: Vec::new(),
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    #[tokio::test]
    async fn scan_uses_cache_and_probes_misses() {
        let one = fingerprint("one.mkv");
        let two = fingerprint("two.mp4");
        let cache = Arc::new(Cache::default());
        cache.upsert(&media(Path::new("one.mkv"))).await.unwrap();
        let service = ScanService::new(
            Arc::new(Catalog(vec![one, two])),
            Arc::new(Probe),
            cache.clone(),
            Arc::new(Paths),
            2,
        );
        let outcome = service
            .scan(
                &ScanRequest {
                    source_path: Some(".".to_owned()),
                    ..ScanRequest::default()
                },
                CancellationToken::new(),
                &NoopScanEventSink,
            )
            .await
            .unwrap();
        assert_eq!(outcome.files.len(), 2);
        assert_eq!(outcome.summary.cached, 1);
        assert_eq!(outcome.summary.mkv, 1);
        assert_eq!(outcome.summary.mp4, 1);
        let cutoffs = cache.prune_cutoffs.read().await;
        assert_eq!(cutoffs.len(), 1);
        let expected = Utc::now() - TimeDelta::days(MEDIA_CACHE_RETENTION_DAYS);
        assert!((cutoffs[0] - expected).num_seconds().abs() <= 1);
    }
}
