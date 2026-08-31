import { getBackendClient } from "./backend/runtime";
import type {
  JobProgressErrorListener,
  JobProgressListener,
  JobProgressSubscription,
  Unsubscribe
} from "./backend/client";
// Settings carry the widest surface of the whole contract, so they are taken
// straight from the generated definitions rather than mirrored by hand. Run
// `cargo run --package mkvo-contract-gen` after changing the Rust contracts.
import type {
  AppStatus,
  CurrentScanResponse,
  FileSystemEntry,
  FileSystemResponse,
  LibraryArtworkRequest,
  LibraryArtworkResponse,
  LibraryAuditResponse,
  LibraryAuditRow,
  LibraryAuditSummary,
  LibraryCatalogItem,
  LibraryCatalogRequest,
  LibraryCatalogResponse,
  LibraryLocalArtworkRequest,
  MediaFileRow,
  MediaServerSyncResponse,
  MediaServerTestResponse,
  MuxActionRow,
  MuxPreviewRequest,
  MuxPreviewResponse,
  OperationJobResponse,
  OperationLogEntry,
  PropEditActionRow,
  PropEditNoChangeRow,
  PropEditPreviewRequest,
  PropEditPreviewResponse,
  PropEditSkippedRow,
  PropEditTemplateResponse,
  PropEditTrackConfigRow,
  RenameApplyResponse,
  RenameBatchListResponse,
  RenameBatchRestoreMove,
  RenameBatchUndoPreviewResponse,
  RenameBatchUndoResponse,
  RenamePreviewResponse,
  RenamePreviewRow,
  RenameProviderTestResponse,
  RenameScopeRow,
  RenameSearchResult,
  ScanJobResponse,
  ScanRequest,
  ScanSummary,
  SourceRoot,
  ThemeDefinition,
  ToolStatus,
  TrackRow,
  WebMediaServer,
  WebMediaServerLibraryPath,
  WebMediaServerPathMapping,
  WebMediaServerRequest,
  WebSettings,
  WebSettingsRequest
} from "./generated/contracts";

export { HttpBackendClient } from "./backend/httpClient";
export { TauriBackendClient } from "./backend/tauriClient";
export { createMockBackendClient } from "./backend/mockClient";
export { ApiError, normalizeApiError } from "./backend/error";
export {
  createBackendClient,
  getBackendClient,
  getBackendTransport,
  isTauriRuntime,
  resetBackendClient,
  setBackendClient
} from "./backend/runtime";
export type {
  BackendClient,
  BackendTransport,
  JobKind,
  JobProgressErrorListener,
  JobProgressEvent,
  JobProgressListener,
  JobProgressSubscription,
  MediaServerConnectionRequest,
  PropEditTemplateRequest,
  RenameApplyRequest,
  RenamePreviewRequest,
  RenameProviderTestRequest,
  RenameScopesRequest,
  RenameSearchRequest,
  Unsubscribe
} from "./backend/client";

export type AttachmentRow = {
  id: number;
  fileName: string;
  contentType: string;
  description: string;
  sizeBytes: number | null;
};

// One definition per wire type. These are re-exported rather than restated,
// so a change on the Rust side reaches every caller through the generator
// instead of waiting for somebody to notice the two copies disagreeing.
export type {
  AppStatus,
  CurrentScanResponse,
  FileSystemEntry,
  FileSystemResponse,
  LibraryArtworkRequest,
  LibraryArtworkResponse,
  LibraryAuditResponse,
  LibraryAuditRow,
  LibraryAuditSummary,
  LibraryCatalogItem,
  LibraryCatalogRequest,
  LibraryCatalogResponse,
  LibraryLocalArtworkRequest,
  MediaFileRow,
  MediaServerSyncResponse,
  MediaServerTestResponse,
  MuxActionRow,
  MuxPreviewRequest,
  MuxPreviewResponse,
  OperationJobResponse,
  OperationLogEntry,
  PropEditActionRow,
  PropEditNoChangeRow,
  PropEditPreviewRequest,
  PropEditPreviewResponse,
  PropEditSkippedRow,
  PropEditTemplateResponse,
  PropEditTrackConfigRow,
  RenameApplyResponse,
  RenameBatchListResponse,
  RenameBatchRestoreMove,
  RenameBatchUndoPreviewResponse,
  RenameBatchUndoResponse,
  RenamePreviewResponse,
  RenamePreviewRow,
  RenameProviderTestResponse,
  RenameScopeRow,
  RenameSearchResult,
  ScanJobResponse,
  ScanRequest,
  ScanSummary,
  SourceRoot,
  ThemeDefinition,
  ToolStatus,
  TrackRow,
  WebMediaServer,
  WebMediaServerLibraryPath,
  WebMediaServerPathMapping,
  WebMediaServerRequest,
  WebSettings,
  WebSettingsRequest
};

export type RenameBatchEntry = {
  originalPath: string;
  renamedPath: string;
  originalFileName: string;
  renamedFileName: string;
};

export type RenameBatchRecord = {
  id: string;
  createdAt: string;
  undoneAt: string | null;
  provider: string;
  template: string;
  totalFiles: number;
  entries: RenameBatchEntry[];
  isUndone: boolean;
  displayName: string;
};

