use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use mkvo_application::{
    ApplicationError, JobSpec, LibraryAuditService, PropertyEditPlanRequest, PropertyEditPlanner,
    RemuxOptions, RemuxPlanRequest, RemuxPlanner, RenamePlanRequest, RenamePlanner, TextEdit,
    ToolInvocation, TrackEditIntent, parse_episode_number,
};
use mkvo_contracts::{
    JobCompletion, JobKind, JobLogLevel, LibraryAuditResponse, LibraryAuditRow,
    LibraryAuditSummary, LogLevel, OperationLogEntry, PropEditActionRow, PropEditNoChangeRow,
    PropEditSkippedRow, PropEditTrackConfigRow, RenameApplyResponse, RenameBatchEntryDto,
    RenameBatchListResponse, RenameBatchRecordDto, RenameBatchRestoreMove,
    RenameBatchUndoPreviewResponse, RenameBatchUndoResponse, RenamePreviewRow, RenameScopeRow,
    TitleEditMode,
};
use mkvo_domain::{
    ContainerKind, ContainerMetadata, EpisodeIdentity, ExternalSubtitle, FileFingerprint,
    IdempotencyKey, MediaAttachment, MediaFile, MediaStatus, MediaTrack, MetadataProvider,
    OperationPlan, PlanId, PropertyEditPlan, PropertyMutation, ProviderMatch, RemuxMode, RemuxPlan,
    RenameBatchEntry, RenameBatchId, RenameBatchRecord, RenamePlan, StoredPlan, ToolFingerprint,
    ToolFingerprints, TrackExtraction, TrackKind, stable_fingerprint,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::compat::{
    LibraryAuditRequest, MuxPreviewRequest, MuxPreviewResponse, PropEditPreviewRequest,
    PropEditPreviewResponse, PropEditTemplateRequest, PropEditTemplateResponse, RenameApplyRequest,
    RenamePreviewRequest, RenamePreviewResponse,
};
use crate::runtime::{display_path, parse_provider, provider_name};
use crate::{MkvoRuntime, RuntimeError, RuntimeResult};

impl MkvoRuntime {
    pub async fn build_rename_preview(
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
        for file in &mut files {
            let number = parse_episode_number(&file.file_name());
            let matched = number.and_then(|number| {
                episodes.iter().find(|episode| {
                    episode.episode == number
                        && (selected_seasons.is_empty()
                            || selected_seasons.contains(&episode.season))
                })
            });
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
        let plan = RenamePlanner.build_plan(RenamePlanRequest {
            files,
            template: request
                .template
                .unwrap_or_else(|| settings.rename.template.clone()),
            provider: Some(provider),
            check_existing_files: true,
            existing_paths: self.current_existing_paths().await,
            authorized_roots: self.authorized_root_paths(),
            settings_fingerprint,
            expires_in_seconds: 900,
            idempotency_key: key.clone(),
        })?;
        self.persist_plan(&plan).await?;
        let scopes = scope_rows(&episodes);
        Ok(rename_preview_response(&plan, scopes, key))
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

    pub async fn build_mux_preview(
        &self,
        request: MuxPreviewRequest,
    ) -> RuntimeResult<MuxPreviewResponse> {
        let plan = self.plan_mux(&request).await?;
        self.persist_plan(&plan).await?;
        Ok(mux_preview_response(&plan))
    }

    pub async fn load_propedit_template(
        &self,
        request: PropEditTemplateRequest,
    ) -> RuntimeResult<PropEditTemplateResponse> {
        let files = self.resolve_rows(&request.files, &[]).await?;
        let template = request
            .template_path
            .as_deref()
            .and_then(|path| {
                files
                    .iter()
                    .find(|file| same_path(&file.path, Path::new(path)))
            })
            .or_else(|| files.first())
            .ok_or_else(|| RuntimeError::invalid("Select a scanned MKV template file first."))?;
        let audio_tracks = template
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio)
            .enumerate()
            .map(|(index, track)| prop_track_row(index, track))
            .collect();
        let subtitle_tracks = template
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Subtitle)
            .enumerate()
            .map(|(index, track)| prop_track_row(index, track))
            .collect();
        Ok(PropEditTemplateResponse {
            template_path: display_path(&template.path),
            template_file_name: template.file_name(),
            audio_tracks,
            subtitle_tracks,
            default_audio: selected_track_label(template, TrackKind::Audio, |track| track.default),
            forced_audio: selected_track_label(template, TrackKind::Audio, |track| track.forced),
            default_subtitle: selected_track_label(template, TrackKind::Subtitle, |track| {
                track.default
            }),
            forced_subtitle: selected_track_label(template, TrackKind::Subtitle, |track| {
                track.forced
            }),
        })
    }

    pub async fn build_propedit_preview(
        &self,
        request: PropEditPreviewRequest,
    ) -> RuntimeResult<PropEditPreviewResponse> {
        let plan = self.plan_propedit(&request).await?;
        self.persist_plan(&plan).await?;
        Ok(propedit_preview_response(&plan))
    }

    pub async fn run_library_audit(
        &self,
        request: LibraryAuditRequest,
    ) -> RuntimeResult<LibraryAuditResponse> {
        let files = self.resolve_rows(&request.files, &[]).await?;
        let audit = LibraryAuditService.build(&self.config().media_root, &files, &[]);
        let items = audit
            .groups
            .iter()
            .map(|group| LibraryAuditRow {
                folder_path: display_path(&self.config().media_root.join(&group.relative_folder)),
                folder_name: if group.season_folder.is_empty() {
                    group.show_name.clone()
                } else {
                    format!("{} / {}", group.show_name, group.season_folder)
                },
                file_count: group.all_file_paths.len(),
                standard_video: group.standard.video.clone(),
                standard_audio: group.standard.audio.clone(),
                standard_subtitles: group.standard.subtitles.clone(),
                template_file_path: group
                    .template_file_path
                    .as_deref()
                    .map_or_else(String::new, display_path),
                template_file_name: group
                    .template_file_path
                    .as_deref()
                    .map_or_else(String::new, file_name),
                has_issues: group.has_issues(),
                issue_summary: if group.issues.is_empty() {
                    "Standard".to_owned()
                } else {
                    format!("{} issue(s)", group.issues.len())
                },
                issues: group
                    .issues
                    .iter()
                    .map(|issue| issue.message.clone())
                    .collect(),
                issue_file_paths: group
                    .issue_file_paths
                    .iter()
                    .map(|path| display_path(path))
                    .collect(),
                all_file_paths: group
                    .all_file_paths
                    .iter()
                    .map(|path| display_path(path))
                    .collect(),
            })
            .collect();
        Ok(LibraryAuditResponse {
            summary: LibraryAuditSummary {
                groups: audit.summary.season_folders,
                files: audit.summary.files,
                issue_groups: audit.summary.issue_groups,
                standard_groups: audit
                    .summary
                    .season_folders
                    .saturating_sub(audit.summary.issue_groups),
            },
            items,
        })
    }

    async fn plan_mux(&self, request: &MuxPreviewRequest) -> RuntimeResult<RemuxPlan> {
        let files = self
            .resolve_rows(&request.files, &request.selected_paths)
            .await?;
        let mode = if request.convert_mp4_to_mkv {
            RemuxMode::ConvertToMkv
        } else if request.extract_subtitles {
            RemuxMode::ExtractSubtitles
        } else if request.mux_matching_external_subtitles {
            RemuxMode::MuxSubtitles
        } else {
            RemuxMode::Remux
        };
        let mut existing_paths = self.current_existing_paths().await;
        let mut external_subtitles = if mode == RemuxMode::MuxSubtitles {
            discover_external_subtitles(
                &files,
                &request.external_subtitle_formats,
                &request.external_subtitle_language,
                request.external_subtitle_track_name.as_deref(),
            )
            .await
        } else {
            BTreeMap::new()
        };
        if request.skip_mux_if_subtitle_already_exists {
            for file in &files {
                if let Some(subtitles) = external_subtitles.get_mut(&file.path) {
                    subtitles.retain(|subtitle| {
                        !file.tracks.iter().any(|track| {
                            track.kind == TrackKind::Subtitle
                                && track
                                    .language_or_undetermined()
                                    .eq_ignore_ascii_case(&subtitle.language)
                                && subtitle.name.as_deref().is_none_or(|name| {
                                    track
                                        .name
                                        .as_deref()
                                        .is_some_and(|existing| existing.eq_ignore_ascii_case(name))
                                })
                        })
                    });
                }
                if external_subtitles
                    .get(&file.path)
                    .is_some_and(Vec::is_empty)
                {
                    external_subtitles.remove(&file.path);
                }
            }
        }
        let extractions = if mode == RemuxMode::ExtractSubtitles {
            build_extractions(
                &files,
                &request.extract_subtitle_languages,
                request.extract_overwrite_existing_files,
            )
        } else {
            BTreeMap::new()
        };
        existing_paths.extend(
            external_subtitles
                .values()
                .flatten()
                .map(|subtitle| subtitle.path.clone()),
        );
        if !request.extract_overwrite_existing_files {
            existing_paths.extend(
                extractions
                    .values()
                    .flatten()
                    .filter(|track| track.output.exists())
                    .map(|track| track.output.clone()),
            );
        }
        let settings = self.dependencies().settings.load().await?.0;
        let settings_fingerprint = stable_fingerprint(&settings)
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        let remove_track_ids = if request.remove_unwanted_track_ids {
            split_u64s(&request.remove_track_ids_text)
        } else {
            BTreeSet::new()
        };
        let key = request
            .idempotency_key
            .clone()
            .unwrap_or_else(IdempotencyKey::generate);
        let base = RemuxPlanner
            .build_plan(RemuxPlanRequest {
                mode,
                files,
                options: RemuxOptions {
                    filter_audio_languages: request.remove_unwanted_audio_languages,
                    keep_audio_languages: split_strings(&request.keep_audio_languages),
                    filter_subtitle_languages: request.remove_unwanted_subtitle_languages,
                    keep_subtitle_languages: split_strings(&request.keep_subtitle_languages),
                    remove_track_ids,
                    preserve_chapters: request.preserve_chapters,
                    preserve_attachments: request.preserve_attachments,
                    delete_source_after_success: request.delete_mp4_after_convert,
                    delete_external_subtitles_after_success: !request
                        .preserve_external_subtitle_files,
                },
                external_subtitles,
                extractions,
                existing_paths,
                authorized_roots: self.authorized_root_paths(),
                settings_fingerprint,
                tool_fingerprints: self.tool_fingerprints().await?,
                expires_in_seconds: 900,
                idempotency_key: key,
            })
            .map_err(RuntimeError::from)?;
        let mut context = base.context;
        let mut payload = base.payload;
        for item in &mut payload.items {
            if item.mode == RemuxMode::ExtractSubtitles {
                continue;
            }
            let old = item.temporary_output.clone();
            let stem = item
                .source
                .file_stem()
                .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
            item.temporary_output = item
                .source
                .with_file_name(format!("{stem}.mkvo-remux.tmp.mkv"));
            for resource in &mut context.resources {
                if same_path(&resource.path, &old) {
                    resource.path.clone_from(&item.temporary_output);
                }
            }
        }
        context.attributes.insert(
            "preserveChapters".to_owned(),
            request.preserve_chapters.to_string(),
        );
        context.attributes.insert(
            "preserveAttachments".to_owned(),
            request.preserve_attachments.to_string(),
        );
        context.attributes.insert(
            "extractOverwriteExistingFiles".to_owned(),
            request.extract_overwrite_existing_files.to_string(),
        );
        context.attributes.insert(
            "skipMuxIfSubtitleAlreadyExists".to_owned(),
            request.skip_mux_if_subtitle_already_exists.to_string(),
        );
        context.attributes.insert(
            "externalSubtitleTrackName".to_owned(),
            request
                .external_subtitle_track_name
                .clone()
                .unwrap_or_default(),
        );
        let now = Utc::now();
        OperationPlan::new(
            base.metadata.kind,
            request,
            payload,
            context,
            now,
            now + Duration::minutes(15),
            base.metadata.idempotency_key,
        )
        .map_err(|error| RuntimeError::internal(error.to_string()))
    }

    async fn plan_propedit(
        &self,
        request: &PropEditPreviewRequest,
    ) -> RuntimeResult<PropertyEditPlan> {
        let files = self
            .resolve_rows(&request.files, &request.selected_paths)
            .await?;
        let settings = self.dependencies().settings.load().await?.0;
        let settings_fingerprint = stable_fingerprint(&settings)
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        let key = request
            .idempotency_key
            .clone()
            .unwrap_or_else(IdempotencyKey::generate);
        PropertyEditPlanner
            .build_plan(PropertyEditPlanRequest {
                files,
                container_title: title_edit(
                    request.container_title_mode,
                    &request.custom_container_title,
                ),
                video_track_name: title_edit(request.video_title_mode, &request.custom_video_title),
                track_edits: build_track_edits(request),
                authorized_roots: self.authorized_root_paths(),
                settings_fingerprint,
                tool_fingerprints: self.tool_fingerprints().await?,
                expires_in_seconds: 900,
                idempotency_key: key,
            })
            .map_err(Into::into)
    }

    async fn resolve_rows(
        &self,
        rows: &[mkvo_contracts::MediaFileRow],
        selected_paths: &[String],
    ) -> RuntimeResult<Vec<MediaFile>> {
        let current = self.current_domain_files().await;
        let selected: BTreeSet<_> = selected_paths.iter().map(|path| path_key(path)).collect();
        let source_rows: Vec<_> = rows
            .iter()
            .filter(|row| selected.is_empty() || selected.contains(&path_key(&row.path)))
            .collect();
        let mut files = Vec::with_capacity(source_rows.len());
        for row in source_rows {
            if let Some(file) = current
                .iter()
                .find(|file| same_path(&file.path, Path::new(&row.path)))
            {
                files.push(file.clone());
                continue;
            }
            let path = PathBuf::from(&row.path);
            let fingerprint = self.dependencies().file_system.fingerprint(&path).await?;
            files.push(media_from_row(row, fingerprint));
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files.dedup_by(|left, right| same_path(&left.path, &right.path));
        Ok(files)
    }

    async fn persist_plan<T: Serialize>(&self, plan: &OperationPlan<T>) -> RuntimeResult<()> {
        let stored = StoredPlan::try_from(plan)?;
        self.dependencies().plans.save(&stored).await?;
        Ok(())
    }

    async fn load_valid_plan<T: DeserializeOwned + Serialize>(
        &self,
        id: PlanId,
        fingerprint: &str,
        key: &IdempotencyKey,
    ) -> RuntimeResult<OperationPlan<T>> {
        let plan = self.load_referenced_plan(id, fingerprint, key).await?;
        let settings = self.dependencies().settings.load().await?.0;
        let actual_settings = stable_fingerprint(&settings)
            .map_err(|error| RuntimeError::internal(error.to_string()))?;
        if plan.context.settings_fingerprint != actual_settings {
            return Err(RuntimeError::new(
                mkvo_contracts::ApiErrorCode::PlanStale,
                "settings changed after the preview was created",
            ));
        }
        for (name, expected) in &plan.context.tool_fingerprints {
            let actual = self.dependencies().tools.status(name).await?;
            if !actual.available
                || !same_path(&expected.executable, Path::new(&actual.resolved_path))
                || (!expected.version.is_empty() && expected.version != actual.version)
            {
                return Err(RuntimeError::new(
                    mkvo_contracts::ApiErrorCode::PlanStale,
                    format!("tool `{name}` changed or became unavailable after preview"),
                ));
            }
        }
        for expected in &plan.context.input_fingerprints {
            self.revalidate_fingerprint(expected).await?;
        }
        Ok(plan)
    }

    async fn load_referenced_plan<T: DeserializeOwned + Serialize>(
        &self,
        id: PlanId,
        fingerprint: &str,
        key: &IdempotencyKey,
    ) -> RuntimeResult<OperationPlan<T>> {
        let stored = self
            .dependencies()
            .plans
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("operation plan {id}")))?;
        if stored.metadata.fingerprint != fingerprint {
            return Err(RuntimeError::new(
                mkvo_contracts::ApiErrorCode::PlanTampered,
                "plan fingerprint does not match the stored immutable plan",
            ));
        }
        if &stored.metadata.idempotency_key != key {
            return Err(RuntimeError::new(
                mkvo_contracts::ApiErrorCode::PlanTampered,
                "idempotency key does not belong to this plan",
            ));
        }
        let plan: OperationPlan<T> = serde_json::from_value(serde_json::to_value(stored)?)?;
        plan.validate_integrity(Utc::now())
            .map_err(mkvo_application::ApplicationError::from)?;
        Ok(plan)
    }

    async fn revalidate_fingerprint(&self, expected: &FileFingerprint) -> RuntimeResult<()> {
        let actual = self
            .dependencies()
            .file_system
            .fingerprint(&expected.path)
            .await?;
        if !expected.matches(&actual) {
            return Err(RuntimeError::new(
                mkvo_contracts::ApiErrorCode::PlanStale,
                format!("input changed after preview: {}", expected.path.display()),
            ));
        }
        Ok(())
    }

    async fn current_existing_paths(&self) -> BTreeSet<PathBuf> {
        self.current_domain_files()
            .await
            .into_iter()
            .map(|file| file.path)
            .collect()
    }

    /// Roots as the authorization service actually holds them.
    ///
    /// These must be the canonical paths, not the raw configured strings: file
    /// paths reaching a planner have been canonicalized, and on Windows that
    /// adds the extended-length `\\?\` prefix. Comparing a canonical file
    /// against a raw root makes every input look unauthorized. Runtime grants
    /// added after startup only exist on the service, so reading the config
    /// alone would also miss them.
    fn authorized_root_paths(&self) -> Vec<PathBuf> {
        if let Some(roots) = self.dependencies().authorized_roots.as_ref() {
            let roots: Vec<PathBuf> = roots
                .roots()
                .into_iter()
                .map(|root| root.path().to_path_buf())
                .collect();
            if !roots.is_empty() {
                return roots;
            }
        }
        self.config()
            .authorized_roots
            .iter()
            .map(|(path, _)| path.clone())
            .collect()
    }

    async fn tool_fingerprints(&self) -> RuntimeResult<ToolFingerprints> {
        let statuses = self.dependencies().tools.all_statuses().await?;
        Ok(statuses
            .into_iter()
            .filter(|status| status.available)
            .map(|status| {
                (
                    status.name.clone(),
                    ToolFingerprint {
                        name: status.name,
                        executable: PathBuf::from(status.resolved_path),
                        version: status.version,
                    },
                )
            })
            .collect())
    }

    async fn load_rename_episodes(
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

    async fn rename_batch(&self, id: &str) -> RuntimeResult<RenameBatchRecord> {
        let id = id
            .parse::<RenameBatchId>()
            .map_err(|_| RuntimeError::invalid(format!("invalid rename batch id: {id}")))?;
        self.dependencies()
            .rename_history
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("rename batch {id}")))
    }

    async fn append_log(&self, area: &str, message: &str, detail: &str) -> RuntimeResult<()> {
        self.dependencies()
            .logs
            .append(&OperationLogEntry {
                timestamp_utc: Utc::now(),
                correlation_id: mkvo_domain::CorrelationId::new(),
                area: area.to_owned(),
                level: LogLevel::Information,
                message: message.to_owned(),
                detail: detail.to_owned(),
            })
            .await?;
        Ok(())
    }
}

