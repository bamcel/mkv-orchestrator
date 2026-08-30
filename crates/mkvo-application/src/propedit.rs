use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{Duration, Utc};
use mkvo_domain::{
    IdempotencyKey, MediaFile, OperationKind, OperationPlan, PlanConflict, PlanConflictKind,
    PlanContext, PropertyEditPlan, PropertyEditPlanItem, PropertyEditPlanPayload, PropertyMutation,
    ResourceClaim, ToolFingerprints, TrackKind, TrackSelector,
};
use serde::{Deserialize, Serialize};

use crate::{ApplicationError, ApplicationResult, FileAccessState, paths::path_contains};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyEditPlanRequest {
    pub files: Vec<MediaFile>,
    pub container_title: TextEdit,
    pub video_track_name: TextEdit,
    #[serde(default)]
    pub track_edits: Vec<TrackEditIntent>,
    #[serde(default)]
    pub authorized_roots: Vec<PathBuf>,
    /// Access state of each source file, keyed by portable path. Gathered by
    /// the host; an absent entry means "not probed" and never blocks.
    #[serde(default)]
    pub source_access: BTreeMap<String, FileAccessState>,
    pub settings_fingerprint: String,
    #[serde(default)]
    pub tool_fingerprints: ToolFingerprints,
    #[serde(default = "default_plan_ttl_seconds")]
    pub expires_in_seconds: u64,
    pub idempotency_key: IdempotencyKey,
}

const fn default_plan_ttl_seconds() -> u64 {
    900
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum TextEdit {
    #[default]
    Keep,
    FromFileName,
    FromEpisodeTitle,
    FromTrackMetadata,
    Set(String),
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEditIntent {
    pub kind: TrackKind,
    pub ordinal: u32,
    #[serde(default)]
    pub name: TextEdit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PropertyEditPlanner;

impl PropertyEditPlanner {
    pub fn build_plan(
        &self,
        request: PropertyEditPlanRequest,
    ) -> ApplicationResult<PropertyEditPlan> {
        if request.files.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "property preview requires at least one file".to_owned(),
            ));
        }
        if request.expires_in_seconds == 0 {
            return Err(ApplicationError::InvalidRequest(
                "property plan expiration must be positive".to_owned(),
            ));
        }
        validate_track_intents(&request.track_edits)?;

        let items = request
            .files
            .iter()
            .map(|file| build_item(file, &request))
            .collect::<ApplicationResult<Vec<_>>>()?;
        let payload = PropertyEditPlanPayload { items };
        let context = PlanContext {
            settings_fingerprint: request.settings_fingerprint.clone(),
            tool_fingerprints: request.tool_fingerprints.clone(),
            input_fingerprints: request
                .files
                .iter()
                .map(|file| file.fingerprint.clone())
                .collect(),
            resources: request
                .files
                .iter()
                .map(|file| ResourceClaim::write(file.path.clone()))
                .collect(),
            attributes: BTreeMap::new(),
        };
        let now = Utc::now();
        let ttl = i64::try_from(request.expires_in_seconds).unwrap_or(i64::MAX);
        OperationPlan::new(
            OperationKind::PropertyEdit,
            &request,
            payload,
            context,
            now,
            now + Duration::seconds(ttl),
            request.idempotency_key.clone(),
        )
        .map_err(|error| ApplicationError::Internal(error.to_string()))
    }
}

fn validate_track_intents(intents: &[TrackEditIntent]) -> ApplicationResult<()> {
    let mut selectors = BTreeSet::new();
    for intent in intents {
        TrackSelector::new(intent.kind, intent.ordinal)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        if !selectors.insert((intent.kind, intent.ordinal)) {
            return Err(ApplicationError::InvalidRequest(format!(
                "duplicate property edit for {:?} track {}",
                intent.kind, intent.ordinal
            )));
        }
        if let Some(language) = &intent.language
            && !valid_language(language)
        {
            return Err(ApplicationError::InvalidRequest(format!(
                "invalid language tag '{language}'"
            )));
        }
    }
    Ok(())
}

