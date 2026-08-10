use std::{path::Path, time::SystemTime};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mkvo_application::{
    MediaProbe as MediaProbePort, PortError, ToolExecutionResult, ToolExecutor, ToolInvocation,
    ToolRegistry as ToolRegistryPort,
};
use mkvo_contracts::ToolStatus as ContractToolStatus;
use mkvo_domain::{
    ContainerKind, ContainerMetadata as DomainContainer, FileFingerprint, MediaAttachment,
    MediaFile, MediaStatus, MediaTrack, TrackKind, VideoResolution,
};
use tokio_util::sync::CancellationToken;

use crate::{MediaScanAdapter, ProcessRunner, ProcessSpec, ScannedMedia, ToolKind, ToolRegistry};

#[async_trait]
impl MediaProbePort for MediaScanAdapter {
    async fn inspect(
        &self,
        path: &Path,
        cancel: CancellationToken,
    ) -> Result<MediaFile, PortError> {
        let scanned = crate::MediaProbe::inspect(self, path, cancel)
            .await
            .map_err(probe_port_error)?;
        scanned_to_domain(scanned).map_err(|error| PortError::InvalidData(error.to_string()))
    }
}

fn scanned_to_domain(scanned: ScannedMedia) -> Result<MediaFile, std::io::Error> {
    let metadata = std::fs::metadata(&scanned.path)?;
    let modified_at: DateTime<Utc> = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH).into();
    let status = if scanned.warning.is_some() {
        MediaStatus::Warning
    } else {
        MediaStatus::Ready
    };
    let container_kind = match scanned.container.format.as_deref() {
        Some(value) if value.to_ascii_lowercase().contains("matroska") => ContainerKind::Matroska,
        Some(value) if value.to_ascii_lowercase().contains("webm") => ContainerKind::WebM,
        Some(value)
            if value.to_ascii_lowercase().contains("mp4")
                || value.to_ascii_lowercase().contains("mov") =>
        {
            ContainerKind::Mp4
        }
        Some(value) => ContainerKind::Other(value.to_owned()),
        None => ContainerKind::Unknown,
    };
    Ok(MediaFile {
        path: scanned.path.clone(),
        original_file_name: scanned
            .path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned()),
        watch_root: None,
        relative_path: None,
        fingerprint: FileFingerprint {
            path: scanned.path,
            size_bytes: metadata.len(),
            modified_at,
            quick_hash: None,
        },
        container: DomainContainer {
            kind: container_kind,
            title: scanned.container.title,
            duration_millis: scanned
                .container
                .duration_millis
                .and_then(|value| i64::try_from(value).ok()),
            muxing_application: None,
            writing_application: None,
        },
        tracks: scanned
            .tracks
            .into_iter()
            .map(|track| MediaTrack {
                mkvmerge_id: u64::from(track.id),
                propedit_track_number: track.track_number.unwrap_or(track.id + 1),
                kind: match track.kind {
                    crate::TrackType::Video => TrackKind::Video,
                    crate::TrackType::Audio => TrackKind::Audio,
                    crate::TrackType::Subtitle => TrackKind::Subtitle,
                    crate::TrackType::Data | crate::TrackType::Other => TrackKind::Other,
                },
                codec: track.codec,
                codec_id: None,
                language: track.language,
                name: track.name,
                resolution: track
                    .width
                    .zip(track.height)
                    .map(|(width, height)| VideoResolution { width, height }),
                bit_depth: track.bit_depth.and_then(|value| u8::try_from(value).ok()),
                hdr: hdr_label(
                    track.color_transfer.as_deref(),
                    track.color_primaries.as_deref(),
                ),
                channels: None,
                sampling_frequency_hz: None,
                default: track.default,
                forced: track.forced,
                enabled: true,
            })
            .collect(),
        attachments: scanned
            .attachments
            .into_iter()
            .map(|attachment| MediaAttachment {
                id: u64::from(attachment.id),
                file_name: attachment.file_name.unwrap_or_default(),
                content_type: attachment.content_type,
                description: attachment.description,
                size_bytes: attachment.size_bytes,
            })
            .collect(),
        episode: None,
        provider_match: None,
        status,
    })
}

