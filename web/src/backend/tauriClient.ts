import type { LogExport } from "./client";
import type {
  AppStatus,
  CurrentScanResponse,
  FileSystemResponse,
  LibraryAuditResponse,
  MediaFileRow,
  MediaServerSyncResponse,
  MediaServerTestResponse,
  MuxPreviewRequest,
  MuxPreviewResponse,
  OperationJobResponse,
  OperationLogEntry,
  PropEditPreviewRequest,
  PropEditPreviewResponse,
  PropEditTemplateResponse,
  RenameApplyResponse,
  RenameBatchListResponse,
  RenameBatchUndoPreviewResponse,
  RenameBatchUndoResponse,
  RenamePreviewResponse,
  RenameProviderTestResponse,
  RenameScopeRow,
  RenameSearchResult,
  ScanJobResponse,
  ScanRequest,
  WebSettings,
  WebSettingsRequest
} from "../api";
import type {
  BackendClient,
  JobProgressEvent,
  JobProgressSubscription,
  MediaServerConnectionRequest,
  PropEditTemplateRequest,
  RenameApplyRequest,
  RenamePreviewRequest,
  RenameProviderTestRequest,
  RenameScopesRequest,
  RenameSearchRequest,
  Unsubscribe
} from "./client";
import { ApiError, normalizeApiError } from "./error";

const JOB_PROGRESS_EVENT = "mkvo-job-progress";
const TERMINAL_JOB_STATUSES = new Set(["Completed", "Failed", "Skipped", "Canceled"]);

type TauriJobProgressPayload = JobProgressEvent & {
  job_id?: string;
};

function eventJobId(payload: TauriJobProgressPayload): string {
  return payload.jobId || payload.job_id || "";
}

function isTerminal(event: JobProgressEvent): boolean {
  return TERMINAL_JOB_STATUSES.has(event.job.status);
}

/** Tauri v2 IPC implementation. All Tauri modules are imported lazily. */
export class TauriBackendClient implements BackendClient {
  readonly transport = "tauri" as const;

