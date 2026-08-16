use super::*;

impl MkvoRuntime {
    pub(super) async fn load_propedit_template_impl(
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

    pub(super) async fn build_propedit_preview_impl(
        &self,
        request: PropEditPreviewRequest,
    ) -> RuntimeResult<PropEditPreviewResponse> {
        let plan = self.plan_propedit(&request).await?;
        self.persist_plan(&plan).await?;
        Ok(propedit_preview_response(&plan))
    }

    pub(super) async fn plan_propedit(
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
        // mkvpropedit rewrites the file in place.
        let source_access = self
            .probe_source_access(&files, RequiredAccess::ReadWrite)
            .await;
        PropertyEditPlanner
            .build_plan(PropertyEditPlanRequest {
                files,
                source_access,
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

    pub(super) async fn start_propedit_apply_impl(
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

    pub(super) async fn execute_propedit_plan(
        &self,
        plan: &PropertyEditPlan,
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
                    .map(|item| item.path.to_string_lossy().into_owned()),
            )
            .await?;
        let total = u64::try_from(runnable.len()).unwrap_or(u64::MAX);
        let workers = self
            .settings_service()
            .load()
            .await?
            .settings
            .workers
            .max_edit_workers;
        let mut executions = stream::iter(runnable.into_iter().enumerate())
            .map(|(index, item)| async move {
                let key = item.path.to_string_lossy().into_owned();
                let result = self
                    .execute_propedit_item(&item, context, index, total)
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

    pub(super) async fn execute_propedit_item(
        &self,
        item: &mkvo_domain::PropertyEditPlanItem,
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
        // Refresh each successful file immediately. If a later file in the
        // batch fails, every tab still sees the edits that already landed.
        self.refresh_working_set(std::slice::from_ref(&item.path))
            .await;
        Ok(())
    }

    pub(super) async fn propedit_invocation(
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
}
