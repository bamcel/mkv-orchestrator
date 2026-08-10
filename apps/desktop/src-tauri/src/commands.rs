use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

use crate::{
    error::{CommandError, CommandResult, decode_request, encode_response},
    state::RuntimeState,
};

const JOB_PROGRESS_EVENT: &str = "mkvo-job-progress";

fn job_progress_payload(kind: &'static str, job: &Value) -> Option<Value> {
    let Some(job_id) = job.get("id").and_then(Value::as_str) else {
        warn!(kind, "runtime job response did not contain a string id");
        return None;
    };
    Some(serde_json::json!({
        "jobId": job_id,
        "kind": kind,
        "job": job,
    }))
}

fn emit_job_progress(app: &AppHandle, kind: &'static str, job: &Value) {
    let Some(payload) = job_progress_payload(kind, job) else {
        return;
    };
    let job_id = payload["jobId"].as_str().unwrap_or_default().to_owned();
    if let Err(error) = app.emit(JOB_PROGRESS_EVENT, payload) {
        warn!(%error, %job_id, kind, "could not emit Tauri job progress event");
    }
}

fn follow_scan_progress(app: AppHandle, runtime: RuntimeState, job_id: String) {
    tauri::async_runtime::spawn(async move {
        let mut events = match runtime.subscribe_job_events(&job_id).await {
            Ok(events) => events,
            Err(error) => {
                warn!(%error, %job_id, "could not subscribe to scan progress");
                return;
            }
        };

        loop {
            match runtime.get_scan_job(&job_id).await {
                Ok(job) => {
                    let terminal = job.status.is_terminal();
                    match serde_json::to_value(job) {
                        Ok(job) => emit_job_progress(&app, "scan", &job),
                        Err(error) => {
                            warn!(%error, %job_id, "could not serialize scan progress");
                        }
                    }
                    if terminal {
                        return;
                    }
                }
                Err(error) => {
                    warn!(%error, %job_id, "could not load scan progress");
                    return;
                }
            }

            match events.recv().await {
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    });
}

fn follow_operation_progress(app: AppHandle, runtime: RuntimeState, job_id: String) {
    tauri::async_runtime::spawn(async move {
        let mut events = match runtime.subscribe_job_events(&job_id).await {
            Ok(events) => events,
            Err(error) => {
                warn!(%error, %job_id, "could not subscribe to operation progress");
                return;
            }
        };

        loop {
            match runtime.get_operation_job(&job_id).await {
                Ok(job) => {
                    let terminal = job.status.is_terminal();
                    match serde_json::to_value(job) {
                        Ok(job) => emit_job_progress(&app, "operation", &job),
                        Err(error) => {
                            warn!(%error, %job_id, "could not serialize operation progress");
                        }
                    }
                    if terminal {
                        return;
                    }
                }
                Err(error) => {
                    warn!(%error, %job_id, "could not load operation progress");
                    return;
                }
            }

            match events.recv().await {
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    });
}

#[tauri::command]
pub async fn get_status(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.get_status().await)
}

#[tauri::command]
pub async fn browse_file_system(
    runtime: State<'_, RuntimeState>,
    path: Option<String>,
) -> CommandResult {
    encode_response(runtime.browse_file_system(path).await)
}

#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    let job = encode_response(runtime.start_scan(decode_request(request)?).await)?;
    emit_job_progress(&app, "scan", &job);
    if let Some(job_id) = job.get("id").and_then(Value::as_str) {
        follow_scan_progress(
            app,
            std::sync::Arc::clone(runtime.inner()),
            job_id.to_owned(),
        );
    }
    Ok(job)
}

#[tauri::command]
pub async fn get_scan_job(runtime: State<'_, RuntimeState>, id: String) -> CommandResult {
    encode_response(runtime.get_scan_job(&id).await)
}

#[tauri::command]
pub async fn cancel_scan(
    app: AppHandle,
    runtime: State<'_, RuntimeState>,
    id: String,
) -> CommandResult {
    let job = encode_response(runtime.cancel_scan(&id).await)?;
    emit_job_progress(&app, "scan", &job);
    Ok(job)
}

#[tauri::command]
pub async fn get_current_scan_files(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.get_current_scan_files().await)
}

#[tauri::command]
pub async fn clear_current_scan_files(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.clear_current_scan_files().await)
}

