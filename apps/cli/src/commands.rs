//! The four verbs, each driving the shared runtime rather than its own logic.

use anyhow::{Context, Result, bail};
use mkvo_contracts::{MediaFileRow, PropEditTrackConfigRow, ScanRequest, TitleEditMode};
use mkvo_runtime::MkvoRuntime;
use mkvo_runtime::compat::{
    PropEditPreviewRequest, PropEditTemplateRequest, RenameApplyRequest, RenamePreviewRequest,
    RenameSearchRequest,
};

use crate::cli::{CleanupArgs, RenameArgs, ScanArgs, ToolArgs};
use crate::{EXIT_NOTHING_TO_DO, EXIT_OK};

async fn scan_folder(
    runtime: &MkvoRuntime,
    path: &std::path::Path,
    ignore: &[String],
    force_refresh: bool,
    tools: &ToolArgs,
) -> Result<Vec<MediaFileRow>> {
    let mut snapshot = runtime
        .start_scan(ScanRequest {
            source_path: Some(path.display().to_string()),
            ignored_folder_names: ignore.to_vec(),
            mkv_merge_path: tools.mkvmerge.clone(),
            ff_probe_path: tools.ffprobe.clone(),
            force_refresh,
            ..Default::default()
        })
        .await
        .context("scan could not be started")?;

    while !snapshot.status.is_terminal() {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        snapshot = runtime.get_scan_job(&snapshot.id.to_string()).await?;
    }

    if snapshot.status != mkvo_contracts::JobStatus::Completed {
        let reason = if snapshot.error.is_empty() {
            format!("{:?}", snapshot.status)
        } else {
            snapshot.error.clone()
        };
        bail!("scan did not complete: {reason}");
    }

    let mut files = snapshot.files;
    files.sort_by_key(|file| file.path.to_lowercase());
    Ok(files)
}

pub async fn scan(runtime: &MkvoRuntime, args: ScanArgs) -> Result<u8> {
    let files = scan_folder(
        runtime,
        &args.path,
        &args.ignore,
        args.force_refresh,
        &args.tools,
    )
    .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&files)?);
        return Ok(EXIT_OK);
    }

    println!("Scanned {} file(s).", files.len());
    for file in &files {
        println!(
            "{} | {} | {} | {} | {} | {}",
            file.file_name,
            file.codec,
            file.resolution,
            file.audio_summary,
            file.subtitle_summary,
            file.status
        );
    }
    Ok(EXIT_OK)
}

/// Applies the requested edits to one set of track rows.
///
/// Edits are positional: row 1 is every file's first track of that kind, which
/// is how the app's bulk editor works too.
fn edit_rows(rows: &mut [PropEditTrackConfigRow], clear_names: bool, language: Option<&str>) {
    for row in rows {
        if clear_names {
            row.edited_name = String::new();
        }
        if let Some(code) = language {
            row.edited_language = code.to_owned();
        }
    }
}

pub async fn cleanup(runtime: &MkvoRuntime, args: CleanupArgs) -> Result<u8> {
    let files = scan_folder(runtime, &args.path, &args.ignore, false, &args.tools).await?;
    if files.is_empty() {
        println!("Planned 0 cleanup action(s).");
        return Ok(EXIT_NOTHING_TO_DO);
    }

    // Track layout is taken from the first file and applied across the set,
    // matching how the UI templates a bulk edit.
    let template = runtime
        .load_propedit_template(PropEditTemplateRequest {
            files: files.clone(),
            template_path: Some(files[0].path.clone()),
        })
        .await
        .context("could not read the track layout of the first file")?;

    let mut audio_tracks = template.audio_tracks;
    let mut subtitle_tracks = template.subtitle_tracks;
    edit_rows(
        &mut audio_tracks,
        args.remove_audio_titles,
        args.set_audio_language.as_deref(),
    );
    edit_rows(
        &mut subtitle_tracks,
        args.remove_subtitle_titles,
        args.set_subtitle_language.as_deref(),
    );

    // Built once and reused: apply replays the same request with the plan
    // fields the preview issued, so the bytes that were previewed are the bytes
    // that get written.
    let request = PropEditPreviewRequest {
        selected_paths: files.iter().map(|file| file.path.clone()).collect(),
        files,
        template_path: Some(template.template_path),
        container_title_mode: if args.keep_container_title {
            TitleEditMode::Keep
        } else {
            TitleEditMode::Remove
        },
        custom_container_title: String::new(),
        video_title_mode: if args.keep_video_title {
            TitleEditMode::Keep
        } else {
            TitleEditMode::Remove
        },
        custom_video_title: String::new(),
        video_track_language: None,
        audio_tracks,
        subtitle_tracks,
        // Left blank so existing default and forced flags are kept; the retired
        // CLI had no switch for them either.
        selected_default_audio: String::new(),
        selected_forced_audio: String::new(),
        selected_default_subtitle: String::new(),
        selected_forced_subtitle: String::new(),
        plan_id: None,
        plan_fingerprint: None,
        idempotency_key: None,
    };

    let preview = runtime.build_propedit_preview(request.clone()).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&preview)?);
    } else {
        println!("Planned {} cleanup action(s).", preview.actions.len());
        for action in &preview.actions {
            println!("{}: {}", action.file_name, action.description);
        }
        for skipped in &preview.skipped {
            println!("{}: skipped, {}", skipped.file_name, skipped.reason);
        }
    }

    if preview.actions.is_empty() {
        return Ok(EXIT_NOTHING_TO_DO);
    }

    if args.apply {
        let mut job = runtime
            .start_propedit_apply(PropEditPreviewRequest {
                plan_id: preview.plan_id,
                plan_fingerprint: preview.plan_fingerprint.clone(),
                idempotency_key: preview.idempotency_key,
                ..request
            })
            .await?;
        while !job.status.is_terminal() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            job = runtime.get_operation_job(&job.id).await?;
        }
        println!(
            "Apply {:?}: {} done, {} failed, {} skipped.",
            job.status, job.completed, job.failed, job.skipped
        );
        if job.status != mkvo_contracts::JobStatus::Completed {
            bail!(
                "property edits did not complete: {}",
                if job.error.is_empty() {
                    "see the logs"
                } else {
                    &job.error
                }
            );
        }
    }

    Ok(EXIT_OK)
}

