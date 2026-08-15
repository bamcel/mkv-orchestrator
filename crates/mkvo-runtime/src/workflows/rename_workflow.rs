use super::*;

impl MkvoRuntime {
    pub(super) async fn build_rename_preview_impl(
        &self,
        request: RenamePreviewRequest,
    ) -> RuntimeResult<RenamePreviewResponse> {
        let mut files = self.resolve_rows(&request.files, &[]).await?;
        if files.is_empty() {
            return Err(RuntimeError::invalid(
                "Scan and select files before building a rename preview.",
            ));
        }
        let provider = request
            .provider
            .as_deref()
            .map(parse_provider)
            .transpose()?
            .unwrap_or(MetadataProvider::Tvdb);
        let episodes = self
            .load_rename_episodes(
                provider,
                &request.selected_result.media_id(),
                request.language.as_deref(),
            )
            .await?;
        let selected_seasons = selected_seasons(&request.scope_keys);
        let is_movie = request.selected_result.format.eq_ignore_ascii_case("movie");
        for file in &mut files {
            // A film has no episode number to parse out of its name, so
            // requiring one left every movie unmatched and every token empty --
            // the rename came out as the bare punctuation of the template. The
            // provider returns exactly one entry for a film, and that is the
            // match.
            let matched = if is_movie {
                episodes.first()
            } else {
                match_episode_for_file(&file.file_name(), &episodes, &selected_seasons)
            };
            if let Some(episode) = matched {
                file.episode = Some(EpisodeIdentity {
                    series_title: Some(request.selected_result.name.clone()),
                    season: Some(episode.season),
                    episode: Some(episode.episode),
                    absolute_episode: episode.absolute_episode,
                    episode_title: Some(episode.title.clone()),
                    year: request.selected_result.year.parse().ok(),
                    is_movie: request.selected_result.format.eq_ignore_ascii_case("movie"),
                });
                file.provider_match = Some(ProviderMatch {
                    provider,
                    media_id: request.selected_result.media_id(),
                    episode_id: Some(episode.id.clone()),
                    title: request.selected_result.name.clone(),
                    episode_title: Some(episode.title.clone()),
                    confidence: Some(100),
                });
            }
        }
        let settings = self.dependencies().settings.load().await?.0;
        let settings_fingerprint = stable_fingerprint(&settings)
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        let key = request
            .idempotency_key
            .unwrap_or_else(IdempotencyKey::generate);
        let files_for_access = files.clone();
        let plan = RenamePlanner.build_plan(RenamePlanRequest {
            files,
            template: request
                .template
                .unwrap_or_else(|| settings.rename.template.clone()),
            provider: Some(provider),
            check_existing_files: true,
            existing_paths: self.current_existing_paths().await,
            source_access: self
                .probe_source_access(&files_for_access, RequiredAccess::ReadWrite)
                .await,
            existing_parents: self.existing_parents(&files_for_access).await,
            authorized_roots: self.authorized_root_paths(),
            settings_fingerprint,
            expires_in_seconds: 900,
            idempotency_key: key.clone(),
        })?;
        self.persist_plan(&plan).await?;
        let scopes = scope_rows(&episodes);
        Ok(rename_preview_response(&plan, scopes, key))
    }

    pub(super) async fn probe_source_access(
        &self,
        files: &[MediaFile],
        access: RequiredAccess,
    ) -> BTreeMap<String, FileAccessState> {
        let mut probed = BTreeMap::new();
        for file in files {
            match self
                .dependencies()
                .file_system
                .probe_access(&file.path, access)
                .await
            {
                Ok(state) => {
                    probed.insert(mkvo_application::paths::path_key(&file.path), state);
                }
                Err(error) => {
                    tracing::debug!(
                        path = %file.path.display(),
                        %error,
                        "source access could not be probed"
                    );
                }
            }
        }
        probed
    }

