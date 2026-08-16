use super::*;

impl MkvoRuntime {
    pub async fn get_status(&self) -> RuntimeResult<AppStatus> {
        let tools = self.inner.dependencies.tools.all_statuses().await?;
        let (default_root, library_roots) = self.scan_locations().await;

        // The host's own roots come from how it was launched; the library roots
        // are the user's. Both are shortcuts the browser offers, so callers see
        // one list, with the user's own winning a name collision.
        // An unrestricted host's own media root is the placeholder the runtime
        // needs some path for, not a library anyone chose. Offering it as a
        // shortcut would put an empty internal folder where the user's library
        // belongs, which is the hardcoded root wearing a different hat.
        let placeholder = (self.inner.config.browse_scope == BrowseScope::Unrestricted)
            .then(|| self.inner.config.media_root.clone());
        let mut source_roots: Vec<SourceRoot> = self
            .inner
            .config
            .source_roots
            .iter()
            .filter(|(_, path)| {
                placeholder
                    .as_deref()
                    .is_none_or(|hidden| !same_path(path, hidden))
            })
            .map(|(name, path)| SourceRoot {
                name: name.clone(),
                path: display_path(path),
            })
            .collect();
        for root in &library_roots {
            let path = display_path(&root.path);
            source_roots.retain(|existing| !same_path(Path::new(&existing.path), Path::new(&path)));
            source_roots.push(SourceRoot {
                name: root.name.clone(),
                path,
            });
        }

        Ok(AppStatus {
            name: self.inner.config.app_name.clone(),
            version: self.inner.config.version.clone(),
            // Empty means "no library folder configured", which only an
            // unrestricted host can report. Callers open the volume list.
            media_root: self
                .effective_media_root(default_root.as_ref(), &library_roots)
                .as_deref()
                .map_or_else(String::new, display_path),
            config_root: display_path(&self.inner.config.config_root),
            source_roots,
            tools,
            contract_version: mkvo_domain::CONTRACT_VERSION,
        })
    }

    /// Where browsing starts, or `None` when the user has no library yet.
    ///
    /// A host launched pointing at a mount keeps that as its root. An
    /// unrestricted desktop has no such thing: choosing one on the user's
    /// behalf is exactly what the hardcoded Videos folder did. So it uses the
    /// configured default directory. Older settings that stored Home as the
    /// first library folder retain that fallback until their next save migrates
    /// the value.
    fn effective_media_root(
        &self,
        default_root: Option<&PathBuf>,
        library_roots: &[LibraryRoot],
    ) -> Option<PathBuf> {
        if self.inner.config.browse_scope == BrowseScope::Unrestricted {
            return default_root
                .cloned()
                .or_else(|| library_roots.first().map(|root| root.path.clone()));
        }
        Some(self.inner.config.media_root.clone())
    }

    async fn scan_locations(&self) -> (Option<PathBuf>, Vec<LibraryRoot>) {
        self.settings_service()
            .load()
            .await
            .map(|loaded| {
                (
                    loaded.settings.scan.default_root,
                    loaded.settings.scan.library_roots,
                )
            })
            .unwrap_or_default()
    }

