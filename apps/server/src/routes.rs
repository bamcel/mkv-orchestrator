use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Sse, sse::Event},
    routing::{any, get, post},
};
use futures::stream;
use mkvo_contracts::{ScanRequest, WebSettingsRequest};
use mkvo_runtime::{
    RuntimeError,
    compat::{
        LibraryAuditRequest, MediaServerConnectionRequest, MuxPreviewRequest,
        PropEditPreviewRequest, PropEditTemplateRequest, RenameApplyRequest, RenamePreviewRequest,
        RenameProviderTestRequest, RenameScopesRequest, RenameSearchRequest, RenameSearchResult,
    },
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{AppState, error::HttpError};

#[derive(Serialize)]
pub(crate) struct HealthResponse {
    status: &'static str,
}

#[derive(Default, Deserialize)]
struct BrowseQuery {
    path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameSearchResponse {
    results: Vec<RenameSearchResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameScopesResponse {
    scopes: Vec<mkvo_contracts::RenameScopeRow>,
}

pub(crate) fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/filesystem", get(browse_file_system))
        .route("/api/scans", post(start_scan))
        .route("/api/scans/{id}", get(get_scan_job))
        .route("/api/scans/{id}/cancel", post(cancel_scan))
        .route("/api/scans/{id}/events", get(job_events))
        .route(
            "/api/files/current",
            get(get_current_scan_files).delete(clear_current_scan_files),
        )
        .route(
            "/api/settings",
            get(get_web_settings).put(save_web_settings),
        )
        .route("/api/media-servers/test", post(test_media_server_connection))
        .route(
            "/api/media-servers/{id}/sync",
            post(sync_media_server_libraries),
        )
        .route("/api/rename/search", post(search_rename_metadata))
        .route("/api/rename/scopes", post(load_rename_scopes))
        .route("/api/rename/test-provider", post(test_rename_provider))
        .route("/api/rename/preview", post(build_rename_preview))
        .route("/api/rename/apply", post(apply_rename_preview))
        .route(
            "/api/rename/batches",
            get(get_rename_batches).delete(clear_rename_batches),
        )
        .route(
            "/api/rename/batches/{id}/preview",
            get(preview_rename_batch_undo),
        )
        .route(
            "/api/rename/batches/{id}/undo",
            post(undo_rename_batch),
        )
        .route("/api/mux/preview", post(build_mux_preview))
        .route("/api/mux/apply", post(start_mux_apply))
        .route("/api/propedit/template", post(load_propedit_template))
        .route("/api/propedit/preview", post(build_propedit_preview))
        .route("/api/propedit/apply", post(start_propedit_apply))
        .route("/api/operations/{id}", get(get_operation_job))
        .route(
            "/api/operations/{id}/cancel",
            post(cancel_operation_job),
        )
        .route("/api/operations/{id}/events", get(job_events))
        .route("/api/library/audit", post(run_library_audit))
        .route("/api/logs", get(get_logs).delete(clear_logs))
        // This must be a route rather than the SPA fallback: unknown API paths
        // are JSON errors and must never receive index.html.
        .route("/api", any(api_not_found))
        .route("/api/{*path}", any(api_not_found))
        .with_state(state)
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn status(State(state): State<AppState>) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_status().await?))
}

async fn browse_file_system(
    State(state): State<AppState>,
    Query(query): Query<BrowseQuery>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.browse_file_system(query.path).await?))
}

async fn start_scan(
    State(state): State<AppState>,
    Json(request): Json<ScanRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let job = state.runtime.start_scan(request).await?;
    let location = format!("/api/scans/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    ))
}

async fn get_scan_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_scan_job(&id).await?))
}

async fn cancel_scan(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.cancel_scan(&id).await?))
}

async fn get_current_scan_files(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_current_scan_files().await?))
}

async fn clear_current_scan_files(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.clear_current_scan_files().await?))
}