  private async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    try {
      // This dynamic import is intentional: the HTTP/browser build must not touch
      // Tauri globals just by importing the shared API module.
      const { invoke } = await import("@tauri-apps/api/core");
      return await invoke<T>(command, args);
    } catch (error) {
      throw normalizeApiError(error, `Tauri command '${command}' failed.`);
    }
  }

  getStatus(): Promise<AppStatus> {
    return this.invoke<AppStatus>("get_status");
  }

  browseFileSystem(path?: string): Promise<FileSystemResponse> {
    return this.invoke<FileSystemResponse>("browse_file_system", { path: path ?? null });
  }

  startScan(request: ScanRequest): Promise<ScanJobResponse> {
    return this.invoke<ScanJobResponse>("start_scan", { request });
  }

  getScanJob(id: string): Promise<ScanJobResponse> {
    return this.invoke<ScanJobResponse>("get_scan_job", { id });
  }

  cancelScan(id: string): Promise<ScanJobResponse> {
    return this.invoke<ScanJobResponse>("cancel_scan", { id });
  }

  getCurrentScanFiles(): Promise<CurrentScanResponse> {
    return this.invoke<CurrentScanResponse>("get_current_scan_files");
  }

  clearCurrentScanFiles(): Promise<CurrentScanResponse> {
    return this.invoke<CurrentScanResponse>("clear_current_scan_files");
  }

  setFileSelection(paths: string[]): Promise<CurrentScanResponse> {
    return this.invoke<CurrentScanResponse>("set_file_selection", { request: { paths } });
  }

  async authorizeBrowsedRoot(path: string): Promise<void> {
    await this.invoke<unknown>("authorize_browsed_root", { path });
  }

  getWebSettings(): Promise<WebSettings> {
    return this.invoke<WebSettings>("get_web_settings");
  }

  saveWebSettings(request: WebSettingsRequest): Promise<WebSettings> {
    return this.invoke<WebSettings>("save_web_settings", { request });
  }

  testMediaServerConnection(request: MediaServerConnectionRequest): Promise<MediaServerTestResponse> {
    return this.invoke<MediaServerTestResponse>("test_media_server_connection", { request });
  }

  syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse> {
    return this.invoke<MediaServerSyncResponse>("sync_media_server_libraries", { id });
  }

  searchRenameMetadata(request: RenameSearchRequest): Promise<{ results: RenameSearchResult[] }> {
    return this.invoke<{ results: RenameSearchResult[] }>("search_rename_metadata", { request });
  }

  loadRenameScopes(request: RenameScopesRequest): Promise<{ scopes: RenameScopeRow[] }> {
    return this.invoke<{ scopes: RenameScopeRow[] }>("load_rename_scopes", { request });
  }

  testRenameProvider(request: RenameProviderTestRequest): Promise<RenameProviderTestResponse> {
    return this.invoke<RenameProviderTestResponse>("test_rename_provider", { request });
  }

  buildRenamePreview(request: RenamePreviewRequest): Promise<RenamePreviewResponse> {
    return this.invoke<RenamePreviewResponse>("build_rename_preview", { request });
  }

  applyRenamePreview(request: RenameApplyRequest): Promise<RenameApplyResponse> {
    return this.invoke<RenameApplyResponse>("apply_rename_preview", { request });
  }

  getRenameBatches(): Promise<RenameBatchListResponse> {
    return this.invoke<RenameBatchListResponse>("get_rename_batches");
  }

  previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse> {
    return this.invoke<RenameBatchUndoPreviewResponse>("preview_rename_batch_undo", { id });
  }

  undoRenameBatch(id: string): Promise<RenameBatchUndoResponse> {
    return this.invoke<RenameBatchUndoResponse>("undo_rename_batch", { id });
  }

  clearRenameBatches(): Promise<RenameBatchListResponse> {
    return this.invoke<RenameBatchListResponse>("clear_rename_batches");
  }

  buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse> {
    return this.invoke<MuxPreviewResponse>("build_mux_preview", { request });
  }

  startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("apply_mux_preview", { request });
  }

  getOperationJob(id: string): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("get_operation_job", { id });
  }

  cancelOperationJob(id: string): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("cancel_operation_job", { id });
  }

  loadPropEditTemplate(request: PropEditTemplateRequest): Promise<PropEditTemplateResponse> {
    return this.invoke<PropEditTemplateResponse>("load_propedit_template", { request });
  }

  buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse> {
    return this.invoke<PropEditPreviewResponse>("build_propedit_preview", { request });
  }

  startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("apply_propedit_preview", { request });
  }

  buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse> {
    return this.invoke<LibraryAuditResponse>("run_library_audit", { request: { files } });
  }

  getOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
    return this.invoke<{ entries: OperationLogEntry[] }>("get_logs");
  }

  clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
    return this.invoke<{ entries: OperationLogEntry[] }>("clear_logs");
  }

  exportOperationLogs(): Promise<LogExport> {
    return this.invoke<LogExport>("export_logs");
  }

  async subscribeJobProgress(
    subscription: JobProgressSubscription,
    listener: (event: JobProgressEvent) => void,
    onError?: (error: ApiError) => void
  ): Promise<Unsubscribe> {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: Unsubscribe | undefined;
    const interval = Math.max(500, subscription.pollIntervalMs ?? 2000);

    const stop = (): void => {
      stopped = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
      const removeListener = unlisten;
      unlisten = undefined;
      removeListener?.();
    };

    const publish = (event: JobProgressEvent): void => {
      if (stopped || event.jobId !== subscription.jobId || event.kind !== subscription.kind) {
        return;
      }
      listener(event);
      if (isTerminal(event)) {
        stop();
      }
    };

    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<TauriJobProgressPayload>(JOB_PROGRESS_EVENT, ({ payload }) => {
        const jobId = eventJobId(payload);
        if (payload.kind === "scan") {
          publish({ jobId, kind: "scan", job: payload.job as ScanJobResponse });
        } else if (payload.kind === "operation") {
          publish({ jobId, kind: "operation", job: payload.job as OperationJobResponse });
        }
      });
      if (stopped) {
        stop();
      }
    } catch (error) {
      // Polling below remains a complete fallback when native events are not
      // available (including early migration stages).
      onError?.(normalizeApiError(error, "Unable to subscribe to native job events; using polling."));
    }

    const poll = async (): Promise<void> => {
      if (stopped) {
        return;
      }
      try {
        if (subscription.kind === "scan") {
          const job = await this.getScanJob(subscription.jobId);
          publish({ jobId: subscription.jobId, kind: "scan", job });
        } else {
          const job = await this.getOperationJob(subscription.jobId);
          publish({ jobId: subscription.jobId, kind: "operation", job });
        }
      } catch (error) {
        onError?.(normalizeApiError(error));
      }

      if (!stopped) {
        timer = setTimeout(() => void poll(), interval);
      }
    };

    void poll();
    return stop;
  }
}
