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
import type { ApiError } from "./error";

export type BackendTransport = "http" | "tauri" | "mock";

export type MediaServerConnectionRequest = {
  id?: string;
  name?: string;
  type?: string;
  serverUrl?: string;
  apiKey?: string;
};

export type RenameSearchRequest = {
  query: string;
  provider?: string;
  language?: string;
};

export type RenameScopesRequest = {
  selectedResult: RenameSearchResult;
  provider?: string;
  language?: string;
};

export type RenameProviderTestRequest = {
  provider?: string;
  language?: string;
};

export type RenamePreviewRequest = RenameScopesRequest & {
  files: MediaFileRow[];
  scopeKeys?: string[];
  template?: string;
};

export type RenameApplyRequest = {
  items: import("../api").RenamePreviewRow[];
  provider?: string;
  template?: string;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
};

export type PropEditTemplateRequest = {
  files: MediaFileRow[];
  templatePath?: string;
};

export type JobKind = "scan" | "operation";

export type JobProgressSubscription = {
  jobId: string;
  kind: JobKind;
  pollIntervalMs?: number;
};

export type JobProgressEvent =
  | { jobId: string; kind: "scan"; job: ScanJobResponse }
  | { jobId: string; kind: "operation"; job: OperationJobResponse };

export type JobProgressListener = (event: JobProgressEvent) => void;
export type JobProgressErrorListener = (error: ApiError) => void;
export type Unsubscribe = () => void;

/**
 * Transport-neutral application boundary used by React pages.
 *
 * Implementations may use HTTP, Tauri IPC, or an in-memory test double. Keeping
 * the complete workflow contract here prevents individual pages from depending
 * on transport details.
 */
export interface BackendClient {
  readonly transport: BackendTransport;

  getStatus(): Promise<AppStatus>;
  browseFileSystem(path?: string): Promise<FileSystemResponse>;

  startScan(request: ScanRequest): Promise<ScanJobResponse>;
  getScanJob(id: string): Promise<ScanJobResponse>;
  cancelScan(id: string): Promise<ScanJobResponse>;
  getCurrentScanFiles(): Promise<CurrentScanResponse>;
  clearCurrentScanFiles(): Promise<CurrentScanResponse>;

  getWebSettings(): Promise<WebSettings>;
  saveWebSettings(request: WebSettingsRequest): Promise<WebSettings>;
  testMediaServerConnection(request: MediaServerConnectionRequest): Promise<MediaServerTestResponse>;
  syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse>;

  searchRenameMetadata(request: RenameSearchRequest): Promise<{ results: RenameSearchResult[] }>;
  loadRenameScopes(request: RenameScopesRequest): Promise<{ scopes: RenameScopeRow[] }>;
  testRenameProvider(request: RenameProviderTestRequest): Promise<RenameProviderTestResponse>;
  buildRenamePreview(request: RenamePreviewRequest): Promise<RenamePreviewResponse>;
  applyRenamePreview(request: RenameApplyRequest): Promise<RenameApplyResponse>;
  getRenameBatches(): Promise<RenameBatchListResponse>;
  previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse>;
  undoRenameBatch(id: string): Promise<RenameBatchUndoResponse>;
  clearRenameBatches(): Promise<RenameBatchListResponse>;

  buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse>;
  startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse>;
  getOperationJob(id: string): Promise<OperationJobResponse>;
  cancelOperationJob(id: string): Promise<OperationJobResponse>;

  loadPropEditTemplate(request: PropEditTemplateRequest): Promise<PropEditTemplateResponse>;
  buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse>;
  startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse>;

  buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse>;
  getOperationLogs(): Promise<{ entries: OperationLogEntry[] }>;
  clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }>;

  subscribeJobProgress(
    subscription: JobProgressSubscription,
    listener: JobProgressListener,
    onError?: JobProgressErrorListener
  ): Promise<Unsubscribe>;
}