impl MkvoRuntime {
    pub async fn start_mux_apply(
        &self,
        request: MuxPreviewRequest,
    ) -> RuntimeResult<crate::compat::OperationJobResponse> {
        let (plan_id, fingerprint, key) = require_plan_fields(
            request.plan_id,
            request.plan_fingerprint,
            request.idempotency_key,
        )?;
        let expected_request = self
            .validate_plan_reference(plan_id, &fingerprint, &key)
            .await?;
        if let Some(existing) = self.dependencies().jobs.find_by_idempotency(&key).await? {
            validate_idempotent_job(&existing, plan_id, &expected_request)?;
            return Ok(crate::compat::OperationJobResponse::from_snapshot(
                &existing,
            ));
        }
        let plan: RemuxPlan = self.load_valid_plan(plan_id, &fingerprint, &key).await?;
        let total = u64::try_from(plan.payload.runnable_count()).unwrap_or(u64::MAX);
        let resources = plan.context.resources.clone();
        let runtime = self.clone();
        let fingerprint_for_task = fingerprint.clone();
        let key_for_task = key.clone();
        let accepted = self
            .jobs()
            .start(
                JobSpec {
                    kind: JobKind::from(plan.metadata.kind),
                    idempotency_key: key,
                    request_fingerprint: plan.metadata.request_fingerprint.clone(),
                    plan_id: Some(plan_id),
                    total,
                    resources,
                },
                move |context| async move {
                    let plan: RemuxPlan = runtime
                        .load_valid_plan(plan_id, &fingerprint_for_task, &key_for_task)
                        .await
                        .map_err(runtime_application_error)?;
                    runtime.execute_remux_plan(&plan, &context).await
                },
            )
            .await?;
        let snapshot = self
            .jobs()
            .get(accepted.id)
            .await?
            .ok_or_else(|| RuntimeError::internal("new mux job was not persisted"))?;
        Ok(crate::compat::OperationJobResponse::from_snapshot(
            &snapshot,
        ))
    }