    pub(super) async fn existing_parents(&self, files: &[MediaFile]) -> BTreeSet<String> {
        let mut parents = BTreeSet::new();
        for parent in files.iter().filter_map(|file| file.path.parent()) {
            if self
                .dependencies()
                .file_system
                .is_directory(parent)
                .await
                .unwrap_or(false)
            {
                parents.insert(mkvo_application::paths::path_key(parent));
            }
        }
        parents
    }

    pub async fn apply_rename_preview(
        &self,
        request: RenameApplyRequest,
    ) -> RuntimeResult<RenameApplyResponse> {
        let (plan_id, fingerprint, key) = require_plan_fields(
            request.plan_id,
            request.plan_fingerprint,
            request.idempotency_key,
        )?;
        let referenced: RenamePlan = self
            .load_referenced_plan(plan_id, &fingerprint, &key)
            .await?;
        let resources = referenced.context.resources.clone();
        let runtime = self.clone();
        let (plan, replay) = self
            .jobs()
            .with_resource_lease(&resources, tokio_util::sync::CancellationToken::new(), || {
                async move {
                    // Re-check after acquiring the lease. A concurrent request
                    // may have completed while this request was waiting.
                    if let Some(journal) = runtime.dependencies().journal.get(&key).await?
                        && journal.status == mkvo_application::JournalStatus::Completed
                    {
                        return Ok((referenced, true));
                    }
                    let plan: RenamePlan = runtime
                        .load_valid_plan(plan_id, &fingerprint, &key)
                        .await
                        .map_err(runtime_application_error)?;
                    let mut journal = mkvo_application::JournalRecord {
                        idempotency_key: key.clone(),
                        plan_id,
                        step: 0,
                        status: mkvo_application::JournalStatus::Prepared,
                        resources: plan.context.resources.clone(),
                        items: plan
                            .payload
                            .items
                            .iter()
                            .filter(|item| item.can_apply())
                            .map(|item| mkvo_application::JournalItemOutcome {
                                key: item.source.to_string_lossy().into_owned(),
                                status: mkvo_application::JournalItemStatus::Pending,
                                detail: None,
                            })
                            .collect(),
                        detail: None,
                        updated_utc: Utc::now(),
                    };
                    runtime.dependencies().journal.begin(&journal).await?;
                    journal.status = mkvo_application::JournalStatus::Running;
                    runtime.dependencies().journal.advance(&journal).await?;

                    let batch_id = RenameBatchId::new();
                    let mut batch = RenameBatchRecord {
                        id: batch_id,
                        created_at: Utc::now(),
                        undone_at: None,
                        provider: plan.payload.provider,
                        template: plan.payload.template.clone(),
                        entries: Vec::new(),
                    };
                    let mutation = async {
                        for item in plan.payload.items.iter().filter(|item| item.can_apply()) {
                            runtime
                                .revalidate_fingerprint(&item.source_fingerprint)
                                .await
                                .map_err(runtime_application_error)?;
                            journal.detail = Some(format!(
                                "prepared move {} -> {}; batchId={batch_id}",
                                item.source.display(),
                                item.target.display()
                            ));
                            journal.updated_utc = Utc::now();
                            runtime.dependencies().journal.advance(&journal).await?;
                            runtime
                                .dependencies()
                                .file_system
                                .move_file(&item.source, &item.target)
                                .await?;
                            let renamed_fingerprint = runtime
                                .dependencies()
                                .file_system
                                .fingerprint(&item.target)
                                .await
                                .ok();
                            batch.entries.push(RenameBatchEntry {
                                original_path: item.source.clone(),
                                renamed_path: item.target.clone(),
                                original_fingerprint: item.source_fingerprint.clone(),
                                renamed_fingerprint,
                            });
                            // Persist after every move. Together with the pre-move journal
                            // detail this permits deterministic crash reconciliation.
                            runtime.dependencies().rename_history.add(&batch).await?;
                            journal.step = journal.step.saturating_add(1);
                            journal.complete_item(&item.source.to_string_lossy());
                            journal.detail = Some(format!(
                                "completed move {} -> {}; batchId={batch_id}",
                                item.source.display(),
                                item.target.display()
                            ));
                            journal.updated_utc = Utc::now();
                            runtime.dependencies().journal.advance(&journal).await?;
                        }
                        Ok::<(), ApplicationError>(())
                    }
                    .await;
                    if let Err(error) = mutation {
                        journal.fail_first_pending_item(error.to_string());
                        journal.status = mkvo_application::JournalStatus::Failed;
                        journal.detail = Some(format!(
                            "rename failed after {} completed move(s); batchId={batch_id}; {error}",
                            journal.step
                        ));
                        journal.updated_utc = Utc::now();
                        let _ = runtime.dependencies().journal.advance(&journal).await;
                        return Err(error);
                    }
                    journal.status = mkvo_application::JournalStatus::Completed;
                    journal.detail = Some("rename plan completed".to_owned());
                    journal.updated_utc = Utc::now();
                    runtime.dependencies().journal.advance(&journal).await?;
                    runtime
                        .append_log(
                            "Rename",
                            "Rename plan completed",
                            &format!("planId={plan_id}; fingerprint={fingerprint}"),
                        )
                        .await
                        .map_err(runtime_application_error)?;
                    Ok((plan, false))
                }
            })
            .await?;

        // The working set is what later operations run against, so it moves
        // with the files. A replay renamed nothing this time round, but the
        // paths it describes are still the ones that now exist.
        let moves: Vec<_> = plan
            .payload
            .items
            .iter()
            .filter(|item| item.can_apply())
            .map(|item| (item.source.clone(), item.target.clone()))
            .collect();
        if !moves.is_empty() {
            self.apply_renames_to_working_set(&moves).await;
        }

        Ok(rename_apply_response(&plan, replay))
    }