async fn get_web_settings(State(state): State<AppState>) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_web_settings().await?))
}

async fn save_web_settings(
    State(state): State<AppState>,
    Json(request): Json<WebSettingsRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.save_web_settings(request).await?))
}

async fn test_media_server_connection(
    State(state): State<AppState>,
    Json(request): Json<MediaServerConnectionRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(
        state.runtime.test_media_server_connection(request).await?,
    ))
}

async fn sync_media_server_libraries(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.sync_media_server_libraries(&id).await?))
}

async fn search_rename_metadata(
    State(state): State<AppState>,
    Json(request): Json<RenameSearchRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(RenameSearchResponse {
        results: state.runtime.search_rename_metadata(request).await?,
    }))
}

async fn load_rename_scopes(
    State(state): State<AppState>,
    Json(request): Json<RenameScopesRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(RenameScopesResponse {
        scopes: state.runtime.load_rename_scopes(request).await?,
    }))
}

async fn test_rename_provider(
    State(state): State<AppState>,
    Json(request): Json<RenameProviderTestRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.test_rename_provider(request).await?))
}

async fn build_rename_preview(
    State(state): State<AppState>,
    Json(request): Json<RenamePreviewRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.build_rename_preview(request).await?))
}

async fn apply_rename_preview(
    State(state): State<AppState>,
    Json(request): Json<RenameApplyRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.apply_rename_preview(request).await?))
}

async fn get_rename_batches(State(state): State<AppState>) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_rename_batches().await?))
}

async fn preview_rename_batch_undo(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.preview_rename_batch_undo(&id).await?))
}

async fn undo_rename_batch(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.undo_rename_batch(&id).await?))
}

async fn clear_rename_batches(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.clear_rename_batches().await?))
}

async fn build_mux_preview(
    State(state): State<AppState>,
    Json(request): Json<MuxPreviewRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.build_mux_preview(request).await?))
}

async fn start_mux_apply(
    State(state): State<AppState>,
    Json(request): Json<MuxPreviewRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let job = state.runtime.start_mux_apply(request).await?;
    let location = format!("/api/operations/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    ))
}

async fn load_propedit_template(
    State(state): State<AppState>,
    Json(request): Json<PropEditTemplateRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.load_propedit_template(request).await?))
}

async fn build_propedit_preview(
    State(state): State<AppState>,
    Json(request): Json<PropEditPreviewRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.build_propedit_preview(request).await?))
}

async fn start_propedit_apply(
    State(state): State<AppState>,
    Json(request): Json<PropEditPreviewRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let job = state.runtime.start_propedit_apply(request).await?;
    let location = format!("/api/operations/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    ))
}

async fn get_operation_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_operation_job(&id).await?))
}

async fn cancel_operation_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.cancel_operation_job(&id).await?))
}

async fn job_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let receiver = state.runtime.subscribe_job_events(&id).await?;
    let events = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Ok(envelope) => {
                let sequence = envelope.sequence.to_string();
                let event = Event::default()
                    .event("job")
                    .id(sequence)
                    .json_data(&envelope)
                    .unwrap_or_else(|error| {
                        Event::default()
                            .event("serialization-error")
                            .data(error.to_string())
                    });
                Some((Ok::<Event, Infallible>(event), receiver))
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => Some((
                Ok(Event::default().event("lagged").data(skipped.to_string())),
                receiver,
            )),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    });
    Ok(Sse::new(events).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

async fn get_logs(State(state): State<AppState>) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.get_logs().await?))
}

async fn clear_logs(State(state): State<AppState>) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.clear_logs().await?))
}

async fn run_library_audit(
    State(state): State<AppState>,
    Json(request): Json<LibraryAuditRequest>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(state.runtime.run_library_audit(request).await?))
}

async fn api_not_found(uri: Uri) -> HttpError {
    RuntimeError::not_found(format!("API route not found: {uri}")).into()
}
