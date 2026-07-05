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
  tracks: TrackRow[];
};

export type ScanSummary = {
  total: number;
  mkv: number;
  mp4: number;
  failed: number;
};

export type ScanResponse = {
  startedUtc: string;
  completedUtc: string;
  files: MediaFileRow[];
  skipped: string[];
  summary: ScanSummary;
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
};

export type WebSettingsRequest = {
  tvdbApiKey?: string;
  tvdbPin?: string;
  tmdbApiKey?: string;
  tvdbLanguage?: string;
  renameLookupProvider?: string;
  renameTemplate?: string;
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
  useSafeTempReplacement: boolean;
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

export function scanSources(request: ScanRequest): Promise<ScanResponse> {
  return fetchJson<ScanResponse>("/api/scan", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
  });
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

export function buildRenamePreview(request: {
  files: MediaFileRow[];
  selectedResult: RenameSearchResult;
  provider?: string;
  language?: string;
  scopeKey?: string;
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

export function applyRenamePreview(items: RenamePreviewRow[]): Promise<RenameApplyResponse> {
  return fetchJson<RenameApplyResponse>("/api/rename/apply", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify({ items })
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

export function applyMuxPlan(request: MuxPreviewRequest): Promise<MuxPreviewResponse> {
  return fetchJson<MuxPreviewResponse>("/api/mux/apply", {
    method: "POST",
    headers: {
      "Content-Type": "application/json"
    },
    body: JSON.stringify(request)
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

export function applyPropEditPlan(request: PropEditPreviewRequest): Promise<PropEditPreviewResponse> {
  return fetchJson<PropEditPreviewResponse>("/api/propedit/apply", {
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