pub async fn rename(runtime: &MkvoRuntime, args: RenameArgs) -> Result<u8> {
    let files = scan_folder(runtime, &args.path, &args.ignore, false, &args.tools).await?;
    if files.is_empty() {
        println!("Rename plan: 0 rename(s), 0 skip(s).");
        return Ok(EXIT_NOTHING_TO_DO);
    }

    // Renaming goes through a provider lookup, so it needs something to search
    // for. The folder name is the same guess a person makes by hand.
    let query = match args.query.clone() {
        Some(query) => query,
        None => args
            .path
            .canonicalize()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .context("could not infer a title from the folder; pass --query")?,
    };

    let matches = runtime
        .search_rename_metadata(RenameSearchRequest {
            query: query.clone(),
            provider: args.provider.clone(),
            language: args.language.clone(),
        })
        .await
        .context("provider search failed")?;

    if args.list_matches {
        if args.json {
            println!("{}", serde_json::to_string_pretty(&matches)?);
        } else {
            for (index, result) in matches.iter().enumerate() {
                println!(
                    "{}. {} [{}]",
                    index + 1,
                    result.display_name,
                    result.provider
                );
            }
        }
        return Ok(if matches.is_empty() {
            EXIT_NOTHING_TO_DO
        } else {
            EXIT_OK
        });
    }

    if matches.is_empty() {
        bail!("no provider match for `{query}`; try --query or --provider");
    }
    let selected = matches
        .get(args.pick.saturating_sub(1))
        .with_context(|| {
            format!(
                "--pick {} is out of range, {} match(es)",
                args.pick,
                matches.len()
            )
        })?
        .clone();

    let preview = runtime
        .build_rename_preview(RenamePreviewRequest {
            files,
            selected_result: selected,
            provider: args.provider.clone(),
            language: args.language.clone(),
            scope_keys: Vec::new(),
            template: args.template.clone(),
            idempotency_key: None,
        })
        .await?;

    let renames = preview.items.iter().filter(|item| item.can_apply).count();
    let skips = preview.items.len() - renames;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&preview)?);
    } else {
        println!("Rename plan: {renames} rename(s), {skips} skip(s).");
        for item in &preview.items {
            println!(
                "{} -> {} [{}]",
                item.current_file_name, item.new_file_name, item.status
            );
        }
    }

    if args.apply {
        let applied = runtime
            .apply_rename_preview(RenameApplyRequest {
                items: preview.items.clone(),
                provider: args.provider.clone(),
                template: args.template.clone(),
                plan_id: preview.plan_id,
                plan_fingerprint: preview.plan_fingerprint.clone(),
                idempotency_key: preview.idempotency_key,
            })
            .await?;
        println!("Applied: {}", applied.summary);
    }

    // Nothing renameable is not a failure, but a script should be able to tell.
    Ok(if renames == 0 {
        EXIT_NOTHING_TO_DO
    } else {
        EXIT_OK
    })
}