export function getStatus(): Promise<AppStatus> {
  return getBackendClient().getStatus();
}

export function browseFileSystem(path?: string): Promise<FileSystemResponse> {
  return getBackendClient().browseFileSystem(path);
}

export function startScan(request: ScanRequest): Promise<ScanJobResponse> {
  return getBackendClient().startScan(request);
}

export function getScanJob(id: string): Promise<ScanJobResponse> {
  return getBackendClient().getScanJob(id);
}

export function cancelScan(id: string): Promise<ScanJobResponse> {
  return getBackendClient().cancelScan(id);
}

export function getCurrentScanFiles(): Promise<CurrentScanResponse> {
  return getBackendClient().getCurrentScanFiles();
}

export function authorizeBrowsedRoot(path: string): Promise<void> {
  return getBackendClient().authorizeBrowsedRoot(path);
}

export function setFileSelection(paths: string[]): Promise<CurrentScanResponse> {
  return getBackendClient().setFileSelection(paths);
}

export function clearCurrentScanFiles(): Promise<CurrentScanResponse> {
  return getBackendClient().clearCurrentScanFiles();
}

export function getWebSettings(): Promise<WebSettings> {
  return getBackendClient().getWebSettings();
}

export function saveWebSettings(request: WebSettingsRequest): Promise<WebSettings> {
  return getBackendClient().saveWebSettings(request);
}

export function testMediaServerConnection(request: {
  id?: string;
  name?: string;
  type?: string;
  serverUrl?: string;
  apiKey?: string;
}): Promise<MediaServerTestResponse> {
  return getBackendClient().testMediaServerConnection(request);
}

export function syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse> {
  return getBackendClient().syncMediaServerLibraries(id);
}

export function searchRenameMetadata(request: {
  query: string;
  provider?: string;
  language?: string;
}): Promise<{ results: RenameSearchResult[] }> {
  return getBackendClient().searchRenameMetadata(request);
}

export function loadRenameScopes(request: {
  selectedResult: RenameSearchResult;
  provider?: string;
  language?: string;
}): Promise<{ scopes: RenameScopeRow[] }> {
  return getBackendClient().loadRenameScopes(request);
}

export function testRenameProvider(request: {
  provider?: string;
  language?: string;
}): Promise<RenameProviderTestResponse> {
  return getBackendClient().testRenameProvider(request);
}

export function buildRenamePreview(request: {
  files: MediaFileRow[];
  selectedResult: RenameSearchResult;
  provider?: string;
  language?: string;
  scopeKeys?: string[];
  template?: string;
  customSeriesTitle?: string;
}): Promise<RenamePreviewResponse> {
  return getBackendClient().buildRenamePreview(request);
}

export function applyRenamePreview(request: {
  items: RenamePreviewRow[];
  provider?: string;
  template?: string;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
}): Promise<RenameApplyResponse> {
  return getBackendClient().applyRenamePreview(request);
}

export function getRenameBatches(): Promise<RenameBatchListResponse> {
  return getBackendClient().getRenameBatches();
}

export function previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse> {
  return getBackendClient().previewRenameBatchUndo(id);
}

export function undoRenameBatch(id: string): Promise<RenameBatchUndoResponse> {
  return getBackendClient().undoRenameBatch(id);
}

export function clearRenameBatches(): Promise<RenameBatchListResponse> {
  return getBackendClient().clearRenameBatches();
}

export function buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse> {
  return getBackendClient().buildMuxPreview(request);
}

export function startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse> {
  return getBackendClient().startMuxApply(request);
}

export function getOperationJob(id: string): Promise<OperationJobResponse> {
  return getBackendClient().getOperationJob(id);
}

export function cancelOperationJob(id: string): Promise<OperationJobResponse> {
  return getBackendClient().cancelOperationJob(id);
}

export function loadPropEditTemplate(request: { files: MediaFileRow[]; templatePath?: string }): Promise<PropEditTemplateResponse> {
  return getBackendClient().loadPropEditTemplate(request);
}

export function buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse> {
  return getBackendClient().buildPropEditPreview(request);
}

export function startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse> {
  return getBackendClient().startPropEditApply(request);
}

export function buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse> {
  return getBackendClient().buildLibraryAudit(files);
}

export function getLibraryCatalog(request: LibraryCatalogRequest): Promise<LibraryCatalogResponse> {
  return getBackendClient().getLibraryCatalog(request);
}

export function getLibraryArtwork(request: LibraryArtworkRequest): Promise<LibraryArtworkResponse> {
  return getBackendClient().getLibraryArtwork(request);
}

export function getLibraryLocalArtwork(request: LibraryLocalArtworkRequest): Promise<LibraryArtworkResponse> {
  return getBackendClient().getLibraryLocalArtwork(request);
}

export function getOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
  return getBackendClient().getOperationLogs();
}

export function exportOperationLogs() {
  return getBackendClient().exportOperationLogs();
}

export function clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
  return getBackendClient().clearOperationLogs();
}

export function subscribeJobProgress(
  subscription: JobProgressSubscription,
  listener: JobProgressListener,
  onError?: JobProgressErrorListener
): Promise<Unsubscribe> {
  return getBackendClient().subscribeJobProgress(subscription, listener, onError);
}