/// Authorize a folder chosen in the in-app browser so it can be scanned.
#[tauri::command]
pub async fn authorize_browsed_root(
    runtime: State<'_, RuntimeState>,
    path: String,
) -> CommandResult {
    let grant = runtime
        .authorize_browsed_root(&path)
        .map_err(CommandError::from)?;
    serde_json::to_value(grant).map_err(|error| {
        CommandError::from(mkvo_runtime::RuntimeError::internal(error.to_string()))
    })
}

#[tauri::command]
pub async fn set_file_selection(runtime: State<'_, RuntimeState>, request: Value) -> CommandResult {
    encode_response(runtime.set_file_selection(decode_request(request)?).await)
}

#[tauri::command]
pub async fn get_web_settings(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.get_web_settings().await)
}

#[tauri::command]
pub async fn save_web_settings(runtime: State<'_, RuntimeState>, request: Value) -> CommandResult {
    encode_response(runtime.save_web_settings(decode_request(request)?).await)
}

#[tauri::command]
pub async fn test_media_server_connection(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    encode_response(
        runtime
            .test_media_server_connection(decode_request(request)?)
            .await,
    )
}

#[tauri::command]
pub async fn sync_media_server_libraries(
    runtime: State<'_, RuntimeState>,
    id: String,
) -> CommandResult {
    encode_response(runtime.sync_media_server_libraries(&id).await)
}

#[tauri::command]
pub async fn search_rename_metadata(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    let results = runtime
        .search_rename_metadata(decode_request(request)?)
        .await
        .map_err(CommandError::from)?;
    Ok(serde_json::json!({ "results": results }))
}

#[tauri::command]
pub async fn load_rename_scopes(runtime: State<'_, RuntimeState>, request: Value) -> CommandResult {
    let scopes = runtime
        .load_rename_scopes(decode_request(request)?)
        .await
        .map_err(CommandError::from)?;
    Ok(serde_json::json!({ "scopes": scopes }))
}

#[tauri::command]
pub async fn test_rename_provider(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    encode_response(runtime.test_rename_provider(decode_request(request)?).await)
}

#[tauri::command]
pub async fn build_rename_preview(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    encode_response(runtime.build_rename_preview(decode_request(request)?).await)
}

#[tauri::command]
pub async fn apply_rename_preview(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    encode_response(runtime.apply_rename_preview(decode_request(request)?).await)
}

#[tauri::command]
pub async fn get_rename_batches(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.get_rename_batches().await)
}

#[tauri::command]
pub async fn preview_rename_batch_undo(
    runtime: State<'_, RuntimeState>,
    id: String,
) -> CommandResult {
    encode_response(runtime.preview_rename_batch_undo(&id).await)
}

#[tauri::command]
pub async fn undo_rename_batch(runtime: State<'_, RuntimeState>, id: String) -> CommandResult {
    encode_response(runtime.undo_rename_batch(&id).await)
}

#[tauri::command]
pub async fn clear_rename_batches(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.clear_rename_batches().await)
}

#[tauri::command]
pub async fn build_mux_preview(runtime: State<'_, RuntimeState>, request: Value) -> CommandResult {
    encode_response(runtime.build_mux_preview(decode_request(request)?).await)
}

#[tauri::command]
pub async fn apply_mux_preview(
    app: AppHandle,
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    let job = encode_response(runtime.start_mux_apply(decode_request(request)?).await)?;
    emit_job_progress(&app, "operation", &job);
    if let Some(job_id) = job.get("id").and_then(Value::as_str) {
        follow_operation_progress(
            app,
            std::sync::Arc::clone(runtime.inner()),
            job_id.to_owned(),
        );
    }
    Ok(job)
}