fn build_item(
    file: &MediaFile,
    request: &PropertyEditPlanRequest,
) -> ApplicationResult<PropertyEditPlanItem> {
    let mut mutations = Vec::new();
    let mut conflicts = Vec::new();
    if !request.authorized_roots.is_empty()
        && !request
            .authorized_roots
            .iter()
            .any(|root| path_contains(root, &file.path))
    {
        conflicts.push(PlanConflict::blocking(
            PlanConflictKind::UnauthorizedPath,
            "File is outside the authorized roots",
            Some(file.path.clone()),
        ));
    }
    // mkvpropedit rewrites the file in place, so it needs write access.
    if let Some(conflict) = crate::rename::access_conflict(
        request
            .source_access
            .get(&crate::paths::path_key(&file.path)),
        &file.path,
    ) {
        conflicts.push(conflict);
    }

    let file_stem = file
        .path
        .file_stem()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    let episode_title = episode_title_for_file(file, &file_stem);
    add_container_edit(
        &request.container_title,
        file.container.title.as_deref(),
        &file_stem,
        &episode_title,
        &mut mutations,
    );

    if !matches!(request.video_track_name, TextEdit::Keep) {
        if let Some(video) = file
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video)
        {
            let selector = TrackSelector::new(TrackKind::Video, 1)
                .map_err(|error| ApplicationError::Internal(error.to_string()))?;
            add_track_name_edit(
                &request.video_track_name,
                video.name.as_deref(),
                &file_stem,
                &episode_title,
                selector,
                &mut mutations,
            );
        } else {
            conflicts.push(PlanConflict::blocking(
                PlanConflictKind::InvalidSelection,
                "File has no video track to edit",
                Some(file.path.clone()),
            ));
        }
    }

    for edit in &request.track_edits {
        let selector = TrackSelector::new(edit.kind, edit.ordinal)
            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
        let track = file
            .tracks
            .iter()
            .filter(|track| track.kind == edit.kind)
            .nth(usize::try_from(edit.ordinal - 1).unwrap_or(usize::MAX));
        let Some(track) = track else {
            conflicts.push(PlanConflict::blocking(
                PlanConflictKind::InvalidSelection,
                format!("File has no {:?} track {}", edit.kind, edit.ordinal),
                Some(file.path.clone()),
            ));
            continue;
        };
        let resolved_name = match &edit.name {
            TextEdit::FromTrackMetadata => {
                TextEdit::Set(metadata_track_name(track, edit.language.as_deref()))
            }
            _ => edit.name.clone(),
        };
        add_track_name_edit(
            &resolved_name,
            track.name.as_deref(),
            &file_stem,
            &episode_title,
            selector,
            &mut mutations,
        );
        if let Some(language) = edit.language.as_deref()
            && !track
                .language
                .as_deref()
                .is_some_and(|current| current.eq_ignore_ascii_case(language))
        {
            mutations.push(PropertyMutation::SetTrackLanguage {
                selector,
                language: language.to_owned(),
            });
        }
        if edit.default.is_some_and(|value| value != track.default) {
            mutations.push(PropertyMutation::SetDefaultFlag {
                selector,
                value: edit.default.unwrap_or_default(),
            });
        }
        if edit.forced.is_some_and(|value| value != track.forced) {
            mutations.push(PropertyMutation::SetForcedFlag {
                selector,
                value: edit.forced.unwrap_or_default(),
            });
        }
    }

    if mutations.is_empty() && conflicts.is_empty() {
        conflicts.push(PlanConflict::informational(
            PlanConflictKind::NoChange,
            "Properties already match the requested values",
            Some(file.path.clone()),
        ));
    }
    Ok(PropertyEditPlanItem {
        path: file.path.clone(),
        source_fingerprint: file.fingerprint.clone(),
        mutations,
        conflicts,
    })
}

fn episode_title_for_file(file: &MediaFile, file_stem: &str) -> String {
    let metadata_title = crate::rename::rename_tokens(file).episode_title;
    if !metadata_title.trim().is_empty() {
        return metadata_title;
    }

    // Scans performed after a rename do not retain provider metadata, but the
    // default rename template places the title immediately after SxxExx. Read
    // that suffix so "Use episode title" also works after an app restart.
    let bytes = file_stem.as_bytes();
    for start in 0..bytes.len() {
        if !matches!(bytes[start], b's' | b'S') {
            continue;
        }
        let mut cursor = start + 1;
        let season_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == season_start || cursor >= bytes.len() || !matches!(bytes[cursor], b'e' | b'E') {
            continue;
        }
        cursor += 1;
        let episode_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == episode_start {
            continue;
        }
        let title = file_stem[cursor..].trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '-' | '_' | '.')
        });
        if !title.is_empty() {
            return title.to_owned();
        }
    }
    String::new()
}

