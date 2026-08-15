use super::*;

impl MkvoRuntime {
    pub(super) async fn build_mux_preview_impl(
        &self,
        request: MuxPreviewRequest,
    ) -> RuntimeResult<MuxPreviewResponse> {
        let plan = self.plan_mux(&request).await?;
        self.persist_plan(&plan).await?;
        Ok(mux_preview_response(&plan))
    }

    pub(super) async fn plan_mux(&self, request: &MuxPreviewRequest) -> RuntimeResult<RemuxPlan> {
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
        // Extraction only reads the source; every other mode rewrites it.
        let required = if mode == RemuxMode::ExtractSubtitles {
            RequiredAccess::Read
        } else {
            RequiredAccess::ReadWrite
        };
        let source_access = self.probe_source_access(&files, required).await;
        let base = RemuxPlanner
            .build_plan(RemuxPlanRequest {
                mode,
                files,
                source_access,
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

    pub(super) async fn start_mux_apply_impl(
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

    pub(super) async fn execute_remux_plan(
        &self,
        plan: &RemuxPlan,
        context: &mkvo_application::JobContext,
    ) -> Result<JobCompletion, ApplicationError> {
        let runnable: Vec<_> = plan
            .payload
            .items
            .iter()
            .filter(|item| item.can_apply())
            .cloned()
            .collect();
        let mut journal = self
            .begin_journal(
                plan.metadata.id,
                &plan.metadata.idempotency_key,
                &plan.context.resources,
                runnable
                    .iter()
                    .map(|item| item.source.to_string_lossy().into_owned()),
            )
            .await?;
        let total = u64::try_from(runnable.len()).unwrap_or(u64::MAX);
        let workers = self
            .settings_service()
            .load()
            .await?
            .settings
            .workers
            .max_remux_workers;
        let mut executions = stream::iter(runnable.into_iter().enumerate())
            .map(|(index, item)| async move {
                let key = item.source.to_string_lossy().into_owned();
                let result = self
                    .execute_remux_item(&item, &plan.context.attributes, context, index, total)
                    .await;
                (key, result)
            })
            .buffer_unordered(workers);
        let mut first_error = None;
        while let Some((key, result)) = executions.next().await {
            match result {
                Ok(()) => {
                    journal.step = journal.step.saturating_add(1);
                    journal.complete_item(&key);
                    context.record_completed().await?;
                }
                Err(error) => {
                    journal.fail_item(&key, error.to_string());
                    context.record_failed().await?;
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            journal.updated_utc = Utc::now();
            self.dependencies().journal.advance(&journal).await?;
        }
        if let Some(error) = first_error {
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

    pub(super) async fn execute_remux_item(
        &self,
        item: &mkvo_domain::RemuxPlanItem,
        attributes: &BTreeMap<String, String>,
        context: &mkvo_application::JobContext,
        index: usize,
        total: u64,
    ) -> Result<(), ApplicationError> {
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
        let invocation = self.remux_invocation(item, attributes).await?;
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
        if item.mode == RemuxMode::ExtractSubtitles {
            self.promote_extractions(item, attributes).await?;
        } else {
            self.promote_remux_output(item).await?;
        }
        if item.delete_external_subtitles_after_success {
            for subtitle in &item.external_subtitles {
                self.dependencies()
                    .file_system
                    .remove_file(&subtitle.path)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn remux_invocation(
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

    pub(super) async fn promote_remux_output(
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

    pub(super) async fn promote_extractions(
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