#[tauri::command]
pub async fn get_operation_job(runtime: State<'_, RuntimeState>, id: String) -> CommandResult {
    encode_response(runtime.get_operation_job(&id).await)
}

#[tauri::command]
pub async fn cancel_operation_job(
    app: AppHandle,
    runtime: State<'_, RuntimeState>,
    id: String,
) -> CommandResult {
    let job = encode_response(runtime.cancel_operation_job(&id).await)?;
    emit_job_progress(&app, "operation", &job);
    Ok(job)
}

#[tauri::command]
pub async fn load_propedit_template(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    encode_response(
        runtime
            .load_propedit_template(decode_request(request)?)
            .await,
    )
}

#[tauri::command]
pub async fn build_propedit_preview(
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    encode_response(
        runtime
            .build_propedit_preview(decode_request(request)?)
            .await,
    )
}

#[tauri::command]
pub async fn apply_propedit_preview(
    app: AppHandle,
    runtime: State<'_, RuntimeState>,
    request: Value,
) -> CommandResult {
    let job = encode_response(runtime.start_propedit_apply(decode_request(request)?).await)?;
    emit_job_progress(&app, "operation", &job);
    if let Some(job_id) = job.get("id").and_then(Value::as_str) {
        follow_operation_progress(
            app,
            std::sync::Arc::clone(runtime.inner()),
            job_id.to_owned(),
        );
    }
    Ok(job)
}

#[tauri::command]
pub async fn run_library_audit(runtime: State<'_, RuntimeState>, request: Value) -> CommandResult {
    encode_response(runtime.run_library_audit(decode_request(request)?).await)
}

/// Open the OS folder picker and authorize whatever the user chooses.
///
/// A picked folder is an explicit user act, so it earns an authorized-root
/// grant — but the grant still goes through the same validation as any other
/// root. The picker result is never trusted as a path on its own.
#[tauri::command]
pub async fn select_source_folder(
    app: AppHandle,
    runtime: State<'_, RuntimeState>,
) -> CommandResult {
    use tauri_plugin_dialog::DialogExt;

    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = sender.send(picked);
    });
    let Ok(Some(picked)) = receiver.await else {
        // A cancelled picker is a normal outcome, not an error.
        return Ok(serde_json::json!({ "cancelled": true }));
    };
    let path = picked.into_path().map_err(|error| {
        CommandError::from(mkvo_runtime::RuntimeError::invalid(error.to_string()))
    })?;
    let grant = runtime
        .grant_authorized_root(&path, true)
        .map_err(CommandError::from)?;
    Ok(serde_json::json!({ "cancelled": false, "root": grant }))
}

#[tauri::command]
pub async fn list_recent_jobs(
    runtime: State<'_, RuntimeState>,
    limit: Option<usize>,
) -> CommandResult {
    encode_response(runtime.list_recent_jobs(limit).await)
}

#[tauri::command]
pub async fn export_logs(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.export_logs().await)
}

#[tauri::command]
pub async fn get_watch_health(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.watch_health().await)
}

#[tauri::command]
pub async fn get_logs(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.get_logs().await)
}

#[tauri::command]
pub async fn clear_logs(runtime: State<'_, RuntimeState>) -> CommandResult {
    encode_response(runtime.clear_logs().await)
}

#[cfg(test)]
mod tests {
    use super::job_progress_payload;

    #[test]
    fn job_progress_payload_matches_the_frontend_contract() {
        let job = serde_json::json!({ "id": "job-42", "status": "Running" });
        let payload = job_progress_payload("scan", &job).expect("job payload");

        assert_eq!(payload["jobId"], "job-42");
        assert_eq!(payload["kind"], "scan");
        assert_eq!(payload["job"], job);
        assert_eq!(payload.as_object().expect("object").len(), 3);
    }
}