    pub(super) async fn browse_file_system_impl(
        &self,
        path: Option<String>,
    ) -> RuntimeResult<FileSystemResponse> {
        // An explicitly empty path means "the top" — the volume list — and is
        // distinct from no path at all, which means "wherever the host starts".
        // Collapsing the two would make the volume list unreachable, since the
        // media root would answer for both.
        let unrestricted = self.inner.config.browse_scope == BrowseScope::Unrestricted;
        if unrestricted && path.as_deref().is_some_and(|value| value.trim().is_empty()) {
            return Ok(volume_listing());
        }

        // A bare `\\server` is not a directory, so `read_dir` would only ever
        // report that the network name cannot be found. Its shares have to be
        // enumerated instead, which is how the user reaches a share that no
        // drive letter points at.
        if unrestricted
            && let Some(requested) = path.as_deref()
            && let Some(UncTarget::Server(server)) = classify_unc(requested)
        {
            return server_listing(&server);
        }

        let requested = match path.filter(|value| !value.trim().is_empty()) {
            Some(value) => PathBuf::from(value),
            None => {
                let (default_root, library_roots) = self.scan_locations().await;
                match self.effective_media_root(default_root.as_ref(), &library_roots) {
                    Some(root) => root,
                    // Nothing configured: show what there is to choose from
                    // rather than inventing a folder.
                    None => return Ok(volume_listing()),
                }
            }
        };
        // Listing is read-only. An unrestricted host still validates every
        // mutation against the authorized roots, so widening what the browser
        // may *show* does not widen what it may change.
        let authorized = if unrestricted {
            requested.clone()
        } else {
            self.inner
                .dependencies
                .paths
                .authorize_read(&requested)
                .await?
        };

        if !authorized.is_dir() {
            return Err(RuntimeError::invalid(format!(
                "browse path is not a directory: {}",
                display_path(&authorized)
            )));
        }

        let mut entries = Vec::new();
        let mut directory = tokio::fs::read_dir(&authorized).await?;
        while let Some(entry) = directory.next_entry().await? {
            let metadata = match entry.metadata().await {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let kind = if metadata.is_dir() {
                FileSystemEntryKind::Folder
            } else if metadata.is_file() {
                FileSystemEntryKind::File
            } else {
                continue;
            };
            entries.push(FileSystemEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: display_path(&entry.path()),
                kind,
                size_bytes: metadata.is_file().then_some(metadata.len()),
                modified_utc: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH).into(),
            });
        }
        entries.sort_by(|left, right| {
            let left_folder = left.kind == FileSystemEntryKind::Folder;
            let right_folder = right.kind == FileSystemEntryKind::Folder;
            right_folder
                .cmp(&left_folder)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        let parent_path = match self.inner.config.browse_scope {
            // Confined hosts stop at their roots: offering a parent the caller
            // cannot then list would be a dead end.
            BrowseScope::AuthorizedRootsOnly => authorized.parent().and_then(|parent| {
                self.inner
                    .config
                    .authorized_roots
                    .iter()
                    .any(|(root, _)| parent.starts_with(root))
                    .then(|| display_path(parent))
            }),
            // Above a volume root is the volume list, addressed as the empty
            // path, so the user can always navigate all the way out.
            BrowseScope::Unrestricted => Some(match authorized.parent() {
                Some(parent) => display_path(parent),
                // Rust folds `\\server\share` into a single path prefix, so a
                // share root reports no parent even though the server above it
                // is real and listable. Falling through to the volume list here
                // would skip a level that a file manager shows.
                None => match classify_unc(&display_path(&authorized)) {
                    Some(UncTarget::Share { server }) => format!(r"\\{server}"),
                    _ => String::new(),
                },
            }),
        };
        Ok(FileSystemResponse {
            path: display_path(&authorized),
            parent_path,
            entries,
        })
    }

    pub async fn start_scan(&self, mut request: ScanRequest) -> RuntimeResult<ScanJobResponse> {
        if request.max_workers.is_none() {
            request.max_workers = Some(
                self.settings_service()
                    .load()
                    .await?
                    .settings
                    .workers
                    .max_scan_workers,
            );
        }
        if request.all_sources().is_empty() {
            let (default_root, library_roots) = self.scan_locations().await;
            let root = self
                .effective_media_root(default_root.as_ref(), &library_roots)
                .ok_or_else(|| {
                    RuntimeError::invalid(
                        "no folder to scan: choose one, or set the default directory in Settings",
                    )
                })?;
            request.source_path = Some(display_path(&root));
        }
        let request_fingerprint = stable_fingerprint(&request)
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        let key = IdempotencyKey::generate();
        let scan = Arc::clone(&self.inner.scan);
        let current = Arc::clone(&self.inner.current_scan);
        // Scanning is the most-run operation, so it has to reach the operation
        // log too; otherwise the Logs page stays empty through normal use.
        let logs = Arc::clone(&self.inner.dependencies.logs);
        let spec = JobSpec {
            kind: JobKind::Scan,
            idempotency_key: key,
            request_fingerprint,
            plan_id: None,
            total: 0,
            resources: request
                .all_sources()
                .into_iter()
                .map(mkvo_domain::ResourceClaim::read)
                .collect(),
        };
        let accepted = self
            .inner
            .jobs
            .start(spec, move |context| async move {
                let outcome = scan
                    .scan(&request, context.cancellation_token(), &context)
                    .await?;
                let state = scan_result_state(&outcome);
                {
                    let mut current = current.write().await;
                    current.updated_utc = Some(Utc::now());
                    current.apply_scan(outcome.files.clone(), outcome.summary);
                }
                let summary = outcome.summary;
                let detail = format!(
                    "{} file(s): {} MKV, {} MP4, {} cached, {} failed",
                    summary.total, summary.mkv, summary.mp4, summary.cached, summary.failed
                );
                if let Err(error) = logs
                    .append(&mkvo_contracts::OperationLogEntry {
                        timestamp_utc: Utc::now(),
                        correlation_id: context.correlation_id(),
                        area: "Scan".to_owned(),
                        level: if summary.failed > 0 {
                            mkvo_contracts::LogLevel::Warning
                        } else {
                            mkvo_contracts::LogLevel::Information
                        },
                        message: "Scan completed".to_owned(),
                        detail,
                    })
                    .await
                {
                    tracing::warn!(%error, "scan completion could not be logged");
                }
                Ok(JobCompletion {
                    result: Some(serde_json::to_value(state).map_err(|error| {
                        ApplicationError::Internal(format!(
                            "scan result serialization failed: {error}"
                        ))
                    })?),
                    message: Some("Scan completed".to_owned()),
                })
            })
            .await?;
        let snapshot = self
            .inner
            .jobs
            .get(accepted.id)
            .await?
            .ok_or_else(|| RuntimeError::internal("new scan job was not persisted"))?;
        self.scan_job_response(&snapshot).await
    }

    pub async fn get_scan_job(&self, id: &str) -> RuntimeResult<ScanJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self
            .inner
            .jobs
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("scan job {id}")))?;
        if snapshot.kind != JobKind::Scan {
            return Err(RuntimeError::not_found(format!("scan job {id}")));
        }
        self.scan_job_response(&snapshot).await
    }

    pub async fn cancel_scan(&self, id: &str) -> RuntimeResult<ScanJobResponse> {
        let id = parse_job_id(id)?;
        let snapshot = self.inner.jobs.cancel(id).await?;
        if snapshot.kind != JobKind::Scan {
            return Err(RuntimeError::not_found(format!("scan job {id}")));
        }
        self.scan_job_response(&snapshot).await
    }

    pub async fn get_current_scan_files(&self) -> RuntimeResult<CurrentScanResponse> {
        let state = self.inner.current_scan.read().await;
        Ok(current_scan_response(&state))
    }

    /// Replace the selected working set.
    ///
    /// Paths outside the current scan are rejected rather than stored: a
    /// selection is only meaningful against files the backend actually knows
    /// about, and accepting an unknown path would let a stale or crafted
    /// request reach an operation.
    ///
    /// The selection lives exactly as long as the working set it refers to.
    /// Both are in-memory, so a page reload keeps them and a host restart
    /// clears them together — persisting the selection alone would restore a
    /// set of paths with no files behind them.
    pub async fn set_file_selection(
        &self,
        request: mkvo_contracts::FileSelectionRequest,
    ) -> RuntimeResult<CurrentScanResponse> {
        let mut state = self.inner.current_scan.write().await;
        let available: BTreeSet<String> = state
            .files
            .iter()
            .map(|file| mkvo_application::paths::path_key(&file.path))
            .collect();

        let mut selected = BTreeSet::new();
        let mut unknown = Vec::new();
        for path in &request.paths {
            let key = mkvo_application::paths::path_key(std::path::Path::new(path));
            if available.contains(&key) {
                selected.insert(key);
            } else {
                unknown.push(path.clone());
            }
        }
        if !unknown.is_empty() {
            return Err(RuntimeError::invalid(format!(
                "{} selected path(s) are not in the current scan: {}",
                unknown.len(),
                unknown.join(", ")
            )));
        }

        state.selected = selected;
        Ok(current_scan_response(&state))
    }

    /// Move the working set with files a rename just moved on disk.
    ///
    /// Without this the set still names paths that no longer exist, so the next
    /// operation resolves nothing and the dashboard shows the old names until a
    /// rescan.
    pub(crate) async fn apply_renames_to_working_set(&self, renames: &[(PathBuf, PathBuf)]) {
        {
            let mut state = self.inner.current_scan.write().await;
            state.apply_renames(renames);
            state.updated_utc = Some(Utc::now());
        }
        for (source, _) in renames {
            if let Err(error) = self.inner.dependencies.cache.remove(source).await {
                tracing::warn!(
                    path = %source.display(),
                    %error,
                    "renamed file's old cache entry could not be removed"
                );
            }
        }
        let targets: Vec<_> = renames.iter().map(|(_, target)| target.clone()).collect();
        self.refresh_working_set(&targets).await;
    }

    /// Re-read files an operation just changed, so the working set describes
    /// them as they are now rather than as they were when scanned.
    ///
    /// A property edit rewrites track names and languages in place: the paths
    /// are unchanged, so nothing else would notice, and the app would keep
    /// showing the old metadata until the user scanned again by hand. A file
    /// that can no longer be read is left as it was rather than dropped --
    /// a transient failure should not empty someone's working set.
    pub(crate) async fn refresh_working_set(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }

        let mut probed = Vec::with_capacity(paths.len());
        for path in paths {
            match self
                .inner
                .dependencies
                .probe
                .inspect(path, CancellationToken::new())
                .await
            {
                Ok(file) => probed.push(file),
                Err(error) => tracing::debug!(
                    path = %path.display(),
                    %error,
                    "file could not be re-read after an operation"
                ),
            }
        }

        for file in &probed {
            if let Err(error) = self.inner.dependencies.cache.upsert(file).await {
                tracing::warn!(
                    path = %file.path.display(),
                    %error,
                    "refreshed file metadata could not be cached"
                );
            }
        }

        let mut state = self.inner.current_scan.write().await;
        for file in probed {
            if let Some(existing) = state
                .files
                .iter_mut()
                .find(|candidate| same_path(&candidate.path, &file.path))
            {
                *existing = file;
            } else {
                state.files.push(file);
            }
        }
        state
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        state.reconcile_selection();
        state.updated_utc = Some(Utc::now());
    }

    pub async fn clear_current_scan_files(&self) -> RuntimeResult<CurrentScanResponse> {
        let mut state = self.inner.current_scan.write().await;
        *state = CurrentScanState::default();
        Ok(current_scan_response(&state))
    }

    async fn scan_job_response(&self, snapshot: &JobSnapshot) -> RuntimeResult<ScanJobResponse> {
        let state = scan_state_from_snapshot(snapshot).unwrap_or_default();
        Ok(ScanJobResponse::from_snapshot(
            snapshot,
            if state.rows.is_empty() {
                state.files.iter().map(MediaFileRow::from).collect()
            } else {
                state.rows
            },
            state.skipped,
            state.summary,
        ))
    }
}
