use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mkvo_domain::{
    LibraryAudit, LibraryAuditGroup, LibraryAuditSummary, LibraryIssue, LibraryIssueKind,
    LibraryStandard, MediaFile, TrackKind, natural_compare,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct LibraryAuditService;

impl LibraryAuditService {
    #[must_use]
    pub fn build(
        &self,
        root: impl Into<PathBuf>,
        files: &[MediaFile],
        uncached_paths: &[PathBuf],
    ) -> LibraryAudit {
        let root = root.into();
        let mut grouped: BTreeMap<AuditGroupKey, Vec<&MediaFile>> = BTreeMap::new();
        for file in files {
            grouped
                .entry(group_key(&root, &file.path))
                .or_default()
                .push(file);
        }

        let mut groups = grouped
            .into_iter()
            .map(|(key, mut files)| {
                files.sort_by_key(|file| file.file_name().to_lowercase());
                build_group(&root, key, &files)
            })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            left.show_name
                .to_lowercase()
                .cmp(&right.show_name.to_lowercase())
                .then_with(|| natural_compare(&left.season_folder, &right.season_folder))
        });
        let summary = LibraryAuditSummary {
            shows: groups
                .iter()
                .map(|group| group.show_name.to_lowercase())
                .collect::<BTreeSet<_>>()
                .len(),
            season_folders: groups.len(),
            files: files.len().saturating_add(uncached_paths.len()),
            issue_groups: groups.iter().filter(|group| group.has_issues()).count(),
            uncached_files: uncached_paths.len(),
        };
        LibraryAudit {
            root,
            groups,
            summary,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AuditGroupKey {
    show_name: String,
    season_folder: String,
    relative_folder: PathBuf,
    sort_season: u32,
}

fn group_key(root: &Path, file: &Path) -> AuditGroupKey {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let directory = relative.parent().unwrap_or_else(|| Path::new(""));
    let segments: Vec<_> = directory
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    if segments.is_empty() {
        return AuditGroupKey {
            show_name: "root".to_owned(),
            season_folder: "root".to_owned(),
            relative_folder: PathBuf::new(),
            sort_season: 0,
        };
    }
    if let Some((index, season)) = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, value)| parse_season_folder(value).map(|season| (index, season)))
    {
        let show_name = if index > 0 {
            segments[index - 1].clone()
        } else {
            segments[0].clone()
        };
        let relative_folder = segments[..=index].iter().collect();
        return AuditGroupKey {
            show_name,
            season_folder: segments[index].clone(),
            relative_folder,
            sort_season: season,
        };
    }
    let folder = segments
        .last()
        .cloned()
        .unwrap_or_else(|| "root".to_owned());
    AuditGroupKey {
        show_name: folder,
        season_folder: "movie/single folder".to_owned(),
        relative_folder: directory.to_owned(),
        sort_season: 9_999,
    }
}