    pub async fn start_propedit_apply(
        &self,
        request: PropEditPreviewRequest,
    ) -> RuntimeResult<crate::compat::OperationJobResponse> {
        let (plan_id, fingerprint, key) = require_plan_fields(
            request.plan_id,
            request.plan_fingerprint,
            request.idempotency_key,
        )?;
        let expected_request = self
            .validate_plan_reference(plan_id, &fingerprint, &key)
            .await?;
        if let Some(existing) = self.dependencies().jobs.find_by_idempotency(&key).await? {
            validate_idempotent_job(&existing, plan_id, &expected_request)?;
            return Ok(crate::compat::OperationJobResponse::from_snapshot(
                &existing,
            ));
        }
        let plan: PropertyEditPlan = self.load_valid_plan(plan_id, &fingerprint, &key).await?;
        let total = u64::try_from(
            plan.payload
                .items
                .iter()
                .filter(|item| item.can_apply())
                .count(),
        )
        .unwrap_or(u64::MAX);
        let resources = plan.context.resources.clone();
        let runtime = self.clone();
        let fingerprint_for_task = fingerprint.clone();
        let key_for_task = key.clone();
        let accepted = self
            .jobs()
            .start(
                JobSpec {
                    kind: JobKind::PropertyEdit,
                    idempotency_key: key,
                    request_fingerprint: plan.metadata.request_fingerprint.clone(),
                    plan_id: Some(plan_id),
                    total,
                    resources,
                },
                move |context| async move {
                    let plan: PropertyEditPlan = runtime
                        .load_valid_plan(plan_id, &fingerprint_for_task, &key_for_task)
                        .await
                        .map_err(runtime_application_error)?;
                    runtime.execute_propedit_plan(&plan, &context).await
                },
            )
            .await?;
        let snapshot = self
            .jobs()
            .get(accepted.id)
            .await?
            .ok_or_else(|| RuntimeError::internal("new propedit job was not persisted"))?;
        Ok(crate::compat::OperationJobResponse::from_snapshot(
            &snapshot,
        ))
    }

    async fn validate_plan_reference(
        &self,
        id: PlanId,
        fingerprint: &str,
        key: &IdempotencyKey,
    ) -> RuntimeResult<String> {
        let stored = self
            .dependencies()
            .plans
            .get(id)
            .await?
            .ok_or_else(|| RuntimeError::not_found(format!("operation plan {id}")))?;
        if stored.metadata.fingerprint != fingerprint || &stored.metadata.idempotency_key != key {
            return Err(RuntimeError::new(
                mkvo_contracts::ApiErrorCode::PlanTampered,
                "plan reference, fingerprint, and idempotency key must match",
            ));
        }
        Ok(stored.metadata.request_fingerprint)
    }

    async fn execute_remux_plan(
        &self,
        plan: &RemuxPlan,
        context: &mkvo_application::JobContext,
    ) -> Result<JobCompletion, ApplicationError> {
        let mut journal = self
            .begin_journal(
                plan.metadata.id,
                &plan.metadata.idempotency_key,
                &plan.context.resources,
            )
            .await?;
        let runnable: Vec<_> = plan
            .payload
            .items
            .iter()
            .filter(|item| item.can_apply())
            .collect();
        let total = u64::try_from(runnable.len()).unwrap_or(u64::MAX);
        let result = async {
            for (index, item) in runnable.iter().enumerate() {
                context.ensure_not_canceled()?;
                self.revalidate_fingerprint(&item.source_fingerprint)
                    .await
                    .map_err(runtime_application_error)?;
                context
                    .progress(
                        u64::try_from(index).unwrap_or(u64::MAX),
                        total,
                        item.source.to_string_lossy(),
                        0,
                    )
                    .await?;
                let invocation = self
                    .remux_invocation(item, &plan.context.attributes)
                    .await?;
                context
                    .log(
                        JobLogLevel::Information,
                        format!("{} {}", remux_mode_label(item.mode), item.source.display()),
                    )
                    .await?;
                let execution = self
                    .dependencies()
                    .tool_executor
                    .execute(&invocation, context.cancellation_token())
                    .await?;
                if execution.exit_code != Some(0) {
                    return Err(ApplicationError::Internal(format!(
                        "{} exited with {:?}: {}",
                        invocation.tool,
                        execution.exit_code,
                        execution.stderr.trim()
                    )));
                }
                for expected in &invocation.expected_outputs {
                    if !execution
                        .validated_outputs
                        .iter()
                        .any(|path| same_path(path, expected))
                    {
                        return Err(ApplicationError::Internal(format!(
                            "{} did not create expected output {}",
                            invocation.tool,
                            expected.display()
                        )));
                    }
                }
                if item.mode != RemuxMode::ExtractSubtitles {
                    self.promote_remux_output(item).await?;
                } else {
                    self.promote_extractions(item, &plan.context.attributes)
                        .await?;
                }
                if item.delete_external_subtitles_after_success {
                    for subtitle in &item.external_subtitles {
                        self.dependencies()
                            .file_system
                            .remove_file(&subtitle.path)
                            .await?;
                    }
                }
                journal.step = journal.step.saturating_add(1);
                journal.updated_utc = Utc::now();
                self.dependencies().journal.advance(&journal).await?;
                context.record_completed().await?;
            }
            Ok::<(), ApplicationError>(())
        }
        .await;
        if let Err(error) = result {
            journal.status = mkvo_application::JournalStatus::Failed;
            journal.detail = Some(error.to_string());
            journal.updated_utc = Utc::now();
            let _ = self.dependencies().journal.advance(&journal).await;
            return Err(error);
        }
        journal.status = mkvo_application::JournalStatus::Completed;
        journal.detail = Some("mux/remux plan completed".to_owned());
        journal.updated_utc = Utc::now();
        self.dependencies().journal.advance(&journal).await?;
        let mut response = mux_preview_response(plan);
        response.status = "Mux/remux operation completed".to_owned();
        self.append_log(
            "Mux",
            "Mux/remux plan completed",
            &plan.metadata.id.to_string(),
        )
        .await
        .map_err(runtime_application_error)?;
        Ok(JobCompletion {
            result: Some(serde_json::to_value(response).map_err(|error| {
                ApplicationError::Internal(format!("mux result serialization failed: {error}"))
            })?),
            message: Some("Mux/remux operation completed".to_owned()),
        })
    }

