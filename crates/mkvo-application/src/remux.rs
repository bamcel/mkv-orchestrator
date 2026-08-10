use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use mkvo_domain::{
    ExternalSubtitle, IdempotencyKey, MediaFile, OperationKind, OperationPlan, PlanConflict,
    PlanConflictKind, PlanContext, RemuxMode, RemuxPlan, RemuxPlanItem, RemuxPlanPayload,
    ResourceClaim, ToolFingerprints, TrackExtraction, TrackKind,
};
use serde::{Deserialize, Serialize};

use crate::{
    ApplicationError, ApplicationResult,
    paths::{path_contains, path_key},
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemuxPlanRequest {
    pub mode: RemuxMode,
    pub files: Vec<MediaFile>,
    pub options: RemuxOptions,
    #[serde(default)]
    pub external_subtitles: BTreeMap<PathBuf, Vec<ExternalSubtitle>>,
    #[serde(default)]
    pub extractions: BTreeMap<PathBuf, Vec<TrackExtraction>>,
    #[serde(default)]
    pub existing_paths: BTreeSet<PathBuf>,
    #[serde(default)]
    pub authorized_roots: Vec<PathBuf>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemuxOptions {
    #[serde(default)]
    pub filter_audio_languages: bool,
    #[serde(default)]
    pub keep_audio_languages: BTreeSet<String>,
    #[serde(default)]
    pub filter_subtitle_languages: bool,
    #[serde(default)]
    pub keep_subtitle_languages: BTreeSet<String>,
    #[serde(default)]
    pub remove_track_ids: BTreeSet<u64>,
    #[serde(default = "default_true")]
    pub preserve_chapters: bool,
    #[serde(default = "default_true")]
    pub preserve_attachments: bool,
    #[serde(default)]
    pub delete_source_after_success: bool,
    #[serde(default)]
    pub delete_external_subtitles_after_success: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for RemuxOptions {
    fn default() -> Self {
        Self {
            filter_audio_languages: false,
            keep_audio_languages: BTreeSet::new(),
            filter_subtitle_languages: false,
            keep_subtitle_languages: BTreeSet::new(),
            remove_track_ids: BTreeSet::new(),
            preserve_chapters: true,
            preserve_attachments: true,
            delete_source_after_success: false,
            delete_external_subtitles_after_success: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RemuxPlanner;

impl RemuxPlanner {
    pub fn build_plan(&self, request: RemuxPlanRequest) -> ApplicationResult<RemuxPlan> {
        if request.files.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "remux preview requires at least one file".to_owned(),
            ));
        }
        if request.expires_in_seconds == 0 {
            return Err(ApplicationError::InvalidRequest(
                "remux plan expiration must be positive".to_owned(),
            ));
        }
        let existing: BTreeSet<_> = request
            .existing_paths
            .iter()
            .map(|path| path_key(path))
            .collect();
        let mut items = Vec::with_capacity(request.files.len());

        for file in &request.files {
            let extension = file.extension().to_ascii_lowercase();
            let final_output = match request.mode {
                RemuxMode::ConvertToMkv => file.path.with_extension("mkv"),
                RemuxMode::ExtractSubtitles => file
                    .path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_owned(),
                RemuxMode::Remux | RemuxMode::MuxSubtitles => file.path.clone(),
            };
            let temporary_output = temporary_output_for(file, request.mode);
            let external_subtitles = request
                .external_subtitles
                .get(&file.path)
                .cloned()
                .unwrap_or_default();
            let extract_tracks = request
                .extractions
                .get(&file.path)
                .cloned()
                .unwrap_or_default();
            let selected_track_ids = selected_tracks(file, &request.options);
            let mut conflicts = Vec::new();

            match request.mode {
                RemuxMode::ConvertToMkv if matches!(extension.as_str(), "mkv" | "mka" | "webm") => {
                    conflicts.push(PlanConflict::informational(
                        PlanConflictKind::NoChange,
                        "Source is already a Matroska container",
                        Some(file.path.clone()),
                    ));
                }
                RemuxMode::ConvertToMkv if !matches!(extension.as_str(), "mp4" | "m4v") => {
                    conflicts.push(PlanConflict::blocking(
                        PlanConflictKind::UnsupportedInput,
                        "MP4-to-MKV conversion requires an MP4 or M4V source",
                        Some(file.path.clone()),
                    ));
                }
                RemuxMode::Remux | RemuxMode::MuxSubtitles | RemuxMode::ExtractSubtitles
                    if !matches!(extension.as_str(), "mkv" | "mka" | "webm") =>
                {
                    conflicts.push(PlanConflict::blocking(
                        PlanConflictKind::UnsupportedInput,
                        "This operation requires a Matroska/WebM source",
                        Some(file.path.clone()),
                    ));
                }
                _ => {}
            }
            if selected_track_ids.is_empty()
                && matches!(request.mode, RemuxMode::Remux | RemuxMode::MuxSubtitles)
            {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::InvalidSelection,
                    "The selected filters remove every track",
                    Some(file.path.clone()),
                ));
            }
            if request.mode == RemuxMode::MuxSubtitles && external_subtitles.is_empty() {
                conflicts.push(PlanConflict::informational(
                    PlanConflictKind::NoChange,
                    "No matching external subtitles were supplied",
                    Some(file.path.clone()),
                ));
            }
            if request.mode == RemuxMode::ExtractSubtitles && extract_tracks.is_empty() {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::InvalidSelection,
                    "No subtitle tracks were selected for extraction",
                    Some(file.path.clone()),
                ));
            }
            if request.mode == RemuxMode::ConvertToMkv
                && existing.contains(&path_key(&final_output))
            {
                conflicts.push(PlanConflict::blocking(
                    PlanConflictKind::ExistingTarget,
                    "Conversion target already exists",
                    Some(final_output.clone()),
                ));
            }
            if request.mode == RemuxMode::Remux
                && selected_track_ids.len() == file.tracks.len()
                && request.options.remove_track_ids.is_empty()
                && !request.options.filter_audio_languages
                && !request.options.filter_subtitle_languages
            {
                conflicts.push(PlanConflict::informational(
                    PlanConflictKind::NoChange,
                    "No track changes are requested",
                    Some(file.path.clone()),
                ));
            }
            for subtitle in &external_subtitles {
                if !existing.contains(&path_key(&subtitle.path)) {
                    conflicts.push(PlanConflict::blocking(
                        PlanConflictKind::MissingSource,
                        "External subtitle file does not exist",
                        Some(subtitle.path.clone()),
                    ));
                }
            }
            for extraction in &extract_tracks {
                if existing.contains(&path_key(&extraction.output)) {
                    conflicts.push(PlanConflict::blocking(
                        PlanConflictKind::ExistingTarget,
                        "Subtitle extraction target already exists",
                        Some(extraction.output.clone()),
                    ));
                }
            }
            if !request.authorized_roots.is_empty() {
                let outputs = std::iter::once(&final_output)
                    .chain(extract_tracks.iter().map(|track| &track.output));
                for output in outputs {
                    if !request
                        .authorized_roots
                        .iter()
                        .any(|root| path_contains(root, output))
                    {
                        conflicts.push(PlanConflict::blocking(
                            PlanConflictKind::UnauthorizedPath,
                            "Output is outside the authorized roots",
                            Some(output.clone()),
                        ));
                    }
                }
            }

            items.push(RemuxPlanItem {
                source: file.path.clone(),
                source_fingerprint: file.fingerprint.clone(),
                temporary_output,
                final_output,
                mode: request.mode,
                selected_track_ids,
                external_subtitles,
                extract_tracks,
                preserve_chapters: request.options.preserve_chapters,
                preserve_attachments: request.options.preserve_attachments,
                delete_source_after_success: request.options.delete_source_after_success,
                delete_external_subtitles_after_success: request
                    .options
                    .delete_external_subtitles_after_success,
                conflicts,
            });
        }

        mark_duplicate_outputs(&mut items);
        let payload = RemuxPlanPayload {
            mode: request.mode,
            items,
        };
        let resources = payload
            .items
            .iter()
            .flat_map(|item| {
                std::iter::once(ResourceClaim::write(item.source.clone()))
                    .chain(std::iter::once(ResourceClaim::write(
                        item.temporary_output.clone(),
                    )))
                    .chain(std::iter::once(ResourceClaim::write(
                        item.final_output.clone(),
                    )))
                    .chain(
                        item.external_subtitles
                            .iter()
                            .map(|subtitle| ResourceClaim::read(subtitle.path.clone())),
                    )
                    .chain(
                        item.extract_tracks
                            .iter()
                            .map(|track| ResourceClaim::write(track.output.clone())),
                    )
            })
            .collect();
        let context = PlanContext {
            settings_fingerprint: request.settings_fingerprint.clone(),
            tool_fingerprints: request.tool_fingerprints.clone(),
            input_fingerprints: request
                .files
                .iter()
                .map(|file| file.fingerprint.clone())
                .collect(),
            resources,
            attributes: BTreeMap::new(),
        };
        let kind = match request.mode {
            RemuxMode::Remux => OperationKind::Remux,
            RemuxMode::ConvertToMkv => OperationKind::ConvertToMkv,
            RemuxMode::MuxSubtitles => OperationKind::MuxSubtitles,
            RemuxMode::ExtractSubtitles => OperationKind::ExtractSubtitles,
        };
        let now = Utc::now();
        let ttl = i64::try_from(request.expires_in_seconds).unwrap_or(i64::MAX);
        OperationPlan::new(
            kind,
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

fn selected_tracks(file: &MediaFile, options: &RemuxOptions) -> Vec<u64> {
    file.tracks
        .iter()
        .filter(|track| !options.remove_track_ids.contains(&track.mkvmerge_id))
        .filter(|track| match track.kind {
            TrackKind::Audio if options.filter_audio_languages => options
                .keep_audio_languages
                .iter()
                .any(|language| language.eq_ignore_ascii_case(track.language_or_undetermined())),
            TrackKind::Subtitle if options.filter_subtitle_languages => options
                .keep_subtitle_languages
                .iter()
                .any(|language| language.eq_ignore_ascii_case(track.language_or_undetermined())),
            _ => true,
        })
        .map(|track| track.mkvmerge_id)
        .collect()
}

fn temporary_output_for(file: &MediaFile, mode: RemuxMode) -> PathBuf {
    let stem = file
        .path
        .file_stem()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    let suffix = match mode {
        RemuxMode::ExtractSubtitles => "extract",
        RemuxMode::ConvertToMkv | RemuxMode::MuxSubtitles | RemuxMode::Remux => "remux",
    };
    file.path
        .with_file_name(format!("{stem}.mkvo-{suffix}.tmp.mkv"))
}

fn mark_duplicate_outputs(items: &mut [RemuxPlanItem]) {
    let mut outputs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        if item.mode == RemuxMode::ConvertToMkv {
            outputs
                .entry(path_key(&item.final_output))
                .or_default()
                .push(index);
        }
        for extraction in &item.extract_tracks {
            outputs
                .entry(path_key(&extraction.output))
                .or_default()
                .push(index);
        }
    }
    for indices in outputs.values().filter(|indices| indices.len() > 1) {
        for &index in indices {
            items[index].conflicts.push(PlanConflict::blocking(
                PlanConflictKind::DuplicateTarget,
                "Multiple actions write the same output",
                Some(items[index].final_output.clone()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use mkvo_domain::{ContainerMetadata, FileFingerprint, MediaStatus, MediaTrack};

    use super::*;
    use serde_json::Value;

    fn media(path: &str) -> MediaFile {
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: FileFingerprint {
                path: PathBuf::from(path),
                size_bytes: 10,
                modified_at: Utc::now(),
                quick_hash: None,
            },
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

    #[test]
    fn conversion_targets_mkv_and_captures_cleanup_policy() {
        let request = RemuxPlanRequest {
            mode: RemuxMode::ConvertToMkv,
            files: vec![media("movie.mp4")],
            options: RemuxOptions {
                delete_source_after_success: true,
                ..RemuxOptions::default()
            },
            external_subtitles: BTreeMap::new(),
            extractions: BTreeMap::new(),
            existing_paths: BTreeSet::new(),
            authorized_roots: Vec::new(),
            settings_fingerprint: "settings".to_owned(),
            tool_fingerprints: BTreeMap::new(),
            expires_in_seconds: 60,
            idempotency_key: IdempotencyKey::generate(),
        };
        let plan = RemuxPlanner.build_plan(request).unwrap();
        assert_eq!(
            plan.payload.items[0].final_output,
            PathBuf::from("movie.mkv")
        );
        assert!(plan.payload.items[0].delete_source_after_success);
        assert!(plan.payload.items[0].can_apply());
    }

    #[test]
    fn unfiltered_remux_is_no_change() {
        let request = RemuxPlanRequest {
            mode: RemuxMode::Remux,
            files: vec![media("movie.mkv")],
            options: RemuxOptions::default(),
            external_subtitles: BTreeMap::new(),
            extractions: BTreeMap::new(),
            existing_paths: BTreeSet::new(),
            authorized_roots: Vec::new(),
            settings_fingerprint: "settings".to_owned(),
            tool_fingerprints: BTreeMap::new(),
            expires_in_seconds: 60,
            idempotency_key: IdempotencyKey::generate(),
        };
        let plan = RemuxPlanner.build_plan(request).unwrap();
        assert!(
            plan.payload.items[0]
                .conflicts
                .iter()
                .any(|conflict| conflict.kind == PlanConflictKind::NoChange)
        );
    }

    fn fixture_kind(value: &str) -> TrackKind {
        match value.to_ascii_lowercase().as_str() {
            "video" => TrackKind::Video,
            "audio" => TrackKind::Audio,
            "subtitle" | "subtitles" => TrackKind::Subtitle,
            _ => TrackKind::Other,
        }
    }

    fn fixture_media(value: &Value) -> MediaFile {
        let path = value["filePath"].as_str().expect("file path");
        let tracks = value["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|track| {
                let id = track["mkvMergeId"].as_u64().expect("track id");
                MediaTrack {
                    mkvmerge_id: id,
                    propedit_track_number: u32::try_from(id + 1).expect("track number"),
                    kind: fixture_kind(track["type"].as_str().unwrap_or_default()),
                    codec: track["codec"].as_str().unwrap_or_default().to_owned(),
                    codec_id: None,
                    language: track["language"].as_str().map(str::to_owned),
                    name: track["name"].as_str().map(str::to_owned),
                    resolution: None,
                    bit_depth: None,
                    hdr: None,
                    channels: None,
                    sampling_frequency_hz: None,
                    default: false,
                    forced: false,
                    enabled: true,
                }
            })
            .collect();
        MediaFile {
            path: PathBuf::from(path),
            original_file_name: None,
            watch_root: None,
            relative_path: None,
            fingerprint: FileFingerprint {
                path: PathBuf::from(path),
                size_bytes: 1,
                modified_at: Utc::now(),
                quick_hash: None,
            },
            container: ContainerMetadata::default(),
            tracks,
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    fn strings(value: &Value, key: &str) -> BTreeSet<String> {
        value[key]
            .as_str()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn portable(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn executes_every_remux_planner_fixture_case() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/parity-fixtures/remux-plans.json"
        ))
        .expect("remux fixture JSON");
        let cases = fixture["cases"].as_array().expect("fixture cases");
        assert_eq!(cases.len(), 4, "fixture case count changed");

        for case in cases {
            let id = case["id"].as_str().expect("case id");
            let input = &case["input"];
            let files = input["files"]
                .as_array()
                .expect("files")
                .iter()
                .filter(|file| file["selected"].as_bool().unwrap_or(true))
                .map(fixture_media)
                .collect::<Vec<_>>();
            let existing_paths = case["virtualFilesystem"]["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect::<BTreeSet<_>>();
            let mode = match id {
                "sidecar-discovery-tagging-and-existing-track-skip" => RemuxMode::MuxSubtitles,
                "subtitle-extraction-skips-existing-output" => RemuxMode::ExtractSubtitles,
                "mp4-lossless-container-conversion" => RemuxMode::ConvertToMkv,
                _ => RemuxMode::Remux,
            };
            let mut external_subtitles = BTreeMap::new();
            if mode == RemuxMode::MuxSubtitles {
                let source = &files[0].path;
                let source_stem = source.file_stem().unwrap().to_string_lossy();
                let subtitles = case["expected"]["actions"][0]["externalSubtitleFilePaths"]
                    .as_array()
                    .expect("external paths")
                    .iter()
                    .map(|value| {
                        let path = PathBuf::from(value.as_str().expect("external path"));
                        let sidecar_stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                        let tail = sidecar_stem
                            .strip_prefix(source_stem.as_ref())
                            .unwrap_or_default()
                            .trim_start_matches('.')
                            .to_owned();
                        let mut tags = tail.splitn(2, '.');
                        let language = tags.next().filter(|value| !value.is_empty()).map_or_else(
                            || {
                                input["externalSubtitleLanguage"]
                                    .as_str()
                                    .unwrap_or("und")
                                    .to_owned()
                            },
                            str::to_owned,
                        );
                        ExternalSubtitle {
                            path,
                            language,
                            name: tags.next().map(str::to_owned),
                            default: false,
                            forced: false,
                        }
                    })
                    .collect();
                external_subtitles.insert(source.clone(), subtitles);
            }
            let mut extractions = BTreeMap::new();
            if mode == RemuxMode::ExtractSubtitles {
                let output = PathBuf::from(
                    case["expected"]["actions"][0]["tempOutputPath"]
                        .as_str()
                        .expect("extraction output"),
                );
                extractions.insert(
                    files[0].path.clone(),
                    vec![TrackExtraction {
                        track_id: 5,
                        kind: TrackKind::Subtitle,
                        output,
                    }],
                );
            }
            let options = RemuxOptions {
                filter_audio_languages: input["removeUnwantedAudioLanguages"]
                    .as_bool()
                    .unwrap_or(false),
                keep_audio_languages: strings(input, "keepAudioLanguages"),
                filter_subtitle_languages: input["removeUnwantedSubtitleLanguages"]
                    .as_bool()
                    .unwrap_or(false),
                keep_subtitle_languages: strings(input, "keepSubtitleLanguages"),
                remove_track_ids: strings(input, "removeTrackIdsText")
                    .into_iter()
                    .filter_map(|value| value.parse().ok())
                    .collect(),
                preserve_chapters: input["preserveChapters"].as_bool().unwrap_or(true),
                preserve_attachments: input["preserveAttachments"].as_bool().unwrap_or(true),
                delete_source_after_success: input["deleteSourceAfterSuccess"]
                    .as_bool()
                    .unwrap_or(false),
                delete_external_subtitles_after_success: !input["preserveExternalSubtitleFiles"]
                    .as_bool()
                    .unwrap_or(true),
            };
            let plan = RemuxPlanner
                .build_plan(RemuxPlanRequest {
                    mode,
                    files,
                    options,
                    external_subtitles,
                    extractions,
                    existing_paths,
                    authorized_roots: Vec::new(),
                    settings_fingerprint: "fixture-settings".to_owned(),
                    tool_fingerprints: BTreeMap::new(),
                    expires_in_seconds: 60,
                    idempotency_key: IdempotencyKey::generate(),
                })
                .unwrap_or_else(|error| panic!("fixture case `{id}`: {error}"));

            match id {
                "language-and-track-id-selection" => {
                    let item = &plan.payload.items[0];
                    assert_eq!(item.selected_track_ids, [0, 1, 4]);
                    assert!(!item.preserve_chapters);
                    assert!(!item.preserve_attachments);
                    assert_eq!(
                        portable(&item.temporary_output),
                        case["expected"]["actions"][0]["tempOutputPath"]
                            .as_str()
                            .unwrap()
                    );
                }
                "sidecar-discovery-tagging-and-existing-track-skip" => {
                    let item = &plan.payload.items[0];
                    assert_eq!(item.external_subtitles.len(), 2);
                    assert_eq!(item.external_subtitles[0].language, "und");
                    assert_eq!(item.external_subtitles[1].language, "jpn");
                    assert_eq!(item.external_subtitles[1].name.as_deref(), Some("Dialogue"));
                    assert!(item.can_apply());
                }
                "subtitle-extraction-skips-existing-output" => {
                    let item = &plan.payload.items[0];
                    assert_eq!(item.extract_tracks.len(), 1);
                    assert_eq!(item.extract_tracks[0].track_id, 5);
                    assert!(item.can_apply());
                }
                "mp4-lossless-container-conversion" => {
                    assert_eq!(plan.payload.items.len(), 2);
                    let conversion = &plan.payload.items[0];
                    assert_eq!(
                        portable(&conversion.final_output),
                        "${ROOT}/Movies/Example Movie.mkv"
                    );
                    assert_eq!(
                        portable(&conversion.temporary_output),
                        "${ROOT}/Movies/Example Movie.mkvo-remux.tmp.mkv"
                    );
                    assert!(conversion.delete_source_after_success);
                    assert!(conversion.can_apply());
                    assert!(
                        plan.payload.items[1]
                            .conflicts
                            .iter()
                            .any(|conflict| conflict.kind == PlanConflictKind::NoChange)
                    );
                    assert!(!plan.payload.items[1].can_apply());
                }
                _ => unreachable!("known fixture case"),
            }
        }
    }
}
