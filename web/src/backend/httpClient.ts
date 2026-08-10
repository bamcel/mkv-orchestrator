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

export type HttpBackendClientOptions = {
  baseUrl?: string;
  fetch?: typeof fetch;
};

type ErrorPayload = {
  message?: unknown;
  error?: unknown;
  code?: unknown;
  details?: unknown;
  title?: unknown;
  detail?: unknown;
};

const TERMINAL_JOB_STATUSES = new Set(["Completed", "Failed", "Skipped", "Canceled"]);

function resolveErrorMessage(payload: ErrorPayload | undefined, fallback: string): string {
  if (typeof payload?.message === "string" && payload.message.trim()) {
    return payload.message;
  }
  if (typeof payload?.error === "string" && payload.error.trim()) {
    return payload.error;
  }
  if (typeof payload?.detail === "string" && payload.detail.trim()) {
    return payload.detail;
  }
  if (typeof payload?.title === "string" && payload.title.trim()) {
    return payload.title;
  }
  if (payload?.error && typeof payload.error === "object") {
    const nestedMessage = (payload.error as ErrorPayload).message;
    if (typeof nestedMessage === "string" && nestedMessage.trim()) {
      return nestedMessage;
    }
  }
  return fallback;
}

export class HttpBackendClient implements BackendClient {
  readonly transport = "http" as const;

  private readonly baseUrl: string;
  private readonly fetchImpl: typeof fetch;