    async fn execute_propedit_plan(
        &self,
        plan: &PropertyEditPlan,
        context: &mkvo_application::JobContext,
    ) -> Result<JobCompletion, ApplicationError> {
        let mut journal = self
            .begin_journal(
                plan.metadata.id,
                &plan.metadata.idempotency_key,
                &plan.context.resources,
            )
            .await?;
        let runnable: Vec<_> = plan
            .payload
            .items
            .iter()
            .filter(|item| item.can_apply())
            .collect();
        let total = u64::try_from(runnable.len()).unwrap_or(u64::MAX);
        let result = async {
            for (index, item) in runnable.iter().enumerate() {
                context.ensure_not_canceled()?;
                self.revalidate_fingerprint(&item.source_fingerprint)
                    .await
                    .map_err(runtime_application_error)?;
                context
                    .progress(
                        u64::try_from(index).unwrap_or(u64::MAX),
                        total,
                        item.path.to_string_lossy(),
                        0,
                    )
                    .await?;
                let invocation = self.propedit_invocation(item).await?;
                let execution = self
                    .dependencies()
                    .tool_executor
                    .execute(&invocation, context.cancellation_token())
                    .await?;
                if execution.exit_code != Some(0) {
                    return Err(ApplicationError::Internal(format!(
                        "mkvpropedit exited with {:?}: {}",
                        execution.exit_code,
                        execution.stderr.trim()
                    )));
                }
                journal.step = journal.step.saturating_add(1);
                journal.updated_utc = Utc::now();
                self.dependencies().journal.advance(&journal).await?;
                context.record_completed().await?;
            }
            Ok::<(), ApplicationError>(())
        }
        .await;
        if let Err(error) = result {
            journal.status = mkvo_application::JournalStatus::Failed;
            journal.detail = Some(error.to_string());
            journal.updated_utc = Utc::now();
            let _ = self.dependencies().journal.advance(&journal).await;
            return Err(error);
        }
        journal.status = mkvo_application::JournalStatus::Completed;
        journal.detail = Some("property-edit plan completed".to_owned());
        journal.updated_utc = Utc::now();
        self.dependencies().journal.advance(&journal).await?;
        let mut response = propedit_preview_response(plan);
        response.status = "Track-properties operation completed".to_owned();
        self.append_log(
            "Properties",
            "Property-edit plan completed",
            &plan.metadata.id.to_string(),
        )
        .await
        .map_err(runtime_application_error)?;
        Ok(JobCompletion {
            result: Some(serde_json::to_value(response).map_err(|error| {
                ApplicationError::Internal(format!("propedit result serialization failed: {error}"))
            })?),
            message: Some("Track-properties operation completed".to_owned()),
        })
    }

    async fn begin_journal(
        &self,
        plan_id: PlanId,
        key: &IdempotencyKey,
        resources: &[mkvo_domain::ResourceClaim],
    ) -> Result<mkvo_application::JournalRecord, ApplicationError> {
        let mut record = mkvo_application::JournalRecord {
            idempotency_key: key.clone(),
            plan_id,
            step: 0,
            status: mkvo_application::JournalStatus::Prepared,
            resources: resources.to_vec(),
            detail: None,
            updated_utc: Utc::now(),
        };
        self.dependencies().journal.begin(&record).await?;
        record.status = mkvo_application::JournalStatus::Running;
        self.dependencies().journal.advance(&record).await?;
        Ok(record)
    }

    async fn remux_invocation(
        &self,
        item: &mkvo_domain::RemuxPlanItem,
        attributes: &BTreeMap<String, String>,
    ) -> Result<ToolInvocation, ApplicationError> {
        let tool = remux_tool_name(item.mode);
        let executable = self.tool_path(tool).await?;
        if item.mode == RemuxMode::ExtractSubtitles {
            let mut arguments = vec![
                "tracks".to_owned(),
                item.source.to_string_lossy().into_owned(),
            ];
            arguments.extend(item.extract_tracks.iter().map(|track| {
                format!(
                    "{}:{}",
                    track.track_id,
                    extraction_temp_path(&track.output, track.track_id).to_string_lossy()
                )
            }));
            return Ok(ToolInvocation {
                tool: tool.to_owned(),
                executable,
                arguments,
                working_directory: item.source.parent().map(Path::to_path_buf),
                expected_outputs: item
                    .extract_tracks
                    .iter()
                    .map(|track| extraction_temp_path(&track.output, track.track_id))
                    .collect(),
            });
        }
        let mut arguments = vec![
            "-o".to_owned(),
            item.temporary_output.to_string_lossy().into_owned(),
        ];
        if attributes
            .get("preserveChapters")
            .is_some_and(|value| value == "false")
        {
            arguments.push("--no-chapters".to_owned());
        }
        if attributes
            .get("preserveAttachments")
            .is_some_and(|value| value == "false")
        {
            arguments.push("--no-attachments".to_owned());
        }
        if let Some(source) = self
            .current_domain_files()
            .await
            .into_iter()
            .find(|file| same_path(&file.path, &item.source))
        {
            append_track_selection(&mut arguments, &source, &item.selected_track_ids);
        }
        arguments.push(item.source.to_string_lossy().into_owned());
        for subtitle in &item.external_subtitles {
            if !subtitle.language.trim().is_empty() {
                arguments.push("--language".to_owned());
                arguments.push(format!("0:{}", subtitle.language.trim()));
            }
            if subtitle.forced {
                arguments.push("--forced-display-flag".to_owned());
                arguments.push("0:yes".to_owned());
            }
            if let Some(name) = subtitle
                .name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
            {
                arguments.push("--track-name".to_owned());
                arguments.push(format!("0:{}", name.trim()));
            }
            arguments.push(subtitle.path.to_string_lossy().into_owned());
        }
        Ok(ToolInvocation {
            tool: tool.to_owned(),
            executable,
            arguments,
            working_directory: item.source.parent().map(Path::to_path_buf),
            expected_outputs: vec![item.temporary_output.clone()],
        })
    }

    async fn propedit_invocation(
        &self,
        item: &mkvo_domain::PropertyEditPlanItem,
    ) -> Result<ToolInvocation, ApplicationError> {
        let mut arguments = vec![item.path.to_string_lossy().into_owned()];
        for mutation in &item.mutations {
            append_propedit_arguments(&mut arguments, mutation);
        }
        Ok(ToolInvocation {
            tool: "mkvpropedit".to_owned(),
            executable: self.tool_path("mkvpropedit").await?,
            arguments,
            working_directory: item.path.parent().map(Path::to_path_buf),
            expected_outputs: Vec::new(),
        })
    }

    async fn tool_path(&self, name: &str) -> Result<PathBuf, ApplicationError> {
        let status = self.dependencies().tools.status(name).await?;
        if !status.available || status.resolved_path.trim().is_empty() {
            return Err(ApplicationError::InvalidRequest(format!(
                "required tool `{name}` is unavailable"
            )));
        }
        Ok(PathBuf::from(status.resolved_path))
    }

    async fn promote_remux_output(
        &self,
        item: &mkvo_domain::RemuxPlanItem,
    ) -> Result<(), ApplicationError> {
        if same_path(&item.source, &item.final_output) {
            let backup = backup_path(&item.source);
            if self.dependencies().file_system.exists(&backup).await? {
                return Err(ApplicationError::Conflict(format!(
                    "stale remux backup exists: {}",
                    backup.display()
                )));
            }
            self.dependencies()
                .file_system
                .move_file(&item.source, &backup)
                .await?;
            if let Err(error) = self
                .dependencies()
                .file_system
                .move_file(&item.temporary_output, &item.final_output)
                .await
            {
                let _ = self
                    .dependencies()
                    .file_system
                    .move_file(&backup, &item.source)
                    .await;
                return Err(error.into());
            }
            self.dependencies().file_system.remove_file(&backup).await?;
        } else {
            self.dependencies()
                .file_system
                .move_file(&item.temporary_output, &item.final_output)
                .await?;
            if item.delete_source_after_success {
                self.dependencies()
                    .file_system
                    .remove_file(&item.source)
                    .await?;
            }
        }
        Ok(())
    }