    pub async fn get_rename_batches(&self) -> RuntimeResult<RenameBatchListResponse> {
        let records = self.dependencies().rename_history.list_recent(100).await?;
        let native_ids: BTreeSet<_> = records.iter().map(|record| record.id.to_string()).collect();
        let mut batches: Vec<_> = records.iter().map(rename_batch_dto).collect();
        batches.extend(
            self.legacy_rename_history()
                .iter()
                .filter(|record| !native_ids.contains(&record.id))
                .filter_map(legacy_rename_batch_dto),
        );
        batches.sort_by_key(|batch| std::cmp::Reverse(batch.created_at));
        batches.truncate(100);
        Ok(RenameBatchListResponse { batches })
    }

    pub async fn preview_rename_batch_undo(
        &self,
        id: &str,
    ) -> RuntimeResult<RenameBatchUndoPreviewResponse> {
        if let Some(batch) = self
            .legacy_rename_history()
            .iter()
            .find(|batch| batch.id == id)
        {
            let skipped = batch.entries.len().max(batch.total_files);
            return Ok(RenameBatchUndoPreviewResponse {
                restorable: 0,
                skipped,
                lines: batch
                    .entries
                    .iter()
                    .map(|entry| {
                        format!(
                            "Read-only legacy history: {} -> {} cannot be safely undone because the old record has no file fingerprint",
                            entry.original_path.display(),
                            entry.renamed_path.display()
                        )
                    })
                    .collect(),
                has_skipped_files: skipped > 0,
            });
        }
        let batch = self.rename_batch(id).await?;
        let mut response = RenameBatchUndoPreviewResponse::default();
        for entry in &batch.entries {
            let current_exists = self
                .dependencies()
                .file_system
                .exists(&entry.renamed_path)
                .await?;
            let target_exists = self
                .dependencies()
                .file_system
                .exists(&entry.original_path)
                .await?;
            if current_exists && !target_exists && batch.undone_at.is_none() {
                response.restorable += 1;
                response.lines.push(format!(
                    "Restore {} -> {}",
                    entry.renamed_path.display(),
                    entry.original_path.display()
                ));
            } else {
                response.skipped += 1;
                response.lines.push(format!(
                    "Skip {}: source missing, target exists, or batch already undone",
                    entry.renamed_path.display()
                ));
            }
        }
        response.has_skipped_files = response.skipped > 0;
        Ok(response)
    }