fn build_group(root: &Path, key: AuditGroupKey, files: &[&MediaFile]) -> LibraryAuditGroup {
    let videos: Vec<_> = files.iter().map(|file| video_signature(file)).collect();
    let audios: Vec<_> = files
        .iter()
        .map(|file| track_signature(file, TrackKind::Audio))
        .collect();
    let subtitles: Vec<_> = files
        .iter()
        .map(|file| track_signature(file, TrackKind::Subtitle))
        .collect();
    let standard = LibraryStandard {
        video: dominant(&videos),
        audio: dominant(&audios),
        subtitles: dominant(&subtitles),
    };
    let mut issues = Vec::new();
    let mut issue_paths = BTreeSet::new();
    for file in files {
        add_mismatch(
            &mut issues,
            &mut issue_paths,
            file,
            LibraryIssueKind::VideoMismatch,
            "video",
            &video_signature(file),
            &standard.video,
        );
        add_mismatch(
            &mut issues,
            &mut issue_paths,
            file,
            LibraryIssueKind::AudioMismatch,
            "audio",
            &track_signature(file, TrackKind::Audio),
            &standard.audio,
        );
        add_mismatch(
            &mut issues,
            &mut issue_paths,
            file,
            LibraryIssueKind::SubtitleMismatch,
            "subtitles",
            &track_signature(file, TrackKind::Subtitle),
            &standard.subtitles,
        );
    }
    add_episode_issues(&mut issues, &mut issue_paths, files);

    let template_file_path = files
        .iter()
        .find(|file| {
            video_signature(file) == standard.video
                && track_signature(file, TrackKind::Audio) == standard.audio
                && track_signature(file, TrackKind::Subtitle) == standard.subtitles
        })
        .or_else(|| files.first())
        .map(|file| file.path.clone());
    LibraryAuditGroup {
        watch_root: root.to_owned(),
        show_name: key.show_name,
        season_folder: key.season_folder,
        relative_folder: key.relative_folder,
        all_file_paths: files.iter().map(|file| file.path.clone()).collect(),
        issue_file_paths: issue_paths.into_iter().collect(),
        template_file_path,
        standard,
        issues,
    }
}

fn add_mismatch(
    issues: &mut Vec<LibraryIssue>,
    issue_paths: &mut BTreeSet<PathBuf>,
    file: &MediaFile,
    kind: LibraryIssueKind,
    label: &str,
    actual: &str,
    expected: &str,
) {
    if expected.eq_ignore_ascii_case("unknown") || actual.eq_ignore_ascii_case(expected) {
        return;
    }
    issue_paths.insert(file.path.clone());
    issues.push(LibraryIssue {
        kind,
        message: format!(
            "{}: {label} mismatch ({} vs {})",
            file.file_name(),
            display_signature(actual),
            display_signature(expected)
        ),
        path: Some(file.path.clone()),
        related_paths: Vec::new(),
    });
}