fn add_container_edit(
    edit: &TextEdit,
    current: Option<&str>,
    file_stem: &str,
    episode_title: &str,
    mutations: &mut Vec<PropertyMutation>,
) {
    match edit {
        TextEdit::Keep => {}
        TextEdit::Delete if current.is_some_and(|value| !value.is_empty()) => {
            mutations.push(PropertyMutation::DeleteContainerTitle);
        }
        TextEdit::FromFileName if current != Some(file_stem) => {
            mutations.push(PropertyMutation::SetContainerTitle {
                value: file_stem.to_owned(),
            });
        }
        TextEdit::FromEpisodeTitle if !episode_title.is_empty() && current != Some(episode_title) => {
            mutations.push(PropertyMutation::SetContainerTitle {
                value: episode_title.to_owned(),
            });
        }
        TextEdit::FromTrackMetadata | TextEdit::FromEpisodeTitle => {}
        TextEdit::Set(value) if current != Some(value.as_str()) => {
            mutations.push(PropertyMutation::SetContainerTitle {
                value: value.clone(),
            });
        }
        TextEdit::Delete | TextEdit::FromFileName | TextEdit::Set(_) => {}
    }
}

fn metadata_track_name(track: &mkvo_domain::MediaTrack, edited_language: Option<&str>) -> String {
    let language = language_display_name(
        edited_language
            .filter(|value| !value.trim().is_empty())
            .or(track.language.as_deref())
            .unwrap_or("und"),
    );
    let codec = codec_display_name(&track.codec);
    let mut parts = Vec::new();
    if !codec.is_empty() {
        parts.push(codec);
    }
    parts.push(language);
    if let Some(channels) = track.channels {
        parts.push(channel_display_name(channels));
    }
    parts.join(" ")
}

fn channel_display_name(channels: u16) -> String {
    match channels {
        1 => "1.0".to_owned(),
        2 => "2.0".to_owned(),
        6 => "5.1".to_owned(),
        8 => "7.1".to_owned(),
        value => format!("{value}.0"),
    }
}

fn codec_display_name(codec: &str) -> String {
    match codec.trim().to_ascii_lowercase().as_str() {
        "aac" => "AAC".to_owned(),
        "ac-3" | "ac3" => "AC-3".to_owned(),
        "e-ac-3" | "eac3" => "E-AC-3".to_owned(),
        "dts" => "DTS".to_owned(),
        "truehd" => "TrueHD".to_owned(),
        "opus" => "Opus".to_owned(),
        "flac" => "FLAC".to_owned(),
        _ => codec.trim().to_owned(),
    }
}

fn language_display_name(language: &str) -> String {
    let name = match language.trim().to_ascii_lowercase().as_str() {
        "eng" | "en" => "English",
        "jpn" | "ja" => "Japanese",
        "spa" | "es" => "Spanish",
        "fra" | "fre" | "fr" => "French",
        "deu" | "ger" | "de" => "German",
        "ita" | "it" => "Italian",
        "por" | "pt" => "Portuguese",
        "kor" | "ko" => "Korean",
        "zho" | "chi" | "zh" => "Chinese",
        "rus" | "ru" => "Russian",
        "ara" | "ar" => "Arabic",
        "hin" | "hi" => "Hindi",
        "nld" | "dut" | "nl" => "Dutch",
        "pol" | "pl" => "Polish",
        "tur" | "tr" => "Turkish",
        "und" | "" => "Undetermined",
        _ => return language.trim().to_owned(),
    };
    name.to_owned()
}

fn add_track_name_edit(
    edit: &TextEdit,
    current: Option<&str>,
    file_stem: &str,
    episode_title: &str,
    selector: TrackSelector,
    mutations: &mut Vec<PropertyMutation>,
) {
    match edit {
        TextEdit::Keep => {}
        TextEdit::Delete if current.is_some_and(|value| !value.is_empty()) => {
            mutations.push(PropertyMutation::DeleteTrackName { selector });
        }
        TextEdit::FromFileName if current != Some(file_stem) => {
            mutations.push(PropertyMutation::SetTrackName {
                selector,
                value: file_stem.to_owned(),
            });
        }
        TextEdit::FromEpisodeTitle if !episode_title.is_empty() && current != Some(episode_title) => {
            mutations.push(PropertyMutation::SetTrackName {
                selector,
                value: episode_title.to_owned(),
            });
        }
        TextEdit::FromTrackMetadata | TextEdit::FromEpisodeTitle => {}
        TextEdit::Set(value) if current != Some(value.as_str()) => {
            if value.is_empty() {
                mutations.push(PropertyMutation::DeleteTrackName { selector });
            } else {
                mutations.push(PropertyMutation::SetTrackName {
                    selector,
                    value: value.clone(),
                });
            }
        }
        TextEdit::Delete | TextEdit::FromFileName | TextEdit::Set(_) => {}
    }
}

