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
};

export type ScanJobResponse = {
  id: string;
  status: "Queued" | "Running" | "Canceling" | "Completed" | "Failed" | "Canceled";
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

export type WebSettings = {
  hasTvdbApiKey: boolean;
  hasTvdbPin: boolean;
  hasTmdbApiKey: boolean;
  tvdbLanguage: string;
  renameLookupProvider: string;
  renameTemplate: string;
  renameTemplates: string[];
  audioNamePresets: string[];
  subtitleNamePresets: string[];
  languagePresets: string[];
  mkvMergeDefaultAudioLanguages: string;
  mkvMergeDefaultSubtitleLanguages: string;
  watchFolders: string[];
  enableLiveWatchFolderMonitoring: boolean;
  mediaServers: WebMediaServer[];
  mediaServerPathMappings: WebMediaServerPathMapping[];
};

export type WebSettingsRequest = {
  tvdbApiKey?: string;
  tvdbPin?: string;
  tmdbApiKey?: string;
  tvdbLanguage?: string;
  renameLookupProvider?: string;
  renameTemplate?: string;
  renameTemplates?: string[];
  audioNamePresets?: string[];
  subtitleNamePresets?: string[];
  languagePresets?: string[];
  mkvMergeDefaultAudioLanguages?: string;
  mkvMergeDefaultSubtitleLanguages?: string;
  watchFolders?: string[];
  enableLiveWatchFolderMonitoring?: boolean;
  mediaServers?: WebMediaServerRequest[];
  mediaServerPathMappings?: WebMediaServerPathMapping[];
};

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
  id: number;
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
};

export type OperationJobResponse = {
  id: string;
  kind: "mux" | "propedit";
  status: "Queued" | "Running" | "Canceling" | "Completed" | "Failed" | "Canceled";
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

async function fetchJson<T>(input: RequestInfo | URL, init?: RequestInit): Promise<T> {
  const response = await fetch(input, init);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `${response.status} ${response.statusText}`);
  }

  return (await response.json()) as T;
}

export function getStatus(): Promise<AppStatus> {
  return fetchJson<AppStatus>("/api/status");
}

export function browseFileSystem(path?: string): Promise<FileSystemResponse> {
  const query = path ? `?path=${encodeURIComponent(path)}` : "";
  return fetchJson<FileSystemResponse>(`/api/filesystem${query}`);
}

