use super::*;

impl MkvoRuntime {
    pub(super) async fn run_library_audit_impl(
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

    pub(super) async fn resolve_rows(
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

    pub(super) async fn persist_plan<T: Serialize>(
        &self,
        plan: &OperationPlan<T>,
    ) -> RuntimeResult<()> {
        let stored = StoredPlan::try_from(plan)?;
        self.dependencies().plans.save(&stored).await?;
        Ok(())
    }

    pub(super) async fn load_valid_plan<T: DeserializeOwned + Serialize>(
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

    pub(super) async fn load_referenced_plan<T: DeserializeOwned + Serialize>(
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

    pub(super) async fn revalidate_fingerprint(
        &self,
        expected: &FileFingerprint,
    ) -> RuntimeResult<()> {
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

    pub(super) async fn current_existing_paths(&self) -> BTreeSet<PathBuf> {
        self.current_domain_files()
            .await
            .into_iter()
            .map(|file| file.path)
            .collect()
    }

    pub(super) fn authorized_root_paths(&self) -> Vec<PathBuf> {
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

    pub(super) async fn tool_fingerprints(&self) -> RuntimeResult<ToolFingerprints> {
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

    pub(super) async fn append_log(
        &self,
        area: &str,
        message: &str,
        detail: &str,
    ) -> RuntimeResult<()> {
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

    pub(super) async fn validate_plan_reference(
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

    pub(super) async fn begin_journal(
        &self,
        plan_id: PlanId,
        key: &IdempotencyKey,
        resources: &[mkvo_domain::ResourceClaim],
        items: impl IntoIterator<Item = String>,
    ) -> Result<mkvo_application::JournalRecord, ApplicationError> {
        let mut record = mkvo_application::JournalRecord {
            idempotency_key: key.clone(),
            plan_id,
            step: 0,
            status: mkvo_application::JournalStatus::Prepared,
            resources: resources.to_vec(),
            items: items
                .into_iter()
                .map(|key| mkvo_application::JournalItemOutcome {
                    key,
                    status: mkvo_application::JournalItemStatus::Pending,
                    detail: None,
                })
                .collect(),
            detail: None,
            updated_utc: Utc::now(),
        };
        self.dependencies().journal.begin(&record).await?;
        record.status = mkvo_application::JournalStatus::Running;
        self.dependencies().journal.advance(&record).await?;
        Ok(record)
    }

    pub(super) async fn tool_path(&self, name: &str) -> Result<PathBuf, ApplicationError> {
        let status = self.dependencies().tools.status(name).await?;
        if !status.available || status.resolved_path.trim().is_empty() {
            return Err(ApplicationError::InvalidRequest(format!(
                "required tool `{name}` is unavailable"
            )));
        }
        Ok(PathBuf::from(status.resolved_path))
    }
}