fn valid_language(value: &str) -> bool {
    let value = value.trim();
    (2..=15).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use mkvo_domain::{ContainerMetadata, EpisodeIdentity, FileFingerprint, MediaStatus, MediaTrack};

    use super::*;

    fn media() -> MediaFile {
        MediaFile {
            path: PathBuf::from("episode.mkv"),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: FileFingerprint {
                path: PathBuf::from("episode.mkv"),
                size_bytes: 1,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: ContainerMetadata::default(),
            tracks: vec![
                track(0, TrackKind::Video, None),
                track(1, TrackKind::Audio, Some("Old")),
            ],
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    fn track(id: u64, kind: TrackKind, name: Option<&str>) -> MediaTrack {
        MediaTrack {
            mkvmerge_id: id,
            propedit_track_number: u32::try_from(id + 1).unwrap(),
            kind,
            codec: "codec".to_owned(),
            codec_id: None,
            language: Some("eng".to_owned()),
            name: name.map(str::to_owned),
            resolution: None,
            bit_depth: None,
            hdr: None,
            channels: None,
            sampling_frequency_hz: None,
            default: false,
            forced: false,
            enabled: true,
        }
    }

    #[test]
    fn creates_type_specific_track_mutations() {
        let request = PropertyEditPlanRequest {
            source_access: BTreeMap::new(),
            files: vec![media()],
            container_title: TextEdit::FromFileName,
            video_track_name: TextEdit::Keep,
            track_edits: vec![TrackEditIntent {
                kind: TrackKind::Audio,
                ordinal: 1,
                name: TextEdit::Set("English".to_owned()),
                language: None,
                default: Some(true),
                forced: None,
            }],
            authorized_roots: Vec::new(),
            settings_fingerprint: "settings".to_owned(),
            tool_fingerprints: BTreeMap::new(),
            expires_in_seconds: 60,
            idempotency_key: IdempotencyKey::generate(),
        };
        let plan = PropertyEditPlanner.build_plan(request).unwrap();
        assert_eq!(plan.payload.items[0].mutations.len(), 3);
        assert!(plan.payload.items[0].can_apply());
    }

    #[test]
    fn duplicate_selector_is_rejected() {
        let edit = TrackEditIntent {
            kind: TrackKind::Audio,
            ordinal: 1,
            name: TextEdit::Keep,
            language: None,
            default: None,
            forced: None,
        };
        assert!(validate_track_intents(&[edit.clone(), edit]).is_err());
    }

    #[test]
    fn metadata_audio_name_uses_codec_language_and_channels_without_brackets() {
        let mut audio = track(1, TrackKind::Audio, None);
        audio.codec = "aac".to_owned();
        audio.channels = Some(6);

        assert_eq!(
            metadata_track_name(&audio, Some("eng")),
            "AAC English 5.1"
        );
    }

    #[test]
    fn resolves_episode_title_for_each_container_and_video_track() {
        let mut first = media();
        first.episode = Some(EpisodeIdentity {
            series_title: None,
            season: Some(1),
            episode: Some(1),
            absolute_episode: None,
            episode_title: Some("Winter Is Coming".to_owned()),
            year: None,
            is_movie: false,
        });
        let mut second = media();
        second.path = PathBuf::from("Kingdom (2019) - S01E02 - The Kingsroad.mkv");
        second.fingerprint.path = second.path.clone();
        let request = PropertyEditPlanRequest {
            source_access: BTreeMap::new(),
            files: vec![first, second],
            container_title: TextEdit::FromEpisodeTitle,
            video_track_name: TextEdit::FromEpisodeTitle,
            track_edits: Vec::new(),
            authorized_roots: Vec::new(),
            settings_fingerprint: "settings".to_owned(),
            tool_fingerprints: BTreeMap::new(),
            expires_in_seconds: 60,
            idempotency_key: IdempotencyKey::generate(),
        };

        let plan = PropertyEditPlanner.build_plan(request).unwrap();
        let values = plan.payload.items.iter().map(|item| {
            item.mutations.iter().filter_map(|mutation| match mutation {
                PropertyMutation::SetContainerTitle { value }
                | PropertyMutation::SetTrackName { value, .. } => Some(value.as_str()),
                _ => None,
            }).collect::<Vec<_>>()
        }).collect::<Vec<_>>();

        assert_eq!(values, [["Winter Is Coming", "Winter Is Coming"], ["The Kingsroad", "The Kingsroad"]]);
    }

    #[test]
    fn metadata_audio_name_omits_unknown_channels() {
        let mut audio = track(1, TrackKind::Audio, None);
        audio.codec = "opus".to_owned();

        assert_eq!(metadata_track_name(&audio, Some("jpn")), "Opus Japanese");
    }
}
