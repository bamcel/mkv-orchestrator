import type {
  AppStatus,
  CurrentScanResponse,
  FileSystemResponse,
  LibraryArtworkRequest,
  LibraryArtworkResponse,
  LibraryAuditResponse,
  LibraryCatalogRequest,
  LibraryCatalogResponse,
  LibraryLocalArtworkRequest,
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

export type LogExport = {
  fileName: string;
  entryCount: number;
  content: string;
};

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
export interface BackendTransportClient {
  readonly transport: BackendTransport;
}

export interface StatusClient {
  getStatus(): Promise<AppStatus>;
  browseFileSystem(path?: string): Promise<FileSystemResponse>;
}

export interface ScanClient {
  startScan(request: ScanRequest): Promise<ScanJobResponse>;
  getScanJob(id: string): Promise<ScanJobResponse>;
  cancelScan(id: string): Promise<ScanJobResponse>;
  getCurrentScanFiles(): Promise<CurrentScanResponse>;
  clearCurrentScanFiles(): Promise<CurrentScanResponse>;
  setFileSelection(paths: string[]): Promise<CurrentScanResponse>;
  /**
   * Authorize a folder found by browsing so it can be scanned.
   *
   * Only meaningful where browsing ranges wider than the authorized roots. A
   * host that confines browsing has already authorized anything it showed.
   */
  authorizeBrowsedRoot(path: string): Promise<void>;
}

export interface SettingsClient {
  getWebSettings(): Promise<WebSettings>;
  saveWebSettings(request: WebSettingsRequest): Promise<WebSettings>;
  testMediaServerConnection(request: MediaServerConnectionRequest): Promise<MediaServerTestResponse>;
  syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse>;
}

export interface RenameClient {
  searchRenameMetadata(request: RenameSearchRequest): Promise<{ results: RenameSearchResult[] }>;
  loadRenameScopes(request: RenameScopesRequest): Promise<{ scopes: RenameScopeRow[] }>;
  testRenameProvider(request: RenameProviderTestRequest): Promise<RenameProviderTestResponse>;
  buildRenamePreview(request: RenamePreviewRequest): Promise<RenamePreviewResponse>;
  applyRenamePreview(request: RenameApplyRequest): Promise<RenameApplyResponse>;
  getRenameBatches(): Promise<RenameBatchListResponse>;
  previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse>;
  undoRenameBatch(id: string): Promise<RenameBatchUndoResponse>;
  clearRenameBatches(): Promise<RenameBatchListResponse>;
}

export interface OperationJobsClient {
  getOperationJob(id: string): Promise<OperationJobResponse>;
  cancelOperationJob(id: string): Promise<OperationJobResponse>;
}

export interface RemuxClient {
  buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse>;
  startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse>;
}

export interface PropertyEditClient {
  loadPropEditTemplate(request: PropEditTemplateRequest): Promise<PropEditTemplateResponse>;
  buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse>;
  startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse>;
}

export interface LibraryClient {
  buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse>;
  getLibraryCatalog(request: LibraryCatalogRequest): Promise<LibraryCatalogResponse>;
  getLibraryArtwork(request: LibraryArtworkRequest): Promise<LibraryArtworkResponse>;
  getLibraryLocalArtwork(request: LibraryLocalArtworkRequest): Promise<LibraryArtworkResponse>;
}

export interface OperationLogsClient {
  getOperationLogs(): Promise<{ entries: OperationLogEntry[] }>;
  clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }>;
  exportOperationLogs(): Promise<LogExport>;
}

export interface JobProgressClient {
  subscribeJobProgress(
    subscription: JobProgressSubscription,
    listener: JobProgressListener,
    onError?: JobProgressErrorListener
  ): Promise<Unsubscribe>;
}

/**
 * Full runtime client retained for composition roots and transports. Feature
 * code can accept one of the smaller interfaces above.
 */
export interface BackendClient
  extends BackendTransportClient,
    StatusClient,
    ScanClient,
    SettingsClient,
    RenameClient,
    OperationJobsClient,
    RemuxClient,
    PropertyEditClient,
    LibraryClient,
    OperationLogsClient,
    JobProgressClient {}