fn hdr_label(transfer: Option<&str>, primaries: Option<&str>) -> Option<String> {
    let transfer = transfer.unwrap_or_default().to_ascii_lowercase();
    let primaries = primaries.unwrap_or_default().to_ascii_lowercase();
    if transfer.contains("2084") || transfer.contains("pq") {
        Some("HDR10/PQ".to_owned())
    } else if transfer.contains("arib") || transfer.contains("hlg") {
        Some("HLG".to_owned())
    } else if primaries.contains("2020") {
        Some("BT.2020".to_owned())
    } else {
        None
    }
}

fn probe_port_error(error: crate::ProbeError) -> PortError {
    match error {
        crate::ProbeError::ToolUnavailable(tool) => {
            PortError::unavailable(format!("required tool `{tool}` is unavailable"), false)
        }
        crate::ProbeError::Process(crate::ProcessError::Cancelled) => PortError::Canceled,
        crate::ProbeError::Process(crate::ProcessError::TimedOut(timeout)) => {
            PortError::unavailable(format!("media probe timed out after {timeout:?}"), true)
        }
        crate::ProbeError::Unsupported(path) => {
            PortError::InvalidData(format!("unsupported media path `{}`", path.display()))
        }
        error => PortError::Other(error.to_string()),
    }
}

#[async_trait]
impl ToolRegistryPort for ToolRegistry {
    async fn status(&self, logical_name: &str) -> Result<ContractToolStatus, PortError> {
        let kind = tool_kind(logical_name)
            .ok_or_else(|| PortError::NotFound(format!("unknown tool `{logical_name}`")))?;
        let status = ToolRegistry::status(self, kind, &ProcessRunner).await;
        Ok(ContractToolStatus {
            name: kind.command_name().to_owned(),
            command: kind.command_name().to_owned(),
            // Tool paths are canonicalized during resolution; Settings shows this
            // value back to the user, so it leaves in its plain form.
            resolved_path: status
                .path
                .map_or_else(String::new, |path| mkvo_domain::normalized_path_text(&path)),
            available: status.available,
            version: status.version.unwrap_or_default(),
            error: status.error,
        })
    }

    async fn all_statuses(&self) -> Result<Vec<ContractToolStatus>, PortError> {
        let mut statuses = Vec::with_capacity(ToolKind::ALL.len());
        for kind in ToolKind::ALL {
            statuses.push(ToolRegistryPort::status(self, kind.command_name()).await?);
        }
        Ok(statuses)
    }
}

fn tool_kind(value: &str) -> Option<ToolKind> {
    let normalized = value.trim().to_ascii_lowercase();
    let value = normalized.strip_suffix(".exe").unwrap_or(&normalized);
    ToolKind::ALL
        .into_iter()
        .find(|kind| kind.command_name().eq_ignore_ascii_case(value))
}

#[derive(Debug, Clone, Default)]
pub struct ProcessToolExecutor {
    runner: ProcessRunner,
}

impl ProcessToolExecutor {
    pub const fn new(runner: ProcessRunner) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl ToolExecutor for ProcessToolExecutor {
    async fn execute(
        &self,
        invocation: &ToolInvocation,
        cancel: CancellationToken,
    ) -> Result<ToolExecutionResult, PortError> {
        if invocation.executable.as_os_str().is_empty() {
            return Err(PortError::InvalidData(
                "tool executable is blank".to_owned(),
            ));
        }
        let mut spec = ProcessSpec::new(&invocation.executable).args(&invocation.arguments);
        if let Some(directory) = &invocation.working_directory {
            spec = spec.current_dir(directory);
        }
        let output = self
            .runner
            .run(spec, cancel)
            .await
            .map_err(|error| match error {
                crate::ProcessError::Cancelled => PortError::Canceled,
                crate::ProcessError::TimedOut(timeout) => {
                    PortError::unavailable(format!("tool timed out after {timeout:?}"), true)
                }
                _ => PortError::Other(error.to_string()),
            })?;
        let validated_outputs = invocation
            .expected_outputs
            .iter()
            .filter(|path| {
                std::fs::metadata(path)
                    .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
            })
            .cloned()
            .collect();
        Ok(ToolExecutionResult {
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            validated_outputs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_tool_names_are_case_insensitive() {
        assert_eq!(tool_kind("MKVMERGE.EXE"), Some(ToolKind::MkvMerge));
        assert_eq!(tool_kind("unknown"), None);
    }

    #[test]
    fn hdr_labels_are_normalized() {
        assert_eq!(
            hdr_label(Some("smpte2084"), None).as_deref(),
            Some("HDR10/PQ")
        );
        assert_eq!(
            hdr_label(Some("arib-std-b67"), None).as_deref(),
            Some("HLG")
        );
    }
}