    pub async fn undo_rename_batch(&self, id: &str) -> RuntimeResult<RenameBatchUndoResponse> {
        if self
            .legacy_rename_history()
            .iter()
            .any(|batch| batch.id == id)
        {
            return Err(RuntimeError::invalid(
                "legacy rename history is read-only and cannot be safely undone because it has no file fingerprints",
            ));
        }
        let batch = self.rename_batch(id).await?;
        let resources: Vec<_> = batch
            .entries
            .iter()
            .flat_map(|entry| {
                [
                    mkvo_domain::ResourceClaim::write(entry.renamed_path.clone()),
                    mkvo_domain::ResourceClaim::write(entry.original_path.clone()),
                ]
            })
            .collect();
        let runtime = self.clone();
        let id = id.to_owned();
        self.jobs()
            .with_resource_lease(
                &resources,
                tokio_util::sync::CancellationToken::new(),
                || {
                    async move {
                        // Re-read under the lease so concurrent undo calls cannot
                        // both pass the undone check.
                        let batch = runtime
                            .rename_batch(&id)
                            .await
                            .map_err(runtime_application_error)?;
                        if batch.undone_at.is_some() {
                            return Err(ApplicationError::Conflict(
                                "rename batch was already undone".to_owned(),
                            ));
                        }
                        let mut response = RenameBatchUndoResponse::default();
                        for entry in &batch.entries {
                            let can_restore = runtime
                                .dependencies()
                                .file_system
                                .exists(&entry.renamed_path)
                                .await?
                                && !runtime
                                    .dependencies()
                                    .file_system
                                    .exists(&entry.original_path)
                                    .await?;
                            if !can_restore {
                                response.skipped += 1;
                                response
                                    .lines
                                    .push(format!("Skipped {}", entry.renamed_path.display()));
                                continue;
                            }
                            if let Some(expected) = &entry.renamed_fingerprint {
                                runtime
                                    .revalidate_fingerprint(expected)
                                    .await
                                    .map_err(runtime_application_error)?;
                            }
                            runtime
                                .dependencies()
                                .file_system
                                .move_file(&entry.renamed_path, &entry.original_path)
                                .await?;
                            response.renamed += 1;
                            response
                                .lines
                                .push(format!("Restored {}", entry.original_path.display()));
                            response.restored.push(RenameBatchRestoreMove {
                                original_path: display_path(&entry.original_path),
                                renamed_path: display_path(&entry.renamed_path),
                                original_file_name: file_name(&entry.original_path),
                            });
                        }
                        runtime
                            .dependencies()
                            .rename_history
                            .mark_undone(batch.id, Utc::now())
                            .await?;
                        runtime
                            .append_log("Rename", "Rename batch undo completed", &id)
                            .await
                            .map_err(runtime_application_error)?;
                        Ok(response)
                    }
                },
            )
            .await
            .map_err(RuntimeError::from)
    }

    pub async fn clear_rename_batches(&self) -> RuntimeResult<RenameBatchListResponse> {
        self.dependencies().rename_history.clear().await?;
        self.get_rename_batches().await
    }

    pub(super) async fn load_rename_episodes(
        &self,
        provider: MetadataProvider,
        media_id: &str,
        language: Option<&str>,
    ) -> RuntimeResult<Vec<mkvo_domain::EpisodeMetadata>> {
        let client = self
            .provider_client(Some(provider_name(provider)), language)
            .await?;
        client
            .episodes(
                media_id,
                language,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .map_err(Into::into)
    }

    pub(super) async fn rename_batch(&self, id: &str) -> RuntimeResult<RenameBatchRecord> {
        let id = id
            .parse::<RenameBatchId>()
            .map_err(|_| RuntimeError::invalid(format!("invalid rename batch id: {id}")))?;
        self.dependencies()
            .rename_history
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("rename batch {id}")))
    }
}