    async fn promote_extractions(
        &self,
        item: &mkvo_domain::RemuxPlanItem,
        attributes: &BTreeMap<String, String>,
    ) -> Result<(), ApplicationError> {
        let overwrite = attributes
            .get("extractOverwriteExistingFiles")
            .is_some_and(|value| value == "true");
        for track in &item.extract_tracks {
            let temporary = extraction_temp_path(&track.output, track.track_id);
            if self
                .dependencies()
                .file_system
                .exists(&track.output)
                .await?
            {
                if !overwrite {
                    return Err(ApplicationError::Conflict(format!(
                        "subtitle output already exists: {}",
                        track.output.display()
                    )));
                }
                let backup = backup_path(&track.output);
                if self.dependencies().file_system.exists(&backup).await? {
                    return Err(ApplicationError::Conflict(format!(
                        "stale extraction backup exists: {}",
                        backup.display()
                    )));
                }
                self.dependencies()
                    .file_system
                    .move_file(&track.output, &backup)
                    .await?;
                if let Err(error) = self
                    .dependencies()
                    .file_system
                    .move_file(&temporary, &track.output)
                    .await
                {
                    let _ = self
                        .dependencies()
                        .file_system
                        .move_file(&backup, &track.output)
                        .await;
                    return Err(error.into());
                }
                self.dependencies().file_system.remove_file(&backup).await?;
            } else {
                self.dependencies()
                    .file_system
                    .move_file(&temporary, &track.output)
                    .await?;
            }
        }
        Ok(())
    }
}

fn require_plan_fields(
    plan_id: Option<PlanId>,
    fingerprint: Option<String>,
    key: Option<IdempotencyKey>,
) -> RuntimeResult<(PlanId, String, IdempotencyKey)> {
    let plan_id = plan_id
        .ok_or_else(|| RuntimeError::invalid("apply requires planId from a successful preview"))?;
    let fingerprint = fingerprint
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            RuntimeError::invalid("apply requires planFingerprint from a successful preview")
        })?;
    let key = key.ok_or_else(|| {
        RuntimeError::invalid("apply requires idempotencyKey from a successful preview")
    })?;
    Ok((plan_id, fingerprint, key))
}

fn runtime_application_error(error: RuntimeError) -> ApplicationError {
    match error.code {
        mkvo_contracts::ApiErrorCode::InvalidRequest => {
            ApplicationError::InvalidRequest(error.message)
        }
        mkvo_contracts::ApiErrorCode::UnauthorizedPath => {
            ApplicationError::UnauthorizedPath(PathBuf::from(error.message))
        }
        mkvo_contracts::ApiErrorCode::NotFound => ApplicationError::NotFound(error.message),
        mkvo_contracts::ApiErrorCode::Conflict
        | mkvo_contracts::ApiErrorCode::PlanExpired
        | mkvo_contracts::ApiErrorCode::PlanStale
        | mkvo_contracts::ApiErrorCode::PlanTampered => ApplicationError::Conflict(error.message),
        mkvo_contracts::ApiErrorCode::JobCanceled => ApplicationError::Canceled,
        _ => ApplicationError::Internal(error.message),
    }
}

fn validate_idempotent_job(
    existing: &mkvo_contracts::JobSnapshot,
    plan_id: PlanId,
    request_fingerprint: &str,
) -> RuntimeResult<()> {
    if existing.plan_id != Some(plan_id) || existing.request_fingerprint != request_fingerprint {
        return Err(RuntimeError::new(
            mkvo_contracts::ApiErrorCode::Conflict,
            "idempotency key was already used for a different plan or request",
        ));
    }
    Ok(())
}

fn append_propedit_arguments(arguments: &mut Vec<String>, mutation: &PropertyMutation) {
    match mutation {
        PropertyMutation::SetContainerTitle { value } => {
            arguments.extend(["--edit".to_owned(), "info".to_owned()]);
            arguments.extend(["--set".to_owned(), format!("title={value}")]);
        }
        PropertyMutation::DeleteContainerTitle => {
            arguments.extend(["--edit".to_owned(), "info".to_owned()]);
            arguments.extend(["--delete".to_owned(), "title".to_owned()]);
        }
        PropertyMutation::SetTrackName { selector, value } => {
            arguments.extend(["--edit".to_owned(), selector.mkvpropedit_value()]);
            arguments.extend(["--set".to_owned(), format!("name={value}")]);
        }
        PropertyMutation::DeleteTrackName { selector } => {
            arguments.extend(["--edit".to_owned(), selector.mkvpropedit_value()]);
            arguments.extend(["--delete".to_owned(), "name".to_owned()]);
        }
        PropertyMutation::SetTrackLanguage { selector, language } => {
            arguments.extend(["--edit".to_owned(), selector.mkvpropedit_value()]);
            arguments.extend(["--set".to_owned(), format!("language={language}")]);
        }
        PropertyMutation::SetDefaultFlag { selector, value } => {
            arguments.extend(["--edit".to_owned(), selector.mkvpropedit_value()]);
            arguments.extend([
                "--set".to_owned(),
                format!("flag-default={}", u8::from(*value)),
            ]);
        }
        PropertyMutation::SetForcedFlag { selector, value } => {
            arguments.extend(["--edit".to_owned(), selector.mkvpropedit_value()]);
            arguments.extend([
                "--set".to_owned(),
                format!("flag-forced={}", u8::from(*value)),
            ]);
        }
    }
}

fn backup_path(source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    source.with_file_name(format!(".{name}.mkvo-backup"))
}

fn extraction_temp_path(output: &Path, track_id: u64) -> PathBuf {
    let name = output
        .file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    output.with_file_name(format!(".{name}.mkvo-extract-{track_id}.tmp"))
}

fn append_track_selection(arguments: &mut Vec<String>, source: &MediaFile, selected: &[u64]) {
    let selected: BTreeSet<_> = selected.iter().copied().collect();
    for (kind, option, empty_option) in [
        (TrackKind::Video, "--video-tracks", "--no-video"),
        (TrackKind::Audio, "--audio-tracks", "--no-audio"),
        (TrackKind::Subtitle, "--subtitle-tracks", "--no-subtitles"),
    ] {
        let all_of_kind: Vec<_> = source
            .tracks
            .iter()
            .filter(|track| track.kind == kind)
            .collect();
        if all_of_kind.is_empty() {
            continue;
        }
        let selected_of_kind: Vec<_> = all_of_kind
            .iter()
            .filter(|track| selected.contains(&track.mkvmerge_id))
            .map(|track| track.mkvmerge_id.to_string())
            .collect();
        if selected_of_kind.is_empty() {
            arguments.push(empty_option.to_owned());
        } else if selected_of_kind.len() != all_of_kind.len() {
            arguments.push(option.to_owned());
            arguments.push(selected_of_kind.join(","));
        }
    }
}

fn rename_preview_response(
    plan: &RenamePlan,
    scopes: Vec<RenameScopeRow>,
    key: IdempotencyKey,
) -> RenamePreviewResponse {
    let items: Vec<_> = plan
        .payload
        .items
        .iter()
        .map(|item| {
            let no_change = same_path(&item.source, &item.target);
            let status = item
                .conflicts
                .first()
                .map_or_else(|| "Ready".to_owned(), |conflict| conflict.message.clone());
            RenamePreviewRow {
                selected: item.can_apply(),
                source_path: display_path(&item.source),
                current_file_name: file_name(&item.source),
                detected: String::new(),
                episode_name: String::new(),
                new_file_name: item.new_file_name.clone(),
                confidence: if item.can_apply() {
                    "High".to_owned()
                } else {
                    String::new()
                },
                status: if no_change {
                    "No change".to_owned()
                } else {
                    status
                },
                can_apply: item.can_apply(),
            }
        })
        .collect();
    let ready = items.iter().filter(|item| item.can_apply).count();
    RenamePreviewResponse {
        summary: format!("{ready} of {} file(s) ready to rename", items.len()),
        status: format!("Rename preview ready: {ready} change(s)"),
        items,
        scopes,
        plan_id: Some(plan.metadata.id),
        plan_fingerprint: Some(plan.metadata.fingerprint.clone()),
        idempotency_key: Some(key),
    }
}

fn rename_apply_response(plan: &RenamePlan, replay: bool) -> RenameApplyResponse {
    let items: Vec<_> = plan
        .payload
        .items
        .iter()
        .map(|item| RenamePreviewRow {
            selected: item.can_apply(),
            source_path: if item.can_apply() {
                display_path(&item.target)
            } else {
                display_path(&item.source)
            },
            current_file_name: file_name(&item.source),
            detected: String::new(),
            episode_name: String::new(),
            new_file_name: item.new_file_name.clone(),
            confidence: String::new(),
            status: if item.can_apply() {
                "Renamed".to_owned()
            } else {
                "Skipped".to_owned()
            },
            can_apply: false,
        })
        .collect();
    let renamed = plan.payload.rename_count();
    let skipped = plan.payload.skip_count();
    let replay_label = if replay { " (idempotent replay)" } else { "" };
    RenameApplyResponse {
        items,
        summary: format!("{renamed} renamed, {skipped} skipped{replay_label}"),
        status: format!("Rename complete: {renamed} renamed, {skipped} skipped"),
    }
}

fn selected_seasons(keys: &[String]) -> BTreeSet<u32> {
    keys.iter()
        .filter_map(|key| key.strip_prefix("season:"))
        .filter_map(|value| value.parse().ok())
        .collect()
}

