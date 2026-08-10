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
  ThemeDefinition,
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

export type ToolStatus = {
  name: string;
  command: string;
  resolvedPath: string;
  available: boolean;
  version: string;
};

export type SourceRoot = {
  name: string;
  path: string;
};

export type AppStatus = {
  name: string;
  version: string;
  mediaRoot: string;
  configRoot: string;
  sourceRoots: SourceRoot[];
  tools: ToolStatus[];
};

export type TrackRow = {
  id: number;
  trackNumber: number;
  type: string;
  codec: string;
  language: string;
  name: string;
  default: boolean;
  forced: boolean;
};

export type AttachmentRow = {
  id: number;
  fileName: string;
  contentType: string;
  description: string;
  sizeBytes: number | null;
};

export type MediaFileRow = {
  path: string;
  fileName: string;
  extension: string;
  status: string;
  reader: string;
  codec: string;
  resolution: string;
  bitDepth: string;
  hdr: string;
  videoSummary: string;
  audioSummary: string;
  subtitleSummary: string;
  attachmentSummary: string;
  tracks: TrackRow[];
  attachments: AttachmentRow[];
};

export type ScanSummary = {
  total: number;
  mkv: number;
  mp4: number;
  failed: number;
};

export type CurrentScanResponse = {
  updatedUtc: string | null;
  files: MediaFileRow[];
  summary: ScanSummary;
  selectedPaths: string[];
};

export type ScanJobResponse = {
  id: string;
  status: "Queued" | "WaitingForResources" | "Running" | "Canceling" | "Completed" | "Failed" | "Skipped" | "Canceled";
  createdUtc: string;
  startedUtc: string | null;
  completedUtc: string | null;
  currentSource: string;
  completed: number;
  total: number;
  files: MediaFileRow[];
  skipped: string[];
  summary: ScanSummary;
  error: string;
};

export type ScanRequest = {
  sourcePath?: string;
  sources?: string[];
  ignoredFolderNames?: string[];
  mkvMergePath?: string;
  ffProbePath?: string;
};

export type FileSystemEntry = {
  name: string;
  path: string;
  kind: "folder" | "file";
  sizeBytes: number | null;
  modifiedUtc: string;
};

export type FileSystemResponse = {
  path: string;
  parentPath: string | null;
  entries: FileSystemEntry[];
};

export type { ThemeDefinition, WebSettings, WebSettingsRequest };

export type WebMediaServerLibraryPath = {
  id: string;
  name: string;
  type: string;
  serverPath: string;
  containerPath: string;
  isEnabled: boolean;
};

export type WebMediaServer = {
  id: string;
  name: string;
  type: string;
  serverUrl: string;
  hasApiKey: boolean;
  isDefault: boolean;
  lastSyncedUtc: string | null;
  libraries: WebMediaServerLibraryPath[];
};

export type WebMediaServerRequest = {
  id?: string;
  name?: string;
  type?: string;
  serverUrl?: string;
  apiKey?: string;
  isDefault: boolean;
  libraries?: WebMediaServerLibraryPath[];
};

export type WebMediaServerPathMapping = {
  serverPathPrefix: string;
  containerPathPrefix: string;
};

export type MediaServerTestResponse = {
  success: boolean;
  status: string;
  libraryCount: number;
};

export type MediaServerSyncResponse = {
  server: WebMediaServer;
  libraries: WebMediaServerLibraryPath[];
  status: string;
};

export type RenameSearchResult = {
  /**
   * A series id is a number; a film's is the string `movie:<id>`, which is how
   * the host carries the media kind through to the episode lookup. Declaring
   * this as a number was wrong for every film.
   */
  id: number | string;
  name: string;
  year: string;
  overview: string;
  provider: string;
  format: string;
  databaseUrl: string;
  displayName: string;
  providerDisplay: string;
};

export type RenameScopeRow = {
  key: string;
  label: string;
  isSelected: boolean;
};

export type RenamePreviewRow = {
  selected: boolean;
  sourcePath: string;
  currentFileName: string;
  detected: string;
  episodeName: string;
  newFileName: string;
  confidence: string;
  status: string;
  canApply: boolean;
};

export type RenamePreviewResponse = {
  items: RenamePreviewRow[];
  summary: string;
  scopes: RenameScopeRow[];
  status: string;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
};

