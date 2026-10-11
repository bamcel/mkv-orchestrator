use super::*;
use base64::Engine as _;

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
        let reorder_template = if request.reorder_tracks {
            let template_path = PathBuf::from(&request.reorder_template_path);
            Some(
                files
                    .iter()
                    .find(|file| same_path(&file.path, &template_path))
                    .cloned()
                    .ok_or_else(|| RuntimeError::invalid("Reorder template must be one of the selected MKV files."))?,
            )
        } else {
            None
        };
        let mode = if request.convert_mp4_to_mkv {
            RemuxMode::ConvertToMkv
        } else if request.extract_subtitles {
            RemuxMode::ExtractSubtitles
        } else if request.mux_matching_external_subtitles
            || !request.manual_subtitle_selections.is_empty()
        {
            RemuxMode::MuxSubtitles
        } else {
            RemuxMode::Remux
        };
        let mut existing_paths = self.current_existing_paths().await;
        let mut external_subtitles = if mode == RemuxMode::MuxSubtitles
            && request.mux_matching_external_subtitles
        {
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
        let allowed_subtitle_formats = split_strings(&request.external_subtitle_formats);
        let mut manual_subtitle_paths = BTreeSet::new();
        for selection in &request.manual_subtitle_selections {
            let target = PathBuf::from(&selection.target_path);
            let Some(file) = files.iter().find(|file| same_path(&file.path, &target)) else {
                return Err(RuntimeError::invalid(format!(
                    "Manual subtitle target is not a selected MKV: {}",
                    selection.target_path
                )));
            };
            let subtitle_path = PathBuf::from(&selection.subtitle_path);
            let extension = subtitle_path.extension().map_or_else(String::new, |value| {
                value.to_string_lossy().to_ascii_lowercase()
            });
            if !allowed_subtitle_formats.contains(&extension) {
                return Err(RuntimeError::invalid(format!(
                    "Unsupported manual subtitle format: {}",
                    selection.subtitle_path
                )));
            }
            if !tokio::fs::metadata(&subtitle_path)
                .await
                .is_ok_and(|metadata| metadata.is_file())
            {
                return Err(RuntimeError::not_found(format!(
                    "Manual subtitle file was not found: {}",
                    selection.subtitle_path
                )));
            }
            let subtitle_key = path_key(&subtitle_path.to_string_lossy());
            manual_subtitle_paths.insert(subtitle_key.clone());
            let subtitles = external_subtitles.entry(file.path.clone()).or_default();
            if subtitles
                .iter()
                .any(|subtitle| path_key(&subtitle.path.to_string_lossy()) == subtitle_key)
            {
                continue;
            }
            let forced = subtitle_path
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().to_ascii_lowercase().contains("forced"));
            subtitles.push(ExternalSubtitle {
                path: subtitle_path,
                language: request.external_subtitle_language.trim().to_owned(),
                name: None,
                default: false,
                forced,
            });
        }
        if request.skip_mux_if_subtitle_already_exists {
            for file in &files {
                if let Some(subtitles) = external_subtitles.get_mut(&file.path) {
                    subtitles.retain(|subtitle| {
                        manual_subtitle_paths
                            .contains(&path_key(&subtitle.path.to_string_lossy()))
                            || !file.tracks.iter().any(|track| {
                            track.kind == TrackKind::Subtitle
                                && track
                                    .language_or_undetermined()
                                    .eq_ignore_ascii_case(&subtitle.language)
                                && subtitle_codec_matches(&track.codec, track.codec_id.as_deref(), &subtitle.path)
                                && subtitle_names_match(track.name.as_deref(), subtitle.name.as_deref())
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
        let required = if mode == RemuxMode::ExtractSubtitles
            || (request.preserve_original
                && matches!(mode, RemuxMode::Remux | RemuxMode::MuxSubtitles))
        {
            RequiredAccess::Read
        } else {
            RequiredAccess::ReadWrite
        };
        let source_access = self.probe_source_access(&files, required).await;
        let base = RemuxPlanner
            .build_plan(RemuxPlanRequest {
                mode,
                files: files.clone(),
                source_access,
                options: RemuxOptions {
                    filter_audio_languages: request.remove_unwanted_audio_languages,
                    keep_audio_languages: split_strings(&request.keep_audio_languages),
                    filter_subtitle_languages: request.remove_unwanted_subtitle_languages,
                    keep_subtitle_languages: split_strings(&request.keep_subtitle_languages),
                    remove_track_ids,
                    preserve_chapters: request.preserve_chapters,
                    preserve_attachments: request.preserve_attachments,
                    preserve_source: request.preserve_original,
                    output_suffix: request.remux_output_suffix.clone(),
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
        if let Some(template) = reorder_template.as_ref() {
            let desired_template_tracks = ordered_template_tracks(
                template,
                &request.reorder_template_track_ids,
            )?;
            for item in &mut payload.items {
                let Some(file) = files.iter().find(|file| same_path(&file.path, &item.source)) else {
                    continue;
                };
                match track_order_for(file, &desired_template_tracks) {
                    Ok(order) if order != file.tracks.iter().map(|track| track.mkvmerge_id).collect::<Vec<_>>() => {
                        item.track_order_ids = order;
                        item.conflicts.retain(|conflict| conflict.kind != mkvo_domain::PlanConflictKind::NoChange);
                    }
                    Ok(order) => {
                        item.track_order_ids = order;
                    }
                    Err(message) => item.conflicts.push(mkvo_domain::PlanConflict::blocking(
                        mkvo_domain::PlanConflictKind::InvalidSelection,
                        message,
                        Some(item.source.clone()),
                    )),
                }
            }
        }
        if request.format_subtitles {
            if request.subtitle_font_family.trim().is_empty() {
                return Err(RuntimeError::invalid("A subtitle font family is required."));
            }
            validate_css_color(&request.subtitle_primary_color)?;
            validate_css_color(&request.subtitle_outline_color)?;
            let outline_size = request.subtitle_outline_size.trim().parse::<f32>()
                .map_err(|_| RuntimeError::invalid("Subtitle outline size must be a number."))?;
            if !(0.0..=20.0).contains(&outline_size) {
                return Err(RuntimeError::invalid("Subtitle outline size must be between 0 and 20."));
            }
            validate_imported_font(&request.subtitle_font_file_name, &request.subtitle_font_data_base64)?;
            for item in &mut payload.items {
                let Some(file) = files.iter().find(|file| same_path(&file.path, &item.source)) else {
                    continue;
                };
                let selected_track_ids = request.subtitle_track_selections.iter()
                    .find(|selection| same_path(Path::new(&selection.file_path), &item.source))
                    .map(|selection| selection.track_ids.iter().copied().collect::<BTreeSet<_>>());
                let transforms = file.tracks.iter().filter(|track| {
                    track.kind == TrackKind::Subtitle
                        && is_editable_ass_track(track)
                        && selected_track_ids.as_ref().is_none_or(|ids| ids.contains(&track.mkvmerge_id))
                }).map(|track| {
                    let stem = item.source.file_stem().map_or_else(String::new, |value| value.to_string_lossy().into_owned());
                    mkvo_domain::SubtitleTransform {
                        track_id: track.mkvmerge_id,
                        temporary_path: item.source.with_file_name(format!(".{stem}.mkvo-subtitle-{}.ass", track.mkvmerge_id)),
                        language: track.language_or_undetermined().to_owned(),
                        name: track.name.clone(),
                        default: track.default,
                        forced: track.forced,
                    }
                }).collect::<Vec<_>>();
                if transforms.is_empty() {
                    item.conflicts.push(mkvo_domain::PlanConflict::blocking(
                        mkvo_domain::PlanConflictKind::InvalidSelection,
                        "No editable ASS/SSA subtitle tracks were selected. Bitmap subtitles such as PGS cannot have their font changed.",
                        Some(item.source.clone()),
                    ));
                    continue;
                }
                let transformed_ids = transforms.iter().map(|track| track.track_id).collect::<BTreeSet<_>>();
                item.selected_track_ids.retain(|id| !transformed_ids.contains(id));
                item.external_subtitles.extend(transforms.iter().map(|track| ExternalSubtitle {
                    path: track.temporary_path.clone(), language: track.language.clone(), name: track.name.clone(),
                    default: track.default, forced: track.forced,
                }));
                item.mux_track_order = file.tracks.iter().map(|track| {
                    transforms.iter().position(|candidate| candidate.track_id == track.mkvmerge_id)
                        .map_or_else(|| format!("0:{}", track.mkvmerge_id), |index| format!("{}:0", index + 1))
                }).collect();
                item.subtitle_transforms = transforms;
                item.conflicts.retain(|conflict| conflict.kind != mkvo_domain::PlanConflictKind::NoChange);
            }
        }
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
            if !same_path(&item.source, &item.final_output)
                && self.dependencies().file_system.exists(&item.final_output).await?
                && !item.conflicts.iter().any(|conflict| {
                    conflict.kind == mkvo_domain::PlanConflictKind::ExistingTarget
                })
            {
                item.conflicts.push(mkvo_domain::PlanConflict::blocking(
                    mkvo_domain::PlanConflictKind::ExistingTarget,
                    "Remux output already exists",
                    Some(item.final_output.clone()),
                ));
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
            "preserveOriginal".to_owned(),
            request.preserve_original.to_string(),
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
        context.attributes.insert("subtitleFontFamily".to_owned(), request.subtitle_font_family.trim().to_owned());
        context.attributes.insert("subtitlePrimaryColor".to_owned(), request.subtitle_primary_color.trim().to_owned());
        context.attributes.insert("subtitleOutlineColor".to_owned(), request.subtitle_outline_color.trim().to_owned());
        context.attributes.insert("subtitleOutlineSize".to_owned(), request.subtitle_outline_size.trim().to_owned());
        context.attributes.insert("subtitleFontFileName".to_owned(), request.subtitle_font_file_name.trim().to_owned());
        context.attributes.insert("subtitleFontDataBase64".to_owned(), request.subtitle_font_data_base64.clone());
        context.attributes.insert("subtitleTimingOffsetMs".to_owned(), request.subtitle_timing_offset_ms.to_string());
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
        if !item.subtitle_transforms.is_empty() {
            self.prepare_subtitle_transforms(item, attributes, context).await?;
        }
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
        for transform in &item.subtitle_transforms {
            if self.dependencies().file_system.exists(&transform.temporary_path).await? {
                self.dependencies().file_system.remove_file(&transform.temporary_path).await?;
            }
        }
        if let Some(path) = imported_font_path(item, attributes) {
            if self.dependencies().file_system.exists(&path).await? { self.dependencies().file_system.remove_file(&path).await?; }
        }
        if item.delete_external_subtitles_after_success {
            for subtitle in &item.external_subtitles {
                self.dependencies()
                    .file_system
                    .remove_file(&subtitle.path)
                    .await?;
            }
        }
        if item.mode != RemuxMode::ExtractSubtitles {
            if item.delete_source_after_success && !same_path(&item.source, &item.final_output) {
                self.apply_renames_to_working_set(&[(
                    item.source.clone(),
                    item.final_output.clone(),
                )])
                .await;
            } else {
                self.refresh_working_set(std::slice::from_ref(&item.final_output))
                    .await;
            }
        }
        Ok(())
    }

    async fn prepare_subtitle_transforms(
        &self,
        item: &mkvo_domain::RemuxPlanItem,
        attributes: &BTreeMap<String, String>,
        context: &mkvo_application::JobContext,
    ) -> Result<(), ApplicationError> {
        let executable = self.tool_path("mkvextract").await?;
        for track in &item.subtitle_transforms {
            if self.dependencies().file_system.exists(&track.temporary_path).await? {
                self.dependencies().file_system.remove_file(&track.temporary_path).await?;
            }
        }
        let font_name = attributes.get("subtitleFontFileName").map_or("", String::as_str);
        let font_data = attributes.get("subtitleFontDataBase64").map_or("", String::as_str);
        if !font_name.is_empty() && !font_data.is_empty() {
            let path = imported_font_path(item, attributes).ok_or_else(|| ApplicationError::InvalidRequest("Invalid imported font file name.".to_owned()))?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(font_data).map_err(|_| ApplicationError::InvalidRequest("Imported font data is not valid base64.".to_owned()))?;
            tokio::fs::write(&path, bytes).await.map_err(|error| ApplicationError::Internal(format!("could not stage imported font: {error}")))?;
        }
        let mut arguments = vec!["tracks".to_owned(), item.source.to_string_lossy().into_owned()];
        arguments.extend(item.subtitle_transforms.iter().map(|track| format!("{}:{}", track.track_id, track.temporary_path.to_string_lossy())));
        let invocation = ToolInvocation {
            tool: "mkvextract".to_owned(), executable, arguments,
            working_directory: item.source.parent().map(Path::to_path_buf),
            expected_outputs: item.subtitle_transforms.iter().map(|track| track.temporary_path.clone()).collect(),
        };
        let execution = self.dependencies().tool_executor.execute(&invocation, context.cancellation_token()).await?;
        if execution.exit_code != Some(0) {
            return Err(ApplicationError::Internal(format!("mkvextract exited with {:?}: {}", execution.exit_code, execution.stderr.trim())));
        }
        let font = attributes.get("subtitleFontFamily").map_or("", String::as_str);
        let primary_color = css_to_ass_color(attributes.get("subtitlePrimaryColor").map_or("#FFFFFF", String::as_str))?;
        let outline_color = css_to_ass_color(attributes.get("subtitleOutlineColor").map_or("#000000", String::as_str))?;
        let outline_size = attributes.get("subtitleOutlineSize").map_or("2", String::as_str);
        let offset = attributes.get("subtitleTimingOffsetMs").and_then(|value| value.parse::<i64>().ok()).unwrap_or(0);
        for track in &item.subtitle_transforms {
            let content = tokio::fs::read_to_string(&track.temporary_path).await.map_err(|error| ApplicationError::Internal(format!("could not read extracted subtitle {}: {error}", track.temporary_path.display())))?;
            let transformed = transform_ass_document(&content, font, &primary_color, &outline_color, outline_size, offset)?;
            tokio::fs::write(&track.temporary_path, transformed).await.map_err(|error| ApplicationError::Internal(format!("could not write transformed subtitle {}: {error}", track.temporary_path.display())))?;
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
        if !item.mux_track_order.is_empty() {
            arguments.push("--track-order".to_owned());
            arguments.push(item.mux_track_order.join(","));
        } else if !item.track_order_ids.is_empty() {
            arguments.push("--track-order".to_owned());
            arguments.push(
                item.track_order_ids
                    .iter()
                    .map(|id| format!("0:{id}"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
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
            arguments.push("--default-track-flag".to_owned());
            arguments.push(format!("0:{}", if subtitle.default { "yes" } else { "no" }));
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
        if let Some(font_path) = imported_font_path(item, attributes) {
            let extension = font_path.extension().and_then(|value| value.to_str()).unwrap_or("ttf");
            arguments.push("--attachment-name".to_owned());
            arguments.push(attributes.get("subtitleFontFileName").cloned().unwrap_or_default());
            arguments.push("--attachment-mime-type".to_owned());
            arguments.push(if extension.eq_ignore_ascii_case("otf") { "application/vnd.ms-opentype" } else { "application/x-truetype-font" }.to_owned());
            arguments.push("--attach-file".to_owned());
            arguments.push(font_path.to_string_lossy().into_owned());
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

fn ordered_template_tracks<'a>(
    template: &'a MediaFile,
    requested_ids: &[u64],
) -> RuntimeResult<Vec<&'a MediaTrack>> {
    let ids = if requested_ids.is_empty() {
        template.tracks.iter().map(|track| track.mkvmerge_id).collect::<Vec<_>>()
    } else {
        requested_ids.to_vec()
    };
    if ids.len() != template.tracks.len() {
        return Err(RuntimeError::invalid("Template order must contain every track exactly once."));
    }
    let mut ordered = Vec::with_capacity(ids.len());
    for id in ids {
        if ordered.iter().any(|track: &&MediaTrack| track.mkvmerge_id == id) {
            return Err(RuntimeError::invalid("Template order contains a duplicate track."));
        }
        ordered.push(template.tracks.iter().find(|track| track.mkvmerge_id == id).ok_or_else(|| {
            RuntimeError::invalid(format!("Template track ID {id} was not found."))
        })?);
    }
    Ok(ordered)
}

fn track_order_for(file: &MediaFile, template_tracks: &[&MediaTrack]) -> Result<Vec<u64>, String> {
    if file.tracks.len() != template_tracks.len() {
        return Err(format!("Track count does not match template ({} vs {}).", file.tracks.len(), template_tracks.len()));
    }
    let mut used = BTreeSet::new();
    let mut order = Vec::with_capacity(template_tracks.len());
    for template in template_tracks {
        let matches = file
            .tracks
            .iter()
            .filter(|track| !used.contains(&track.mkvmerge_id) && tracks_match(template, track))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "Could not uniquely match template track {} ({} / {} / {}).",
                template.propedit_track_number,
                track_kind_label(template.kind),
                template.language_or_undetermined(),
                template.name.as_deref().unwrap_or("unnamed")
            ));
        }
        used.insert(matches[0].mkvmerge_id);
        order.push(matches[0].mkvmerge_id);
    }
    Ok(order)
}

fn tracks_match(left: &MediaTrack, right: &MediaTrack) -> bool {
    left.kind == right.kind
        && left.language_or_undetermined().eq_ignore_ascii_case(right.language_or_undetermined())
        && left.name.as_deref().unwrap_or("").trim().eq_ignore_ascii_case(right.name.as_deref().unwrap_or("").trim())
        && left.codec_id.as_deref().unwrap_or(&left.codec).eq_ignore_ascii_case(right.codec_id.as_deref().unwrap_or(&right.codec))
        && left.channels == right.channels
}

fn subtitle_codec_matches(codec: &str, codec_id: Option<&str>, subtitle_path: &Path) -> bool {
    let extension = subtitle_path
        .extension()
        .map_or_else(String::new, |value| value.to_string_lossy().to_ascii_lowercase());
    let codec = codec.to_ascii_lowercase();
    let codec_id = codec_id.unwrap_or_default().to_ascii_lowercase();
    match extension.as_str() {
        "srt" => codec.contains("subrip") || codec.contains("srt") || codec_id == "s_text/utf8",
        "ass" => codec.contains("ass") || codec_id == "s_text/ass",
        "ssa" => codec.contains("ssa") || codec_id == "s_text/ssa",
        "sub" | "idx" => codec.contains("vobsub") || codec_id == "s_vobsub",
        "sup" => codec.contains("pgs") || codec.contains("hdmv") || codec_id == "s_hdmv/pgs",
        _ => false,
    }
}

fn subtitle_names_match(existing: Option<&str>, candidate: Option<&str>) -> bool {
    let existing = existing.map(str::trim).filter(|value| !value.is_empty());
    let candidate = candidate.map(str::trim).filter(|value| !value.is_empty());
    match (existing, candidate) {
        (None, None) => true,
        (Some(existing), Some(candidate)) => existing.eq_ignore_ascii_case(candidate),
        _ => false,
    }
}

fn is_editable_ass_track(track: &MediaTrack) -> bool {
    let codec = track.codec.to_ascii_lowercase();
    let codec_id = track.codec_id.as_deref().unwrap_or_default().to_ascii_lowercase();
    codec.contains("substation") || codec == "ass" || codec == "ssa" || codec_id == "s_text/ass" || codec_id == "s_text/ssa"
}

fn transform_ass_document(content: &str, font: &str, primary_color: &str, outline_color: &str, outline_size: &str, offset_ms: i64) -> Result<String, ApplicationError> {
    let mut section = "";
    let mut style_font_index = None;
    let mut style_primary_index = None;
    let mut style_outline_color_index = None;
    let mut style_outline_size_index = None;
    let mut event_start_index = None;
    let mut event_end_index = None;
    let mut event_text_index = None;
    let mut output = String::with_capacity(content.len());
    for raw in content.split_inclusive('\n') {
        let newline = if raw.ends_with("\r\n") { "\r\n" } else if raw.ends_with('\n') { "\n" } else { "" };
        let line = raw.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') { section = trimmed; }
        let mut next = line.to_owned();
        if section.eq_ignore_ascii_case("[V4+ Styles]") || section.eq_ignore_ascii_case("[V4 Styles]") {
            if let Some(value) = trimmed.strip_prefix("Format:") {
                let fields = value.split(',').map(str::trim).collect::<Vec<_>>();
                style_font_index = fields.iter().position(|field| field.eq_ignore_ascii_case("Fontname"));
                style_primary_index = fields.iter().position(|field| field.eq_ignore_ascii_case("PrimaryColour"));
                style_outline_color_index = fields.iter().position(|field| field.eq_ignore_ascii_case("OutlineColour"));
                style_outline_size_index = fields.iter().position(|field| field.eq_ignore_ascii_case("Outline"));
            } else if let Some(value) = line.strip_prefix("Style:") {
                let mut fields = value.split(',').map(str::to_owned).collect::<Vec<_>>();
                set_ass_style_field(&mut fields, style_font_index, font);
                set_ass_style_field(&mut fields, style_primary_index, primary_color);
                set_ass_style_field(&mut fields, style_outline_color_index, outline_color);
                set_ass_style_field(&mut fields, style_outline_size_index, outline_size);
                next = format!("Style:{}", fields.join(","));
            }
        } else if section.eq_ignore_ascii_case("[Events]") {
            if let Some(value) = trimmed.strip_prefix("Format:") {
                let fields = value.split(',').map(str::trim).collect::<Vec<_>>();
                event_start_index = fields.iter().position(|field| field.eq_ignore_ascii_case("Start"));
                event_end_index = fields.iter().position(|field| field.eq_ignore_ascii_case("End"));
                event_text_index = fields.iter().position(|field| field.eq_ignore_ascii_case("Text"));
            } else {
                if let Some(value) = line.strip_prefix("Dialogue:") {
                    let mut fields = value.split(',').map(str::to_owned).collect::<Vec<_>>();
                    if let (Some(start), Some(end)) = (event_start_index, event_end_index) {
                        if offset_ms != 0 && start < fields.len() && end < fields.len() {
                            fields[start] = shift_ass_time(&fields[start], offset_ms)?;
                            fields[end] = shift_ass_time(&fields[end], offset_ms)?;
                        }
                    }
                    if let Some(text) = event_text_index.filter(|index| *index < fields.len()) {
                        fields[text] = replace_ass_inline_fonts(&fields[text..].join(","), font);
                        fields.truncate(text + 1);
                    }
                    next = format!("Dialogue:{}", fields.join(","));
                }
            }
        }
        output.push_str(&next);
        output.push_str(newline);
    }
    Ok(output)
}

fn set_ass_style_field(fields: &mut [String], index: Option<usize>, value: &str) {
    if let Some(index) = index.filter(|index| *index < fields.len()) { fields[index] = value.to_owned(); }
}

fn validate_css_color(value: &str) -> RuntimeResult<()> {
    let value = value.trim();
    if value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|character| character.is_ascii_hexdigit()) { return Ok(()); }
    Err(RuntimeError::invalid("Subtitle colors must use #RRGGBB format."))
}

fn css_to_ass_color(value: &str) -> Result<String, ApplicationError> {
    validate_css_color(value).map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let hex = &value.trim()[1..];
    Ok(format!("&H00{}{}{}", &hex[4..6], &hex[2..4], &hex[0..2]))
}

fn validate_imported_font(file_name: &str, data: &str) -> RuntimeResult<()> {
    if file_name.trim().is_empty() && data.is_empty() { return Ok(()); }
    if file_name.trim().is_empty() || data.is_empty() { return Err(RuntimeError::invalid("Imported font name and data must both be provided.")); }
    let path = Path::new(file_name.trim());
    if path.file_name().and_then(|value| value.to_str()) != Some(file_name.trim()) { return Err(RuntimeError::invalid("Imported font name must not contain a path.")); }
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default();
    if !extension.eq_ignore_ascii_case("ttf") && !extension.eq_ignore_ascii_case("otf") { return Err(RuntimeError::invalid("Imported fonts must be .ttf or .otf files.")); }
    let decoded = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| RuntimeError::invalid("Imported font data is not valid base64."))?;
    if decoded.len() > 1024 * 1024 { return Err(RuntimeError::invalid("Imported fonts must be 1 MB or smaller.")); }
    Ok(())
}

fn imported_font_path(item: &mkvo_domain::RemuxPlanItem, attributes: &BTreeMap<String, String>) -> Option<PathBuf> {
    let name = attributes.get("subtitleFontFileName")?.trim();
    if name.is_empty() || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name) { return None; }
    let source_name = item.source.file_stem()?.to_string_lossy();
    Some(item.source.with_file_name(format!(".{source_name}.mkvo-font-{name}")))
}

fn replace_ass_inline_fonts(text: &str, font: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("\\fn") {
        output.push_str(&rest[..index + 3]);
        rest = &rest[index + 3..];
        let end = rest.find(['\\', '}']).unwrap_or(rest.len());
        output.push_str(font);
        rest = &rest[end..];
    }
    output.push_str(rest);
    output
}

fn shift_ass_time(value: &str, offset_ms: i64) -> Result<String, ApplicationError> {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    if parts.len() != 3 { return Err(ApplicationError::Internal(format!("invalid ASS timestamp: {value}"))); }
    let hours = parts[0].parse::<i64>().map_err(|_| ApplicationError::Internal(format!("invalid ASS timestamp: {value}")))?;
    let minutes = parts[1].parse::<i64>().map_err(|_| ApplicationError::Internal(format!("invalid ASS timestamp: {value}")))?;
    let seconds = parts[2].parse::<f64>().map_err(|_| ApplicationError::Internal(format!("invalid ASS timestamp: {value}")))?;
    let millis = ((hours * 3600 + minutes * 60) * 1000 + (seconds * 1000.0).round() as i64 + offset_ms).max(0);
    Ok(format!("{}:{:02}:{:02}.{:02}", millis / 3_600_000, (millis / 60_000) % 60, (millis / 1000) % 60, (millis % 1000) / 10))
}

#[cfg(test)]
mod subtitle_match_tests {
    use super::{shift_ass_time, subtitle_codec_matches, subtitle_names_match, track_order_for, transform_ass_document, validate_imported_font};
    use mkvo_domain::{ContainerMetadata, FileFingerprint, MediaFile, MediaStatus, MediaTrack, TrackKind};
    use std::path::Path;
    use chrono::Utc;

    fn track(id: u64, number: u32, kind: TrackKind, name: &str) -> MediaTrack {
        MediaTrack {
            mkvmerge_id: id, propedit_track_number: number, kind,
            codec: if kind == TrackKind::Subtitle { "SubStationAlpha" } else { "HEVC" }.to_owned(),
            codec_id: None, language: Some(if kind == TrackKind::Subtitle { "eng" } else { "und" }.to_owned()),
            name: (!name.is_empty()).then(|| name.to_owned()), resolution: None, bit_depth: None,
            hdr: None, channels: None, sampling_frequency_hz: None, default: false, forced: false, enabled: true,
        }
    }

    fn file(tracks: Vec<MediaTrack>) -> MediaFile {
        MediaFile {
            path: "episode.mkv".into(), original_file_name: None, watch_root: None, relative_path: None,
            fingerprint: FileFingerprint { path: "episode.mkv".into(), size_bytes: 1, modified_at: Utc::now(), quick_hash: None },
            container: ContainerMetadata::default(), tracks, attachments: Vec::new(), episode: None,
            provider_match: None, status: MediaStatus::Ready,
        }
    }

    #[test]
    fn requires_the_external_format_to_match_the_embedded_codec() {
        assert!(subtitle_codec_matches("SubRip/SRT", Some("S_TEXT/UTF8"), Path::new("Episode.eng.srt")));
        assert!(!subtitle_codec_matches("SubStationAlpha", Some("S_TEXT/ASS"), Path::new("Episode.eng.srt")));
        assert!(subtitle_codec_matches("SubStationAlpha", Some("S_TEXT/ASS"), Path::new("Episode.eng.ass")));
    }

    #[test]
    fn requires_both_tracks_to_have_the_same_name() {
        assert!(subtitle_names_match(Some("Dialogue"), Some("dialogue")));
        assert!(subtitle_names_match(None, None));
        assert!(!subtitle_names_match(Some("Dialogue"), Some("SDH")));
        assert!(!subtitle_names_match(Some("Dialogue"), None));
    }

    #[test]
    fn maps_reversed_subtitles_to_the_template_order() {
        let template_tracks = [
            track(0, 1, TrackKind::Video, ""),
            track(1, 2, TrackKind::Subtitle, "Dialogue"),
            track(2, 3, TrackKind::Subtitle, "Signs/Songs"),
        ];
        let target = file(vec![
            track(0, 1, TrackKind::Video, ""),
            track(1, 2, TrackKind::Subtitle, "Signs/Songs"),
            track(2, 3, TrackKind::Subtitle, "Dialogue"),
        ]);
        let desired = template_tracks.iter().collect::<Vec<_>>();

        assert_eq!(track_order_for(&target, &desired), Ok(vec![0, 2, 1]));
    }

    #[test]
    fn refuses_to_guess_between_duplicate_track_signatures() {
        let template_tracks = [track(0, 1, TrackKind::Subtitle, "Dialogue")];
        let target = file(vec![
            track(1, 1, TrackKind::Subtitle, "Dialogue"),
            track(2, 2, TrackKind::Subtitle, "Dialogue"),
        ]);
        let desired = template_tracks.iter().collect::<Vec<_>>();

        assert!(track_order_for(&target, &desired).is_err());
    }

    #[test]
    fn changes_ass_fonts_without_changing_other_style_fields() {
        let source = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, Outline, Bold\nStyle: Default,Arial,48,&H00FFFFFF,&H00000000,2,-1\n[Events]\nFormat: Layer, Start, End, Text\nDialogue: 0,0:00:01.00,0:00:02.00,{\\fnComic Sans}Hello, world\n";
        let result = transform_ass_document(source, "Noto Sans", "&H0000FFFF", "&H00FF0000", "3.5", 0).expect("transform");
        assert!(result.contains("Style: Default,Noto Sans,48,&H0000FFFF,&H00FF0000,3.5,-1"));
        assert!(result.contains("Dialogue: 0,0:00:01.00,0:00:02.00,{\\fnNoto Sans}Hello, world"));
    }

    #[test]
    fn shifts_ass_cues_and_clamps_negative_times() {
        assert_eq!(shift_ass_time("0:00:01.25", 500).expect("shift"), "0:00:01.75");
        assert_eq!(shift_ass_time("0:00:01.25", -2_000).expect("shift"), "0:00:00.00");
    }

    #[test]
    fn accepts_only_safe_supported_imported_fonts() {
        assert!(validate_imported_font("Papernotes.ttf", "AA==").is_ok());
        assert!(validate_imported_font("../Papernotes.ttf", "AA==").is_err());
        assert!(validate_imported_font("Papernotes.exe", "AA==").is_err());
        assert!(validate_imported_font("Papernotes.ttf", "not base64").is_err());
    }
}
