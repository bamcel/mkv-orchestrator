//! Tauri delivery adapter for MKV Orchestrator.

mod commands;
mod error;
mod state;

use tauri::Manager;

/// Starts the desktop host.
///
/// Feature commands and state composition are intentionally kept in this host;
/// media behavior remains in the shared application and infrastructure crates.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let runtime = state::compose_runtime(app.handle())?;
            // Migration and crash recovery must finish before the UI can issue
            // a command. In particular, a usable OS keychain selects the async
            // migration path, so leaving this in a detached task would let the
            // first settings read race the one-time import.
            let migration = tauri::async_runtime::block_on(runtime.migrate_legacy_data())?;
            tracing::info!(status = ?migration.status, "legacy data migration checked");
            let recovery = tauri::async_runtime::block_on(runtime.recover_startup_state())?;
            tracing::info!(
                completed = recovery.completed,
                clean_retry = recovery.clean_retry,
                manual_review = recovery.manual_review,
                "startup recovery completed"
            );
            // Watching is started off the setup path: it enumerates configured
            // roots, which on a network share can block, and the window must
            // not wait on it. A watcher that cannot start is logged, never fatal.
            let watch_runtime = std::sync::Arc::clone(&runtime);
            tauri::async_runtime::spawn(async move {
                match watch_runtime.start_watchers().await {
                    Ok(report) if report.started => {
                        tracing::info!(roots = report.roots.len(), "watch folders active");
                    }
                    Ok(report) => {
                        if let Some(error) = report.error {
                            tracing::warn!(%error, "watch folders inactive");
                        }
                    }
                    Err(error) => tracing::warn!(%error, "watch folders could not be started"),
                }
            });
            app.manage(runtime);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::browse_file_system,
            commands::start_scan,
            commands::get_scan_job,
            commands::cancel_scan,
            commands::get_current_scan_files,
            commands::clear_current_scan_files,
            commands::set_file_selection,
            commands::authorize_browsed_root,
            commands::get_web_settings,
            commands::save_web_settings,
            commands::test_media_server_connection,
            commands::sync_media_server_libraries,
            commands::search_rename_metadata,
            commands::load_rename_scopes,
            commands::test_rename_provider,
            commands::build_rename_preview,
            commands::apply_rename_preview,
            commands::get_rename_batches,
            commands::preview_rename_batch_undo,
            commands::undo_rename_batch,
            commands::clear_rename_batches,
            commands::build_mux_preview,
            commands::apply_mux_preview,
            commands::get_operation_job,
            commands::cancel_operation_job,
            commands::load_propedit_template,
            commands::build_propedit_preview,
            commands::apply_propedit_preview,
            commands::run_library_audit,
            commands::select_source_folder,
            commands::get_watch_health,
            commands::list_recent_jobs,
            commands::export_logs,
            commands::get_logs,
            commands::clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start MKV Orchestrator");
}
