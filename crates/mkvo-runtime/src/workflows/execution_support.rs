use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mkvo_application::ApplicationError;
use mkvo_domain::{IdempotencyKey, MediaFile, PlanId, PropertyMutation, TrackKind};

use crate::{RuntimeError, RuntimeResult};

pub(super) fn require_plan_fields(
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

pub(super) fn runtime_application_error(error: RuntimeError) -> ApplicationError {
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

pub(super) fn validate_idempotent_job(
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

pub(super) fn append_propedit_arguments(arguments: &mut Vec<String>, mutation: &PropertyMutation) {
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

pub(super) fn backup_path(source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    source.with_file_name(format!(".{name}.mkvo-backup"))
}

pub(super) fn extraction_temp_path(output: &Path, track_id: u64) -> PathBuf {
    let name = output
        .file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    output.with_file_name(format!(".{name}.mkvo-extract-{track_id}.tmp"))
}

pub(super) fn append_track_selection(
    arguments: &mut Vec<String>,
    source: &MediaFile,
    selected: &[u64],
) {
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
