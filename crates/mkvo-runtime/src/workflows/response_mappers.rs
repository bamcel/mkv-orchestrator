use mkvo_contracts::{
    PropEditActionRow, PropEditNoChangeRow, PropEditSkippedRow, RenameApplyResponse,
    RenamePreviewRow, RenameScopeRow,
};
use mkvo_domain::{IdempotencyKey, PropertyEditPlan, RemuxPlan, RenamePlan};

use super::rename_presentation::{
    file_name, redacted_remux_command, remux_description, remux_mode_label, remux_tool_name,
    same_path,
};
use crate::compat::{MuxPreviewResponse, PropEditPreviewResponse, RenamePreviewResponse};
use crate::runtime::display_path;

pub(super) fn rename_preview_response(
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

pub(super) fn rename_apply_response(plan: &RenamePlan, replay: bool) -> RenameApplyResponse {
    let items = plan
        .payload
        .items
        .iter()
        .map(|item| RenamePreviewRow {
            // Applying consumes the selection. Leaving completed rows selected
            // makes the Rename page look as though the old preview is still
            // waiting to run.
            selected: false,
            source_path: display_path(if item.can_apply() {
                &item.target
            } else {
                &item.source
            }),
            // Once the move succeeds, the target is the current file. The old
            // mapping combined the target path with the source file name,
            // leaving the Rename page visually stuck on its pre-apply state.
            current_file_name: file_name(if item.can_apply() {
                &item.target
            } else {
                &item.source
            }),
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

pub(super) fn mux_preview_response(plan: &RemuxPlan) -> MuxPreviewResponse {
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

pub(super) fn propedit_preview_response(plan: &PropertyEditPlan) -> PropEditPreviewResponse {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{Duration, Utc};
    use mkvo_domain::{
        FileFingerprint, IdempotencyKey, OperationKind, OperationPlan, PlanContext, RenamePlanItem,
        RenamePlanPayload,
    };

    use super::rename_apply_response;

    #[test]
    fn applied_rename_rows_present_the_target_as_the_current_file() {
        let created_at = Utc::now();
        let source = PathBuf::from(r"C:\media\old-name.mkv");
        let target = PathBuf::from(r"C:\media\new-name.mkv");
        let payload = RenamePlanPayload {
            template: "{title}".to_owned(),
            provider: None,
            items: vec![RenamePlanItem {
                source: source.clone(),
                target: target.clone(),
                source_fingerprint: FileFingerprint {
                    path: source,
                    size_bytes: 1,
                    modified_at: created_at,
                    quick_hash: None,
                },
                new_file_name: "new-name.mkv".to_owned(),
                conflicts: Vec::new(),
            }],
        };
        let plan = OperationPlan::new(
            OperationKind::Rename,
            &"request",
            payload,
            PlanContext::empty("settings"),
            created_at,
            created_at + Duration::minutes(15),
            IdempotencyKey::generate(),
        )
        .expect("valid rename plan");

        let response = rename_apply_response(&plan, false);
        let row = &response.items[0];

        assert_eq!(row.current_file_name, "new-name.mkv");
        assert!(row.source_path.ends_with(r"media\new-name.mkv"));
        assert!(!row.selected);
        assert_eq!(row.status, "Renamed");
    }
}
