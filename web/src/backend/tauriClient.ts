import type { LogExport } from "./client";
import type {
  AppStatus,
  CurrentScanResponse,
  FileSystemResponse,
  LibraryArtworkRequest,
  LibraryArtworkResponse,
  LibraryAuditResponse,
  LibraryCatalogRequest,
  LibraryCatalogResponse,
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
import { validateContract, type ContractName } from "../generated/contracts";

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

  private async invoke<T>(command: string, args?: Record<string, unknown>, contract?: ContractName): Promise<T> {
    try {
      // This dynamic import is intentional: the HTTP/browser build must not touch
      // Tauri globals just by importing the shared API module.
      const { invoke } = await import("@tauri-apps/api/core");
      const payload: unknown = await invoke(command, args);
      if (contract) {
        const validation = validateContract(contract, payload);
        if (!validation.ok) {
          throw new ApiError(`The desktop backend returned an invalid ${contract} response.`, {
            code: "INVALID_RESPONSE",
            details: validation.errors
          });
        }
      }
      return payload as T;
    } catch (error) {
      throw normalizeApiError(error, `Tauri command '${command}' failed.`);
    }
  }

  getStatus(): Promise<AppStatus> {
    return this.invoke<AppStatus>("get_status", undefined, "AppStatus");
  }

  browseFileSystem(path?: string): Promise<FileSystemResponse> {
    return this.invoke<FileSystemResponse>("browse_file_system", { path: path ?? null }, "FileSystemResponse");
  }

  startScan(request: ScanRequest): Promise<ScanJobResponse> {
    return this.invoke<ScanJobResponse>("start_scan", { request }, "ScanJobResponse");
  }

  getScanJob(id: string): Promise<ScanJobResponse> {
    return this.invoke<ScanJobResponse>("get_scan_job", { id }, "ScanJobResponse");
  }

  cancelScan(id: string): Promise<ScanJobResponse> {
    return this.invoke<ScanJobResponse>("cancel_scan", { id }, "ScanJobResponse");
  }

  getCurrentScanFiles(): Promise<CurrentScanResponse> {
    return this.invoke<CurrentScanResponse>("get_current_scan_files", undefined, "CurrentScanResponse");
  }

  clearCurrentScanFiles(): Promise<CurrentScanResponse> {
    return this.invoke<CurrentScanResponse>("clear_current_scan_files", undefined, "CurrentScanResponse");
  }

  setFileSelection(paths: string[]): Promise<CurrentScanResponse> {
    return this.invoke<CurrentScanResponse>("set_file_selection", { request: { paths } }, "CurrentScanResponse");
  }

  async authorizeBrowsedRoot(path: string): Promise<void> {
    await this.invoke<unknown>("authorize_browsed_root", { path });
  }

  getWebSettings(): Promise<WebSettings> {
    return this.invoke<WebSettings>("get_web_settings", undefined, "WebSettings");
  }

  saveWebSettings(request: WebSettingsRequest): Promise<WebSettings> {
    return this.invoke<WebSettings>("save_web_settings", { request }, "WebSettings");
  }

  testMediaServerConnection(request: MediaServerConnectionRequest): Promise<MediaServerTestResponse> {
    return this.invoke<MediaServerTestResponse>("test_media_server_connection", { request }, "MediaServerTestResponse");
  }

  syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse> {
    return this.invoke<MediaServerSyncResponse>("sync_media_server_libraries", { id }, "MediaServerSyncResponse");
  }

  searchRenameMetadata(request: RenameSearchRequest): Promise<{ results: RenameSearchResult[] }> {
    return this.invoke<{ results: RenameSearchResult[] }>("search_rename_metadata", { request }, "RenameSearchResponse");
  }

  loadRenameScopes(request: RenameScopesRequest): Promise<{ scopes: RenameScopeRow[] }> {
    return this.invoke<{ scopes: RenameScopeRow[] }>("load_rename_scopes", { request }, "RenameScopesResponse");
  }

  testRenameProvider(request: RenameProviderTestRequest): Promise<RenameProviderTestResponse> {
    return this.invoke<RenameProviderTestResponse>("test_rename_provider", { request }, "RenameProviderTestResponse");
  }

  buildRenamePreview(request: RenamePreviewRequest): Promise<RenamePreviewResponse> {
    return this.invoke<RenamePreviewResponse>("build_rename_preview", { request }, "RenamePreviewResponse");
  }

  applyRenamePreview(request: RenameApplyRequest): Promise<RenameApplyResponse> {
    return this.invoke<RenameApplyResponse>("apply_rename_preview", { request }, "RenameApplyResponse");
  }

  getRenameBatches(): Promise<RenameBatchListResponse> {
    return this.invoke<RenameBatchListResponse>("get_rename_batches", undefined, "RenameBatchListResponse");
  }

  previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse> {
    return this.invoke<RenameBatchUndoPreviewResponse>("preview_rename_batch_undo", { id }, "RenameBatchUndoPreviewResponse");
  }

  undoRenameBatch(id: string): Promise<RenameBatchUndoResponse> {
    return this.invoke<RenameBatchUndoResponse>("undo_rename_batch", { id }, "RenameBatchUndoResponse");
  }

  clearRenameBatches(): Promise<RenameBatchListResponse> {
    return this.invoke<RenameBatchListResponse>("clear_rename_batches", undefined, "RenameBatchListResponse");
  }

  buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse> {
    return this.invoke<MuxPreviewResponse>("build_mux_preview", { request }, "MuxPreviewResponse");
  }

  startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("apply_mux_preview", { request }, "OperationJobResponse");
  }

  getOperationJob(id: string): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("get_operation_job", { id }, "OperationJobResponse");
  }

  cancelOperationJob(id: string): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("cancel_operation_job", { id }, "OperationJobResponse");
  }

  loadPropEditTemplate(request: PropEditTemplateRequest): Promise<PropEditTemplateResponse> {
    return this.invoke<PropEditTemplateResponse>("load_propedit_template", { request }, "PropEditTemplateResponse");
  }

  buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse> {
    return this.invoke<PropEditPreviewResponse>("build_propedit_preview", { request }, "PropEditPreviewResponse");
  }

  startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse> {
    return this.invoke<OperationJobResponse>("apply_propedit_preview", { request }, "OperationJobResponse");
  }

  buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse> {
    return this.invoke<LibraryAuditResponse>("run_library_audit", { request: { files } }, "LibraryAuditResponse");
  }

  getLibraryCatalog(request: LibraryCatalogRequest): Promise<LibraryCatalogResponse> {
    return this.invoke<LibraryCatalogResponse>("get_library_catalog", { request }, "LibraryCatalogResponse");
  }

  getLibraryArtwork(request: LibraryArtworkRequest): Promise<LibraryArtworkResponse> {
    return this.invoke<LibraryArtworkResponse>("get_library_artwork", { request }, "LibraryArtworkResponse");
  }

  getOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
    return this.invoke<{ entries: OperationLogEntry[] }>("get_logs", undefined, "OperationLogResponse");
  }

  clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
    return this.invoke<{ entries: OperationLogEntry[] }>("clear_logs", undefined, "OperationLogResponse");
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
          const validation = validateContract("ScanJobResponse", payload.job);
          if (validation.ok) {
            publish({ jobId, kind: "scan", job: validation.value });
          } else {
            onError?.(new ApiError("The desktop backend emitted an invalid scan update.", {
              code: "INVALID_RESPONSE",
              details: validation.errors
            }));
          }
        } else if (payload.kind === "operation") {
          const validation = validateContract("OperationJobResponse", payload.job);
          if (validation.ok) {
            publish({ jobId, kind: "operation", job: validation.value });
          } else {
            onError?.(new ApiError("The desktop backend emitted an invalid operation update.", {
              code: "INVALID_RESPONSE",
              details: validation.errors
            }));
          }
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