fn add_episode_issues(
    issues: &mut Vec<LibraryIssue>,
    issue_paths: &mut BTreeSet<PathBuf>,
    files: &[&MediaFile],
) {
    let numbered: Vec<_> = files
        .iter()
        .filter_map(|file| parse_episode_number(&file.file_name()).map(|number| (number, *file)))
        .collect();
    if numbered.len() < 2 {
        return;
    }
    let mut by_episode: BTreeMap<u32, Vec<PathBuf>> = BTreeMap::new();
    for (episode, file) in &numbered {
        by_episode
            .entry(*episode)
            .or_default()
            .push(file.path.clone());
    }
    let duplicate_numbers: Vec<_> = by_episode
        .iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(number, _)| *number)
        .collect();
    if !duplicate_numbers.is_empty() {
        let related_paths: Vec<_> = duplicate_numbers
            .iter()
            .flat_map(|number| by_episode[number].clone())
            .collect();
        issue_paths.extend(related_paths.iter().cloned());
        issues.push(LibraryIssue {
            kind: LibraryIssueKind::DuplicateEpisode,
            message: format!(
                "duplicate episode numbers: {}",
                duplicate_numbers
                    .iter()
                    .map(|number| format!("{number:02}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            path: None,
            related_paths,
        });
    }

    let numbers: Vec<_> = by_episode.keys().copied().collect();
    if numbers.len() < 3 {
        return;
    }
    let first = numbers[0];
    let last = *numbers.last().unwrap_or(&first);
    let known: BTreeSet<_> = numbers.into_iter().collect();
    let missing: Vec<_> = (first..=last)
        .filter(|number| !known.contains(number))
        .take(20)
        .collect();
    if !missing.is_empty() {
        issues.push(LibraryIssue {
            kind: LibraryIssueKind::PossibleMissingEpisode,
            message: format!(
                "possible missing episode numbers: {}",
                missing
                    .iter()
                    .map(|number| format!("{number:02}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            path: None,
            related_paths: Vec::new(),
        });
    }
}

fn video_signature(file: &MediaFile) -> String {
    file.tracks
        .iter()
        .find(|track| track.kind == TrackKind::Video)
        .map_or_else(
            || "unknown".to_owned(),
            |track| {
                [
                    track.resolution.map(|value| value.to_string()),
                    Some(clean(&track.codec, "unknown")),
                    track.bit_depth.map(|value| format!("{value}bit")),
                    track.hdr.clone().filter(|value| !value.trim().is_empty()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
            },
        )
}

fn track_signature(file: &MediaFile, kind: TrackKind) -> String {
    let mut tracks = file
        .tracks
        .iter()
        .filter(|track| track.kind == kind)
        .map(|track| {
            let mut value = format!(
                "{}:{}",
                track.language_or_undetermined(),
                clean(&track.codec, "unknown")
            );
            if track.forced {
                value.push_str(":forced");
            }
            if let Some(name) = track.name.as_deref().filter(|name| !name.trim().is_empty()) {
                value.push_str(&format!(" - \"{}\"", name.trim()));
            }
            value
        })
        .collect::<Vec<_>>();
    tracks.sort_by_key(|value| value.to_lowercase());
    if tracks.is_empty() {
        "none".to_owned()
    } else {
        tracks.join(", ")
    }
}

fn dominant(values: &[String]) -> String {
    let mut counts: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for value in values {
        let display = clean(value, "unknown");
        let entry = counts
            .entry(display.to_lowercase())
            .or_insert_with(|| (0, display));
        entry.0 += 1;
    }
    counts
        .into_values()
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
        .map_or_else(|| "unknown".to_owned(), |(_, value)| value)
}

fn clean(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn display_signature(value: &str) -> &str {
    if value.trim().is_empty() {
        "unknown"
    } else {
        value
    }
}

fn parse_season_folder(value: &str) -> Option<u32> {
    let compact: String = value
        .to_ascii_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let digits = compact
        .strip_prefix("season")
        .or_else(|| compact.strip_prefix('s'))?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[must_use]
pub fn parse_episode_number(file_name: &str) -> Option<u32> {
    let lower = file_name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b's' {
            continue;
        }
        let mut cursor = index + 1;
        let season_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - season_start < 2 {
            cursor += 1;
        }
        if cursor == season_start || cursor >= bytes.len() || bytes[cursor] != b'e' {
            continue;
        }
        cursor += 1;
        let episode_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - episode_start < 3 {
            cursor += 1;
        }
        if cursor > episode_start {
            return lower[episode_start..cursor].parse().ok();
        }
    }
    for marker in ["episode", "ep", "e"] {
        let mut offset = 0;
        while let Some(found) = lower[offset..].find(marker) {
            let start = offset + found;
            let boundary_before = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
            let mut cursor = start + marker.len();
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let digit_start = cursor;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - digit_start < 3
            {
                cursor += 1;
            }
            let boundary_after = cursor == bytes.len() || !bytes[cursor].is_ascii_alphanumeric();
            if boundary_before && boundary_after && cursor > digit_start {
                return lower[digit_start..cursor].parse().ok();
            }
            offset = start + marker.len();
        }
    }
    // Anime release names commonly put the episode in its own numeric bracket,
    // for example `[VCB-Studio] Show [01][Ma10p_1080p].mkv`. This is checked
    // after explicit SxxExx / Episode markers so a deliberate marker always
    // wins. Requiring the entire bracket to be 1-3 digits avoids treating
    // resolution, codec, bit-depth, or CRC tags as episode numbers.
    let mut cursor = 0usize;
    while let Some(open_offset) = lower[cursor..].find('[') {
        let open = cursor + open_offset;
        let content_start = open + 1;
        let Some(close_offset) = lower[content_start..].find(']') else {
            break;
        };
        let close = content_start + close_offset;
        let content = &lower[content_start..close];
        if !content.is_empty()
            && content.len() <= 3
            && content.bytes().all(|byte| byte.is_ascii_digit())
        {
            return content.parse().ok();
        }
        cursor = close + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use mkvo_domain::{
        ContainerMetadata, FileFingerprint, MediaStatus, MediaTrack, VideoResolution,
    };

    use super::*;
    use serde_json::Value;

    fn media(path: &str, audio_language: &str) -> MediaFile {
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
            tracks: vec![
                track(0, TrackKind::Video, "und", "AVC"),
                track(1, TrackKind::Audio, audio_language, "AAC"),
            ],
            attachments: Vec::new(),
            episode: None,
            provider_match: None,
            status: MediaStatus::Ready,
        }
    }

    fn track(id: u64, kind: TrackKind, language: &str, codec: &str) -> MediaTrack {
        MediaTrack {
            mkvmerge_id: id,
            propedit_track_number: u32::try_from(id + 1).unwrap(),
            kind,
            codec: codec.to_owned(),
            codec_id: None,
            language: Some(language.to_owned()),
            name: None,
            resolution: (kind == TrackKind::Video).then_some(VideoResolution {
                width: 1_920,
                height: 1_080,
            }),
            bit_depth: None,
            hdr: None,
            channels: None,
            sampling_frequency_hz: None,
            default: true,
            forced: false,
            enabled: true,
        }
    }

    #[test]
    fn finds_common_episode_patterns() {
        assert_eq!(parse_episode_number("Show.S02E013.mkv"), Some(13));
        assert_eq!(parse_episode_number("Show - Episode 007.mkv"), Some(7));
        assert_eq!(
            parse_episode_number("[VCB-Studio] 7th Time Loop [01][Ma10p_1080p][x265_flac].mkv"),
            Some(1)
        );
        assert_eq!(
            parse_episode_number("[VCB-Studio] Show [Ma10p_1080p][A1B2C3D4].mkv"),
            None
        );
    }

    #[test]
    fn audit_reports_track_mismatch_and_missing_episode() {
        let files = vec![
            media("root/Show/Season 1/Show S01E01.mkv", "eng"),
            media("root/Show/Season 1/Show S01E03.mkv", "eng"),
            media("root/Show/Season 1/Show S01E04.mkv", "jpn"),
        ];
        let audit = LibraryAuditService.build("root", &files, &[]);
        assert_eq!(audit.groups.len(), 1);
        assert_eq!(audit.summary.issue_groups, 1);
        assert!(
            audit.groups[0]
                .issues
                .iter()
                .any(|issue| issue.kind == LibraryIssueKind::AudioMismatch)
        );
        assert!(
            audit.groups[0]
                .issues
                .iter()
                .any(|issue| issue.kind == LibraryIssueKind::PossibleMissingEpisode)
        );
    }

    fn fixture_track(id: u64, value: &Value) -> MediaTrack {
        let kind = match value["type"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "video" => TrackKind::Video,
            "audio" => TrackKind::Audio,
            "subtitle" | "subtitles" => TrackKind::Subtitle,
            _ => TrackKind::Other,
        };
        MediaTrack {
            mkvmerge_id: id,
            propedit_track_number: u32::try_from(id + 1).expect("track number fits u32"),
            kind,
            codec: value["codec"].as_str().unwrap_or_default().to_owned(),
            codec_id: None,
            language: value["language"].as_str().map(str::to_owned),
            name: value["name"].as_str().map(str::to_owned),
            resolution: None,
            bit_depth: None,
            hdr: None,
            channels: None,
            sampling_frequency_hz: None,
            default: value["default"].as_bool().unwrap_or(false),
            forced: value["forced"].as_bool().unwrap_or(false),
            enabled: true,
        }
    }

    fn fixture_media(value: &Value) -> MediaFile {
        let path = value["filePath"].as_str().expect("file path");
        let metadata = &value["metadata"];
        let resolution = metadata["resolution"]
            .as_str()
            .and_then(|value| value.split_once('x'))
            .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
            .map(|(width, height)| VideoResolution { width, height });
        let bit_depth = metadata["bitDepth"]
            .as_str()
            .and_then(|value| value.trim_end_matches("bit").parse::<u8>().ok());
        let mut tracks = vec![MediaTrack {
            mkvmerge_id: 0,
            propedit_track_number: 1,
            kind: TrackKind::Video,
            codec: metadata["codec"].as_str().unwrap_or_default().to_owned(),
            codec_id: None,
            language: Some("und".to_owned()),
            name: None,
            resolution,
            bit_depth,
            hdr: metadata["hdr"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            channels: None,
            sampling_frequency_hz: None,
            default: true,
            forced: false,
            enabled: true,
        }];
        tracks.extend(
            value["tracks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|track| fixture_track(track["mkvMergeId"].as_u64().unwrap(), track)),
        );
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

    fn portable(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn executes_library_audit_fixture_case() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/parity-fixtures/library-audit.json"
        ))
        .expect("library fixture JSON");
        let cases = fixture["cases"].as_array().expect("fixture cases");
        assert_eq!(cases.len(), 1, "fixture case count changed");
        let case = &cases[0];
        let input = &case["input"];
        let cached = input["cache"]
            .as_array()
            .expect("cache")
            .iter()
            .map(fixture_media)
            .collect::<Vec<_>>();
        let cached_paths = cached
            .iter()
            .map(|file| portable(&file.path))
            .collect::<BTreeSet<_>>();
        let uncached = input["enumeratedFiles"]
            .as_array()
            .expect("enumerated files")
            .iter()
            .filter_map(Value::as_str)
            .filter(|path| !cached_paths.contains(*path))
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        let audit = LibraryAuditService.build(
            input["watchRoot"].as_str().expect("watch root"),
            &cached,
            &uncached,
        );
        let expected = &case["expected"];
        assert_eq!(
            audit.summary.shows,
            expected["summary"]["shows"].as_u64().unwrap() as usize
        );
        assert_eq!(
            audit.summary.season_folders,
            expected["summary"]["seasonFolders"].as_u64().unwrap() as usize
        );
        assert_eq!(
            audit.summary.files,
            expected["summary"]["files"].as_u64().unwrap() as usize
        );
        assert_eq!(
            audit.summary.issue_groups,
            expected["summary"]["issueGroups"].as_u64().unwrap() as usize
        );
        assert_eq!(
            audit.summary.uncached_files,
            expected["summary"]["uncachedFiles"].as_u64().unwrap() as usize
        );
        let expected_groups = expected["groups"].as_array().expect("expected groups");
        assert_eq!(audit.groups.len(), expected_groups.len());
        for (actual, expected) in audit.groups.iter().zip(expected_groups) {
            assert_eq!(actual.show_name, expected["showName"].as_str().unwrap());
            assert_eq!(
                actual.season_folder,
                expected["seasonFolder"].as_str().unwrap()
            );
            assert_eq!(
                portable(&actual.relative_folder),
                expected["relativeFolder"].as_str().unwrap()
            );
            assert_eq!(
                actual.standard.video,
                expected["standardVideo"].as_str().unwrap()
            );
            assert_eq!(
                actual.standard.audio,
                expected["standardAudio"].as_str().unwrap()
            );
            assert_eq!(
                actual.standard.subtitles,
                expected["standardSubtitles"].as_str().unwrap()
            );
            assert_eq!(
                actual
                    .issues
                    .iter()
                    .map(|issue| issue.message.as_str())
                    .collect::<Vec<_>>(),
                expected["issues"]
                    .as_array()
                    .expect("issues")
                    .iter()
                    .map(|issue| issue.as_str().unwrap())
                    .collect::<Vec<_>>()
            );
        }
    }
}