export function startScan(request: ScanRequest): Promise<ScanJobResponse> {
  return fetchJson<ScanJobResponse>("/api/scans", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function getScanJob(id: string): Promise<ScanJobResponse> {
  return fetchJson<ScanJobResponse>(`/api/scans/${encodeURIComponent(id)}`);
}

export function cancelScan(id: string): Promise<ScanJobResponse> {
  return fetchJson<ScanJobResponse>(`/api/scans/${encodeURIComponent(id)}/cancel`, {
    method: "POST"
  });
}

export function getCurrentScanFiles(): Promise<CurrentScanResponse> {
  return fetchJson<CurrentScanResponse>("/api/files/current");
}

export function clearCurrentScanFiles(): Promise<CurrentScanResponse> {
  return fetchJson<CurrentScanResponse>("/api/files/current", {
    method: "DELETE"
  });
}

export function getWebSettings(): Promise<WebSettings> {
  return fetchJson<WebSettings>("/api/settings");
}

export function saveWebSettings(request: WebSettingsRequest): Promise<WebSettings> {
  return fetchJson<WebSettings>("/api/settings", {
    method: "PUT",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function testMediaServerConnection(request: {
  id?: string;
  name?: string;
  type?: string;
  serverUrl?: string;
  apiKey?: string;
}): Promise<MediaServerTestResponse> {
  return fetchJson<MediaServerTestResponse>("/api/media-servers/test", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function syncMediaServerLibraries(id: string): Promise<MediaServerSyncResponse> {
  return fetchJson<MediaServerSyncResponse>(`/api/media-servers/${encodeURIComponent(id)}/sync`, {
    method: "POST"
  });
}

export function searchRenameMetadata(request: {
  query: string;
  provider?: string;
  language?: string;
}): Promise<{ results: RenameSearchResult[] }> {
  return fetchJson<{ results: RenameSearchResult[] }>("/api/rename/search", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function loadRenameScopes(request: {
  selectedResult: RenameSearchResult;
  provider?: string;
  language?: string;
}): Promise<{ scopes: RenameScopeRow[] }> {
  return fetchJson<{ scopes: RenameScopeRow[] }>("/api/rename/scopes", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function testRenameProvider(request: {
  provider?: string;
  language?: string;
}): Promise<RenameProviderTestResponse> {
  return fetchJson<RenameProviderTestResponse>("/api/rename/test-provider", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function buildRenamePreview(request: {
  files: MediaFileRow[];
  selectedResult: RenameSearchResult;
  provider?: string;
  language?: string;
  scopeKeys?: string[];
  template?: string;
}): Promise<RenamePreviewResponse> {
  return fetchJson<RenamePreviewResponse>("/api/rename/preview", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function applyRenamePreview(request: {
  items: RenamePreviewRow[];
  provider?: string;
  template?: string;
}): Promise<RenameApplyResponse> {
  return fetchJson<RenameApplyResponse>("/api/rename/apply", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function getRenameBatches(): Promise<RenameBatchListResponse> {
  return fetchJson<RenameBatchListResponse>("/api/rename/batches");
}

export function previewRenameBatchUndo(id: string): Promise<RenameBatchUndoPreviewResponse> {
  return fetchJson<RenameBatchUndoPreviewResponse>(`/api/rename/batches/${encodeURIComponent(id)}/preview`);
}

export function undoRenameBatch(id: string): Promise<RenameBatchUndoResponse> {
  return fetchJson<RenameBatchUndoResponse>(`/api/rename/batches/${encodeURIComponent(id)}/undo`, {
    method: "POST"
  });
}

export function clearRenameBatches(): Promise<RenameBatchListResponse> {
  return fetchJson<RenameBatchListResponse>("/api/rename/batches", {
    method: "DELETE"
  });
}

export function buildMuxPreview(request: MuxPreviewRequest): Promise<MuxPreviewResponse> {
  return fetchJson<MuxPreviewResponse>("/api/mux/preview", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function startMuxApply(request: MuxPreviewRequest): Promise<OperationJobResponse> {
  return fetchJson<OperationJobResponse>("/api/mux/apply", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function getOperationJob(id: string): Promise<OperationJobResponse> {
  return fetchJson<OperationJobResponse>(`/api/operations/${encodeURIComponent(id)}`);
}

export function cancelOperationJob(id: string): Promise<OperationJobResponse> {
  return fetchJson<OperationJobResponse>(`/api/operations/${encodeURIComponent(id)}/cancel`, {
    method: "POST"
  });
}

export function loadPropEditTemplate(request: { files: MediaFileRow[]; templatePath?: string }): Promise<PropEditTemplateResponse> {
  return fetchJson<PropEditTemplateResponse>("/api/propedit/template", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function buildPropEditPreview(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse> {
  return fetchJson<PropEditPreviewResponse>("/api/propedit/preview", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function startPropEditApply(request: PropEditPreviewRequest): Promise<OperationJobResponse> {
  return fetchJson<OperationJobResponse>("/api/propedit/apply", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
}

export function buildLibraryAudit(files: MediaFileRow[]): Promise<LibraryAuditResponse> {
  return fetchJson<LibraryAuditResponse>("/api/library/audit", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify({ files })
  });
}

export function getOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
  return fetchJson<{ entries: OperationLogEntry[] }>("/api/logs");
}

export function clearOperationLogs(): Promise<{ entries: OperationLogEntry[] }> {
  return fetchJson<{ entries: OperationLogEntry[] }>("/api/logs", {
    method: "DELETE"
  });
}