export type RenameApplyResponse = {
  items: RenamePreviewRow[];
  summary: string;
  status: string;
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

export type RenameBatchListResponse = {
  batches: RenameBatchRecord[];
};

export type RenameBatchUndoPreviewResponse = {
  restorable: number;
  skipped: number;
  lines: string[];
  hasSkippedFiles: boolean;
};

export type RenameBatchRestoreMove = {
  originalPath: string;
  renamedPath: string;
  originalFileName: string;
};

export type RenameBatchUndoResponse = {
  renamed: number;
  skipped: number;
  lines: string[];
  restored: RenameBatchRestoreMove[];
};

export type MuxPreviewRequest = {
  files: MediaFileRow[];
  selectedPaths?: string[];
  removeUnwantedAudioLanguages: boolean;
  keepAudioLanguages: string;
  removeUnwantedSubtitleLanguages: boolean;
  keepSubtitleLanguages: string;
  removeUnwantedTrackIds: boolean;
  removeTrackIdsText: string;
  preserveChapters: boolean;
  preserveAttachments: boolean;
  muxMatchingExternalSubtitles: boolean;
  externalSubtitleLanguage: string;
  externalSubtitleFormats: string;
  preserveExternalSubtitleFiles: boolean;
  skipMuxIfSubtitleAlreadyExists: boolean;
  extractSubtitles: boolean;
  extractSubtitleLanguages: string;
  extractOverwriteExistingFiles: boolean;
  convertMp4ToMkv: boolean;
  deleteMp4AfterConvert: boolean;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
};

export type MuxActionRow = {
  index: number;
  filePath: string;
  fileName: string;
  operation: string;
  toolName: string;
  description: string;
  command: string;
};

export type MuxPreviewResponse = {
  actions: MuxActionRow[];
  noChangeFiles: string[];
  summary: string;
  status: string;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
};

export type PropEditTrackConfigRow = {
  trackNumber: number;
  trackLabel: string;
  type: string;
  currentName: string;
  currentLanguage: string;
  currentDefault: boolean;
  editedName: string;
  editedLanguage: string;
};

export type PropEditTemplateResponse = {
  templatePath: string;
  templateFileName: string;
  audioTracks: PropEditTrackConfigRow[];
  subtitleTracks: PropEditTrackConfigRow[];
  defaultAudio: string;
  forcedAudio: string;
  defaultSubtitle: string;
  forcedSubtitle: string;
};

export type PropEditPreviewRequest = {
  files: MediaFileRow[];
  selectedPaths?: string[];
  templatePath?: string;
  containerTitleMode: "keep" | "file" | "custom" | "remove";
  customContainerTitle: string;
  videoTitleMode: "keep" | "file" | "custom" | "remove";
  customVideoTitle: string;
  audioTracks: PropEditTrackConfigRow[];
  subtitleTracks: PropEditTrackConfigRow[];
  selectedDefaultAudio: string;
  selectedForcedAudio: string;
  selectedDefaultSubtitle: string;
  selectedForcedSubtitle: string;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
};

export type PropEditActionRow = {
  index: number;
  filePath: string;
  fileName: string;
  description: string;
  command: string;
};

export type PropEditSkippedRow = {
  filePath: string;
  fileName: string;
  reason: string;
};

export type PropEditNoChangeRow = {
  filePath: string;
  fileName: string;
  reason: string;
};

export type PropEditPreviewResponse = {
  actions: PropEditActionRow[];
  skipped: PropEditSkippedRow[];
  noChange: PropEditNoChangeRow[];
  summary: string;
  status: string;
  planId?: string;
  planFingerprint?: string;
  idempotencyKey?: string;
};

export type OperationJobResponse = {
  id: string;
  kind: "mux" | "propedit";
  status: "Queued" | "WaitingForResources" | "Running" | "Canceling" | "Completed" | "Failed" | "Skipped" | "Canceled";
  createdUtc: string;
  startedUtc: string | null;
  completedUtc: string | null;
  completed: number;
  failed: number;
  skipped: number;
  total: number;
  currentFile: string;
  currentFilePercent: number;
  lines: string[];
  muxResult: MuxPreviewResponse | null;
  propEditResult: PropEditPreviewResponse | null;
  error: string;
};

export type RenameProviderTestResponse = {
  success: boolean;
  status: string;
};

export type LibraryAuditSummary = {
  groups: number;
  files: number;
  issueGroups: number;
  standardGroups: number;
};

export type LibraryAuditRow = {
  folderPath: string;
  folderName: string;
  fileCount: number;
  standardVideo: string;
  standardAudio: string;
  standardSubtitles: string;
  templateFilePath: string;
  templateFileName: string;
  hasIssues: boolean;
  issueSummary: string;
  issues: string[];
  issueFilePaths: string[];
  allFilePaths: string[];
};

export type LibraryAuditResponse = {
  summary: LibraryAuditSummary;
  items: LibraryAuditRow[];
};

export type OperationLogEntry = {
  timestampUtc: string;
  area: string;
  message: string;
  detail: string;
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