fn scope_rows(episodes: &[mkvo_domain::EpisodeMetadata]) -> Vec<RenameScopeRow> {
    let mut seasons: Vec<_> = episodes.iter().map(|episode| episode.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    let mut rows = vec![RenameScopeRow {
        key: "all".to_owned(),
        label: format!("All episodes ({})", episodes.len()),
        is_selected: true,
    }];
    rows.extend(seasons.into_iter().map(|season| RenameScopeRow {
        key: format!("season:{season}"),
        label: format!("Season {season}"),
        is_selected: false,
    }));
    rows
}

fn rename_batch_dto(record: &RenameBatchRecord) -> RenameBatchRecordDto {
    RenameBatchRecordDto {
        id: record.id.to_string(),
        created_at: record.created_at,
        undone_at: record.undone_at,
        provider: record
            .provider
            .map_or_else(String::new, |value| provider_name(value).to_owned()),
        template: record.template.clone(),
        total_files: record.entries.len(),
        entries: record
            .entries
            .iter()
            .map(|entry| RenameBatchEntryDto {
                original_path: display_path(&entry.original_path),
                renamed_path: display_path(&entry.renamed_path),
                original_file_name: file_name(&entry.original_path),
                renamed_file_name: file_name(&entry.renamed_path),
            })
            .collect(),
        is_undone: record.undone_at.is_some(),
        display_name: format!(
            "{} - {} file(s)",
            record.created_at.format("%Y-%m-%d %H:%M"),
            record.entries.len()
        ),
    }
}

fn legacy_rename_batch_dto(
    record: &mkvo_infra_sqlite::LegacyRenameBatchRecord,
) -> Option<RenameBatchRecordDto> {
    let created_at = chrono::DateTime::parse_from_rfc3339(&record.created_at)
        .ok()?
        .with_timezone(&Utc);
    let undone_at = record
        .undone_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    let entries = record
        .entries
        .iter()
        .map(|entry| RenameBatchEntryDto {
            original_path: display_path(&entry.original_path),
            renamed_path: display_path(&entry.renamed_path),
            original_file_name: file_name(&entry.original_path),
            renamed_file_name: file_name(&entry.renamed_path),
        })
        .collect();
    Some(RenameBatchRecordDto {
        id: record.id.clone(),
        created_at,
        undone_at,
        provider: record.provider.clone(),
        template: record.template.clone(),
        total_files: record.total_files.max(record.entries.len()),
        entries,
        is_undone: record.undone_at.is_some(),
        display_name: format!(
            "Legacy (read-only) • {} • {} file(s)",
            created_at.format("%Y-%m-%d %H:%M"),
            record.total_files.max(record.entries.len())
        ),
    })
}

fn mux_preview_response(plan: &RemuxPlan) -> MuxPreviewResponse {
    let actions = plan
        .payload
        .items
        .iter()
        .filter(|item| item.can_apply())
        .enumerate()
        .map(|(index, item)| mkvo_contracts::MuxActionRow {
            index,
            file_path: item.source.to_string_lossy().into_owned(),
            file_name: file_name(&item.source),
            operation: remux_mode_label(item.mode).to_owned(),
            tool_name: remux_tool_name(item.mode).to_owned(),
            description: remux_description(item),
            command: redacted_remux_command(item),
        })
        .collect::<Vec<_>>();
    let no_change_files = plan
        .payload
        .items
        .iter()
        .filter(|item| !item.can_apply())
        .map(|item| item.source.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    MuxPreviewResponse {
        summary: format!(
            "{} action(s), {} skipped/no-change file(s)",
            actions.len(),
            no_change_files.len()
        ),
        status: "Mux/remux preview ready".to_owned(),
        actions,
        no_change_files,
        plan_id: Some(plan.metadata.id),
        plan_fingerprint: Some(plan.metadata.fingerprint.clone()),
        idempotency_key: Some(plan.metadata.idempotency_key.clone()),
    }
}

fn propedit_preview_response(plan: &PropertyEditPlan) -> PropEditPreviewResponse {
    let mut actions = Vec::new();
    let mut skipped = Vec::new();
    let mut no_change = Vec::new();
    for item in &plan.payload.items {
        if item.can_apply() {
            actions.push(PropEditActionRow {
                index: actions.len(),
                file_path: item.path.to_string_lossy().into_owned(),
                file_name: file_name(&item.path),
                description: format!("Apply {} property mutation(s)", item.mutations.len()),
                command: format!(
                    "mkvpropedit \"{}\" [redacted structured edits]",
                    item.path.display()
                ),
            });
        } else {
            let row = PropEditSkippedRow {
                file_path: item.path.to_string_lossy().into_owned(),
                file_name: file_name(&item.path),
                reason: item.conflicts.first().map_or_else(
                    || "No property changes".to_owned(),
                    |value| value.message.clone(),
                ),
            };
            if item.mutations.is_empty() {
                no_change.push(PropEditNoChangeRow {
                    file_path: row.file_path,
                    file_name: row.file_name,
                    reason: row.reason,
                });
            } else {
                skipped.push(row);
            }
        }
    }
    PropEditPreviewResponse {
        summary: format!(
            "{} action(s), {} skipped, {} no-change",
            actions.len(),
            skipped.len(),
            no_change.len()
        ),
        status: "Track-properties preview ready".to_owned(),
        actions,
        skipped,
        no_change,
        plan_id: Some(plan.metadata.id),
        plan_fingerprint: Some(plan.metadata.fingerprint.clone()),
        idempotency_key: Some(plan.metadata.idempotency_key.clone()),
    }
}

fn media_from_row(row: &mkvo_contracts::MediaFileRow, fingerprint: FileFingerprint) -> MediaFile {
    let tracks = row
        .tracks
        .iter()
        .map(|track| MediaTrack {
            mkvmerge_id: track.id,
            propedit_track_number: track.track_number,
            kind: parse_track_kind(&track.track_type),
            codec: track.codec.clone(),
            codec_id: None,
            language: (!track.language.is_empty()).then(|| track.language.clone()),
            name: (!track.name.is_empty()).then(|| track.name.clone()),
            resolution: None,
            bit_depth: None,
            hdr: None,
            channels: None,
            sampling_frequency_hz: None,
            default: track.default,
            forced: track.forced,
            enabled: true,
        })
        .collect();
    let attachments = row
        .attachments
        .iter()
        .map(|attachment| MediaAttachment {
            id: attachment.id,
            file_name: attachment.file_name.clone(),
            content_type: (!attachment.content_type.is_empty())
                .then(|| attachment.content_type.clone()),
            description: (!attachment.description.is_empty())
                .then(|| attachment.description.clone()),
            size_bytes: attachment.size_bytes,
        })
        .collect();
    MediaFile {
        path: PathBuf::from(&row.path),
        original_file_name: Some(row.file_name.clone()),
        watch_root: None,
        relative_path: None,
        fingerprint,
        container: ContainerMetadata {
            kind: match row
                .extension
                .trim_start_matches('.')
                .to_ascii_lowercase()
                .as_str()
            {
                "mkv" | "mka" => ContainerKind::Matroska,
                "webm" => ContainerKind::WebM,
                "mp4" | "m4v" => ContainerKind::Mp4,
                other => ContainerKind::Other(other.to_owned()),
            },
            title: None,
            duration_millis: None,
            muxing_application: None,
            writing_application: None,
        },
        tracks,
        attachments,
        episode: None,
        provider_match: None,
        status: MediaStatus::Ready,
    }
}

fn parse_track_kind(value: &str) -> TrackKind {
    match value.trim().to_ascii_lowercase().as_str() {
        "video" => TrackKind::Video,
        "audio" => TrackKind::Audio,
        "subtitle" | "subtitles" => TrackKind::Subtitle,
        "buttons" => TrackKind::Buttons,
        _ => TrackKind::Other,
    }
}

async fn discover_external_subtitles(
    files: &[MediaFile],
    formats: &str,
    default_language: &str,
    track_name_template: Option<&str>,
) -> BTreeMap<PathBuf, Vec<ExternalSubtitle>> {
    let formats = split_strings(formats);
    let mut result = BTreeMap::new();
    for file in files {
        let Some(parent) = file.path.parent() else {
            continue;
        };
        let stem = file
            .path
            .file_stem()
            .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
        let Ok(mut entries) = tokio::fs::read_dir(parent).await else {
            continue;
        };
        let mut subtitles = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let extension = path.extension().map_or_else(String::new, |value| {
                value.to_string_lossy().to_ascii_lowercase()
            });
            let candidate_stem = path
                .file_stem()
                .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
            let candidate_lower = candidate_stem.to_ascii_lowercase();
            let stem_lower = stem.to_ascii_lowercase();
            let matching_suffix = candidate_lower.strip_prefix(&stem_lower);
            if formats.contains(&extension)
                && matching_suffix.is_some_and(|suffix| {
                    suffix.is_empty()
                        || suffix.starts_with('.')
                        || suffix.starts_with(' ')
                        || suffix.starts_with('-')
                        || suffix.starts_with('_')
                })
            {
                let suffix = candidate_stem
                    .get(stem.len()..)
                    .unwrap_or_default()
                    .trim_start_matches(['.', ' ', '-', '_']);
                let mut parts = suffix.split('.').filter(|value| !value.trim().is_empty());
                let first = parts.next();
                let (language, tag) = first.map_or_else(
                    || (default_language.trim().to_owned(), String::new()),
                    |first| {
                        if looks_like_language(first) {
                            (
                                first.to_ascii_lowercase(),
                                parts.collect::<Vec<_>>().join("."),
                            )
                        } else {
                            (default_language.trim().to_owned(), suffix.to_owned())
                        }
                    },
                );
                let rendered_name = track_name_template
                    .unwrap_or("{tag}")
                    .replace("{tag}", tag.trim())
                    .replace("{language}", &language)
                    .replace("{file}", &candidate_stem);
                subtitles.push(ExternalSubtitle {
                    path,
                    language,
                    name: (!rendered_name.trim().is_empty())
                        .then(|| rendered_name.trim().to_owned()),
                    default: false,
                    forced: candidate_stem.to_ascii_lowercase().contains("forced"),
                });
            }
        }
        if !subtitles.is_empty() {
            result.insert(file.path.clone(), subtitles);
        }
    }
    result
}

fn looks_like_language(value: &str) -> bool {
    (2..=3).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn build_extractions(
    files: &[MediaFile],
    languages: &str,
    _overwrite: bool,
) -> BTreeMap<PathBuf, Vec<TrackExtraction>> {
    let languages = split_strings(languages);
    files
        .iter()
        .map(|file| {
            let tracks = file
                .tracks
                .iter()
                .filter(|track| track.kind == TrackKind::Subtitle)
                .filter(|track| {
                    languages.is_empty()
                        || languages
                            .contains(&track.language_or_undetermined().to_ascii_lowercase())
                })
                .map(|track| {
                    let language = track.language_or_undetermined();
                    let output = file.path.with_extension(format!(
                        "{language}.{}.{}",
                        track.mkvmerge_id,
                        subtitle_extension(&track.codec)
                    ));
                    TrackExtraction {
                        track_id: track.mkvmerge_id,
                        kind: TrackKind::Subtitle,
                        output,
                    }
                })
                .collect();
            (file.path.clone(), tracks)
        })
        .collect()
}

fn subtitle_extension(codec: &str) -> &'static str {
    let codec = codec.to_ascii_lowercase();
    if codec.contains("ass") || codec.contains("ssa") {
        "ass"
    } else if codec.contains("pgs") || codec.contains("hdmv") {
        "sup"
    } else {
        "srt"
    }
}

fn split_strings(value: &str) -> BTreeSet<String> {
    value
        .split([',', ';', ' '])
        .map(|value| value.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn split_u64s(value: &str) -> BTreeSet<u64> {
    value
        .split([',', ';', ' '])
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

fn title_edit(mode: TitleEditMode, custom: &str) -> TextEdit {
    match mode {
        TitleEditMode::Keep => TextEdit::Keep,
        TitleEditMode::File => TextEdit::FromFileName,
        TitleEditMode::Custom => TextEdit::Set(custom.trim().to_owned()),
        TitleEditMode::Remove => TextEdit::Delete,
    }
}

fn build_track_edits(request: &PropEditPreviewRequest) -> Vec<TrackEditIntent> {
    let mut edits = Vec::new();
    append_track_edits(
        &mut edits,
        TrackKind::Audio,
        &request.audio_tracks,
        &request.selected_default_audio,
        &request.selected_forced_audio,
    );
    append_track_edits(
        &mut edits,
        TrackKind::Subtitle,
        &request.subtitle_tracks,
        &request.selected_default_subtitle,
        &request.selected_forced_subtitle,
    );
    edits
}

fn append_track_edits(
    edits: &mut Vec<TrackEditIntent>,
    kind: TrackKind,
    rows: &[PropEditTrackConfigRow],
    selected_default: &str,
    selected_forced: &str,
) {
    for (index, row) in rows.iter().enumerate() {
        let name = if row.edited_name == row.current_name {
            TextEdit::Keep
        } else if row.edited_name.trim().is_empty() {
            TextEdit::Delete
        } else {
            TextEdit::Set(row.edited_name.trim().to_owned())
        };
        let language = (row.edited_language.trim() != row.current_language.trim())
            .then(|| row.edited_language.trim().to_owned());
        let should_default = !selected_default.is_empty() && selected_default == row.track_label;
        let should_forced = !selected_forced.is_empty() && selected_forced == row.track_label;
        edits.push(TrackEditIntent {
            kind,
            ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
            name,
            language,
            default: (should_default != row.current_default).then_some(should_default),
            forced: (!selected_forced.is_empty()).then_some(should_forced),
        });
    }
}

fn prop_track_row(index: usize, track: &MediaTrack) -> PropEditTrackConfigRow {
    PropEditTrackConfigRow {
        track_number: u32::try_from(index + 1).unwrap_or(u32::MAX),
        track_label: format!("{} {}", track_kind_label(track.kind), index + 1),
        track_type: track_kind_label(track.kind).to_ascii_lowercase(),
        current_name: track.name.clone().unwrap_or_default(),
        current_language: track.language.clone().unwrap_or_default(),
        current_default: track.default,
        edited_name: track.name.clone().unwrap_or_default(),
        edited_language: track.language.clone().unwrap_or_default(),
    }
}

fn selected_track_label(
    file: &MediaFile,
    kind: TrackKind,
    predicate: impl Fn(&MediaTrack) -> bool,
) -> String {
    file.tracks
        .iter()
        .filter(|track| track.kind == kind)
        .enumerate()
        .find(|(_, track)| predicate(track))
        .map_or_else(String::new, |(index, _)| {
            format!("{} {}", track_kind_label(kind), index + 1)
        })
}

fn track_kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "Video",
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitle",
        TrackKind::Buttons => "Buttons",
        TrackKind::Other => "Track",
    }
}

fn remux_mode_label(mode: RemuxMode) -> &'static str {
    match mode {
        RemuxMode::Remux => "Remux",
        RemuxMode::ConvertToMkv => "Convert to MKV",
        RemuxMode::MuxSubtitles => "Mux subtitles",
        RemuxMode::ExtractSubtitles => "Extract subtitles",
    }
}

fn remux_tool_name(mode: RemuxMode) -> &'static str {
    if mode == RemuxMode::ExtractSubtitles {
        "mkvextract"
    } else {
        "mkvmerge"
    }
}