  constructor(options: HttpBackendClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? "").replace(/\/+$/, "");
    this.fetchImpl = options.fetch ?? ((input, init) => fetch(input, init));
  }

  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    let response: Response;
    try {
      const headers = new Headers(init?.headers);
      if (!headers.has("Accept")) {
        headers.set("Accept", "application/json");
      }
      response = await this.fetchImpl(this.url(path), {
        credentials: "same-origin",
        ...init,
        headers
      });
    } catch (error) {
      throw normalizeApiError(error, `Unable to reach the MKVO server at ${this.url(path)}.`);
    }

    const text = await response.text();
    let payload: unknown;
    if (text) {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = text;
      }
    }

    if (!response.ok) {
      const errorPayload = payload && typeof payload === "object" ? (payload as ErrorPayload) : undefined;
      const fallback =
        (typeof payload === "string" && payload.trim()) ||
        `${response.status} ${response.statusText || "Request failed"}`;
      const code = typeof errorPayload?.code === "string" ? errorPayload.code : `HTTP_${response.status}`;
      throw new ApiError(resolveErrorMessage(errorPayload, fallback), {
        code,
        status: response.status,
        details: errorPayload?.details ?? errorPayload?.detail ?? payload
      });
    }

    return payload as T;
  }

  private json<T>(path: string, method: string, body?: unknown): Promise<T> {
    return this.request<T>(path, {
      method,
      headers: body === undefined ? undefined : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  }

  getStatus(): Promise<AppStatus> {
    return this.request<AppStatus>("/api/status");
  }

  browseFileSystem(path?: string): Promise<FileSystemResponse> {
    const query = path ? `?path=${encodeURIComponent(path)}` : "";
    return this.request<FileSystemResponse>(`/api/filesystem${query}`);
  }

  startScan(request: ScanRequest): Promise<ScanJobResponse> {
    return this.json<ScanJobResponse>("/api/scans", "POST", request);
  }

  getScanJob(id: string): Promise<ScanJobResponse> {
    return this.request<ScanJobResponse>(`/api/scans/${encodeURIComponent(id)}`);
  }

  cancelScan(id: string): Promise<ScanJobResponse> {
    return this.json<ScanJobResponse>(`/api/scans/${encodeURIComponent(id)}/cancel`, "POST");
  }

  getCurrentScanFiles(): Promise<CurrentScanResponse> {
    return this.request<CurrentScanResponse>("/api/files/current");
  }

  clearCurrentScanFiles(): Promise<CurrentScanResponse> {
    return this.json<CurrentScanResponse>("/api/files/current", "DELETE");
  }

  getWebSettings(): Promise<WebSettings> {
    return this.request<WebSettings>("/api/settings");
  }

  saveWebSettings(request: WebSettingsRequest): Promise<WebSettings> {
    return this.json<WebSettings>("/api/settings", "PUT", request);
  }

  testMediaServerConnection(request: MediaServerConnectionRequest): Promise<MediaServerTestResponse> {
    return this.json<MediaServerTestResponse>("/api/media-servers/test", "POST", request);
  }

  syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse> {
    return this.json<MediaServerSyncResponse>(`/api/media-servers/${encodeURIComponent(id)}/sync`, "POST");
  }

  searchRenameMetadata(request: RenameSearchRequest): Promise<{ results: RenameSearchResult[] }> {
    return this.json<{ results: RenameSearchResult[] }>("/api/rename/search", "POST", request);
  }

  loadRenameScopes(request: RenameScopesRequest): Promise<{ scopes: RenameScopeRow[] }> {
    return this.json<{ scopes: RenameScopeRow[] }>("/api/rename/scopes", "POST", request);
  }

  testRenameProvider(request: RenameProviderTestRequest): Promise<RenameProviderTestResponse> {
    return this.json<RenameProviderTestResponse>("/api/rename/test-provider", "POST", request);
  }

  buildRenamePreview(request: RenamePreviewRequest): Promise<RenamePreviewResponse> {
    return this.json<RenamePreviewResponse>("/api/rename/preview", "POST", request);
  }

  applyRenamePreview(request: RenameApplyRequest): Promise<RenameApplyResponse> {
    return this.json<RenameApplyResponse>("/api/rename/apply", "POST", request);
  }

  getRenameBatches(): Promise<RenameBatchListResponse> {
    return this.request<RenameBatchListResponse>("/api/rename/batches");
  }

  previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse> {
    return this.request<RenameBatchUndoPreviewResponse>(`/api/rename/batches/${encodeURIComponent(id)}/preview`);
  }

  undoRenameBatch(id: string): Promise<RenameBatchUndoResponse> {
    return this.json<RenameBatchUndoResponse>(`/api/rename/batches/${encodeURIComponent(id)}/undo`, "POST");
  }

  clearRenameBatches(): Promise<RenameBatchListResponse> {
    return this.json<RenameBatchListResponse>("/api/rename/batches", "DELETE");
  }

  buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse> {
    return this.json<MuxPreviewResponse>("/api/mux/preview", "POST", request);
  }

  startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse> {
    return this.json<OperationJobResponse>("/api/mux/apply", "POST", request);
  }

  getOperationJob(id: string): Promise<OperationJobResponse> {
    return this.request<OperationJobResponse>(`/api/operations/${encodeURIComponent(id)}`);
  }

  cancelOperationJob(id: string): Promise<OperationJobResponse> {
    return this.json<OperationJobResponse>(`/api/operations/${encodeURIComponent(id)}/cancel`, "POST");
  }

  loadPropEditTemplate(request: PropEditTemplateRequest): Promise<PropEditTemplateResponse> {
    return this.json<PropEditTemplateResponse>("/api/propedit/template", "POST", request);
  }

  buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse> {
    return this.json<PropEditPreviewResponse>("/api/propedit/preview", "POST", request);
  }

  startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse> {
    return this.json<OperationJobResponse>("/api/propedit/apply", "POST", request);
  }

  buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse> {
    return this.json<LibraryAuditResponse>("/api/library/audit", "POST", { files });
  }

  getOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
    return this.request<{ entries: OperationLogEntry[] }>("/api/logs");
  }

  clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
    return this.json<{ entries: OperationLogEntry[] }>("/api/logs", "DELETE");
  }

  async subscribeJobProgress(
    subscription: JobProgressSubscription,
    listener: (event: JobProgressEvent) => void,
    onError?: (error: ApiError) => void
  ): Promise<Unsubscribe> {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const interval = Math.max(250, subscription.pollIntervalMs ?? 1000);

    const poll = async (): Promise<void> => {
      if (stopped) {
        return;
      }

      try {
        if (subscription.kind === "scan") {
          const job = await this.getScanJob(subscription.jobId);
          listener({ jobId: subscription.jobId, kind: "scan", job });
          if (TERMINAL_JOB_STATUSES.has(job.status)) {
            stopped = true;
            return;
          }
        } else {
          const job = await this.getOperationJob(subscription.jobId);
          listener({ jobId: subscription.jobId, kind: "operation", job });
          if (TERMINAL_JOB_STATUSES.has(job.status)) {
            stopped = true;
            return;
          }
        }
      } catch (error) {
        onError?.(normalizeApiError(error));
      }

      if (!stopped) {
        timer = setTimeout(() => void poll(), interval);
      }
    };

    void poll();
    return () => {
      stopped = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    };
  }
}