fn remux_description(item: &mkvo_domain::RemuxPlanItem) -> String {
    match item.mode {
        RemuxMode::ExtractSubtitles => {
            format!("Extract {} subtitle track(s)", item.extract_tracks.len())
        }
        RemuxMode::MuxSubtitles => format!(
            "Remux with {} matching external subtitle(s)",
            item.external_subtitles.len()
        ),
        RemuxMode::ConvertToMkv => "Losslessly copy streams into MKV".to_owned(),
        RemuxMode::Remux => format!("Keep {} selected track(s)", item.selected_track_ids.len()),
    }
}

fn redacted_remux_command(item: &mkvo_domain::RemuxPlanItem) -> String {
    format!(
        "{} [structured arguments] \"{}\"",
        remux_tool_name(item.mode),
        item.source.display()
    )
}

fn same_path(left: &Path, right: &Path) -> bool {
    path_key(&left.to_string_lossy()) == path_key(&right.to_string_lossy())
}

fn path_key(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use mkvo_application::{
        AuthorizedPathPolicy, FileSystem, MediaCatalog, MediaEnumerationRequest, MediaProbe,
        PortError, ToolExecutionResult, ToolExecutor, ToolRegistry,
    };
    use mkvo_contracts::{ApiErrorCode, ToolStatus};
    use mkvo_domain::{AppSettings, FileFingerprint, MediaFile, MediaStatus, RemuxMode};
    use mkvo_infra_sqlite::{SqliteRepositories, SqliteStore};
    use tokio::sync::RwLock;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{MemorySecretStore, RuntimeConfig, RuntimeDependencies};

    #[derive(Default)]
    struct EmptyCatalog;

    #[async_trait]
    impl MediaCatalog for EmptyCatalog {
        async fn enumerate(
            &self,
            _request: &MediaEnumerationRequest,
            _cancel: CancellationToken,
        ) -> Result<Vec<FileFingerprint>, PortError> {
            Ok(Vec::new())
        }
    }

    struct FailingProbe;

    #[async_trait]
    impl MediaProbe for FailingProbe {
        async fn inspect(
            &self,
            _path: &Path,
            _cancel: CancellationToken,
        ) -> Result<MediaFile, PortError> {
            Err(PortError::Other("probe not used".to_owned()))
        }
    }

    #[derive(Default)]
    struct AllowPaths;

    #[async_trait]
    impl AuthorizedPathPolicy for AllowPaths {
        async fn authorize_read(&self, path: &Path) -> Result<PathBuf, PortError> {
            Ok(path.to_path_buf())
        }

        async fn authorize_write(&self, path: &Path) -> Result<PathBuf, PortError> {
            Ok(path.to_path_buf())
        }
    }

    #[derive(Default)]
    struct FakeFileSystem {
        files: RwLock<HashMap<PathBuf, FileFingerprint>>,
        moves: AtomicUsize,
    }

    impl FakeFileSystem {
        async fn add(&self, fingerprint: FileFingerprint) {
            self.files
                .write()
                .await
                .insert(fingerprint.path.clone(), fingerprint);
        }
    }

    #[async_trait]
    impl FileSystem for FakeFileSystem {
        async fn exists(&self, path: &Path) -> Result<bool, PortError> {
            Ok(self.files.read().await.contains_key(path))
        }

        async fn is_directory(&self, _path: &Path) -> Result<bool, PortError> {
            Ok(true)
        }

        async fn fingerprint(&self, path: &Path) -> Result<FileFingerprint, PortError> {
            self.files
                .read()
                .await
                .get(path)
                .cloned()
                .ok_or_else(|| PortError::NotFound(path.display().to_string()))
        }

        async fn move_file(&self, source: &Path, target: &Path) -> Result<(), PortError> {
            self.moves.fetch_add(1, Ordering::SeqCst);
            let mut files = self.files.write().await;
            let mut fingerprint = files
                .remove(source)
                .ok_or_else(|| PortError::NotFound(source.display().to_string()))?;
            if files.contains_key(target) {
                return Err(PortError::Conflict(target.display().to_string()));
            }
            fingerprint.path = target.to_path_buf();
            files.insert(target.to_path_buf(), fingerprint);
            Ok(())
        }

        async fn remove_file(&self, path: &Path) -> Result<(), PortError> {
            self.files.write().await.remove(path);
            Ok(())
        }
    }

    struct FakeTools;

    #[async_trait]
    impl ToolRegistry for FakeTools {
        async fn status(&self, logical_name: &str) -> Result<ToolStatus, PortError> {
            Ok(ToolStatus {
                name: logical_name.to_owned(),
                command: logical_name.to_owned(),
                resolved_path: format!("/tools/{logical_name}"),
                available: true,
                version: "test".to_owned(),
                error: None,
            })
        }

        async fn all_statuses(&self) -> Result<Vec<ToolStatus>, PortError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct FailingExecutor {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ToolExecutor for FailingExecutor {
        async fn execute(
            &self,
            _invocation: &ToolInvocation,
            _cancel: CancellationToken,
        ) -> Result<ToolExecutionResult, PortError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(PortError::Other("intentional test stop".to_owned()))
        }
    }

    struct Fixture {
        runtime: MkvoRuntime,
        file_system: Arc<FakeFileSystem>,
        executor: Arc<FailingExecutor>,
    }

    fn fixture() -> Fixture {
        let repositories = Arc::new(SqliteRepositories::from_store(
            SqliteStore::open_in_memory().expect("SQLite"),
        ));
        let file_system = Arc::new(FakeFileSystem::default());
        let executor = Arc::new(FailingExecutor::default());
        let dependencies = RuntimeDependencies {
            catalog: Arc::new(EmptyCatalog),
            probe: Arc::new(FailingProbe),
            cache: repositories.clone(),
            paths: Arc::new(AllowPaths),
            file_system: file_system.clone(),
            tools: Arc::new(FakeTools),
            tool_executor: executor.clone(),
            settings: repositories.clone(),
            secrets: Arc::new(MemorySecretStore::new()),
            plans: repositories.clone(),
            jobs: repositories.clone(),
            rename_history: repositories.clone(),
            journal: repositories.clone(),
            logs: repositories,
            watcher: None,
            process_tools: None,
            authorized_roots: None,
        };
        let config = RuntimeConfig::new(PathBuf::from("/media"), PathBuf::from("/config"));
        Fixture {
            runtime: MkvoRuntime::from_parts(config, dependencies),
            file_system,
            executor,
        }
    }

    fn fingerprint(path: &str) -> FileFingerprint {
        FileFingerprint {
            path: PathBuf::from(path),
            size_bytes: 10,
            modified_at: Utc::now(),
            quick_hash: Some("hash".to_owned()),
        }
    }

    fn media(path: &str) -> MediaFile {
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: fingerprint(path),
            container: ContainerMetadata::default(),
            tracks: vec![MediaTrack {
                mkvmerge_id: 0,
                propedit_track_number: 1,
                kind: TrackKind::Video,
                codec: "AVC".to_owned(),
                codec_id: None,
                language: None,
                name: None,
                resolution: None,
                bit_depth: None,
                hdr: None,
                channels: None,
                sampling_frequency_hz: None,
                default: true,
                forced: false,
                enabled: true,
            }],
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    fn mux_request(plan: Option<&RemuxPlan>) -> MuxPreviewRequest {
        MuxPreviewRequest {
            files: Vec::new(),
            selected_paths: Vec::new(),
            remove_unwanted_audio_languages: false,
            keep_audio_languages: String::new(),
            remove_unwanted_subtitle_languages: false,
            keep_subtitle_languages: String::new(),
            remove_unwanted_track_ids: false,
            remove_track_ids_text: String::new(),
            preserve_chapters: true,
            preserve_attachments: true,
            mux_matching_external_subtitles: false,
            external_subtitle_language: "eng".to_owned(),
            external_subtitle_track_name: Some("{tag}".to_owned()),
            external_subtitle_formats: "srt".to_owned(),
            preserve_external_subtitle_files: true,
            skip_mux_if_subtitle_already_exists: false,
            extract_subtitles: false,
            extract_subtitle_languages: "eng".to_owned(),
            extract_overwrite_existing_files: false,
            convert_mp4_to_mkv: false,
            delete_mp4_after_convert: false,
            plan_id: plan.map(|plan| plan.metadata.id),
            plan_fingerprint: plan.map(|plan| plan.metadata.fingerprint.clone()),
            idempotency_key: plan.map(|plan| plan.metadata.idempotency_key.clone()),
        }
    }

    fn remux_plan(path: &str, key: IdempotencyKey) -> RemuxPlan {
        let settings = AppSettings::default();
        RemuxPlanner
            .build_plan(RemuxPlanRequest {
                mode: RemuxMode::ConvertToMkv,
                files: vec![media(path)],
                options: RemuxOptions::default(),
                external_subtitles: BTreeMap::new(),
                extractions: BTreeMap::new(),
                existing_paths: BTreeSet::new(),
                authorized_roots: Vec::new(),
                settings_fingerprint: stable_fingerprint(&settings).expect("settings fingerprint"),
                tool_fingerprints: BTreeMap::new(),
                expires_in_seconds: 60,
                idempotency_key: key,
            })
            .expect("remux plan")
    }

    #[tokio::test]
    async fn mutation_is_rejected_without_an_immutable_plan() {
        let fixture = fixture();
        let rename_error = fixture
            .runtime
            .apply_rename_preview(RenameApplyRequest::default())
            .await
            .expect_err("rename without plan");
        assert_eq!(rename_error.code, ApiErrorCode::InvalidRequest);

        let mux_error = fixture
            .runtime
            .start_mux_apply(mux_request(None))
            .await
            .expect_err("mux without plan");
        assert_eq!(mux_error.code, ApiErrorCode::InvalidRequest);
        assert_eq!(fixture.file_system.moves.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.executor.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn tampered_plan_fingerprint_never_reaches_file_system() {
        let fixture = fixture();
        let source = media("/media/Episode.mkv");
        fixture.file_system.add(source.fingerprint.clone()).await;
        let settings_fingerprint = stable_fingerprint(&AppSettings::default()).expect("settings");
        let plan = RenamePlanner
            .build_plan(RenamePlanRequest {
                files: vec![source],
                template: "Renamed".to_owned(),
                provider: None,
                check_existing_files: false,
                existing_paths: BTreeSet::new(),
                authorized_roots: Vec::new(),
                settings_fingerprint,
                expires_in_seconds: 60,
                idempotency_key: IdempotencyKey::generate(),
            })
            .expect("rename plan");
        fixture.runtime.persist_plan(&plan).await.expect("persist");
        let error = fixture
            .runtime
            .apply_rename_preview(RenameApplyRequest {
                plan_id: Some(plan.metadata.id),
                plan_fingerprint: Some("tampered".to_owned()),
                idempotency_key: Some(plan.metadata.idempotency_key.clone()),
                ..RenameApplyRequest::default()
            })
            .await
            .expect_err("tampered plan");
        assert_eq!(error.code, ApiErrorCode::PlanTampered);
        assert_eq!(fixture.file_system.moves.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repeated_job_submission_is_idempotent_and_different_plan_conflicts() {
        let fixture = fixture();
        let key = IdempotencyKey::parse("same-operation").expect("key");
        let plan = remux_plan("/media/movie.mp4", key.clone());
        fixture
            .file_system
            .add(plan.context.input_fingerprints[0].clone())
            .await;
        fixture.runtime.persist_plan(&plan).await.expect("persist");

        let first = fixture
            .runtime
            .start_mux_apply(mux_request(Some(&plan)))
            .await
            .expect("first submission");
        let second = fixture
            .runtime
            .start_mux_apply(mux_request(Some(&plan)))
            .await
            .expect("idempotent replay");
        assert_eq!(first.id, second.id);

        let other = remux_plan("/media/other.mp4", key);
        fixture
            .file_system
            .add(other.context.input_fingerprints[0].clone())
            .await;
        fixture
            .runtime
            .persist_plan(&other)
            .await
            .expect("persist other");
        let conflict = fixture
            .runtime
            .start_mux_apply(mux_request(Some(&other)))
            .await
            .expect_err("same key for another plan");
        assert_eq!(conflict.code, ApiErrorCode::Conflict);

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert_eq!(fixture.executor.calls.load(Ordering::SeqCst), 1);
    }
}
