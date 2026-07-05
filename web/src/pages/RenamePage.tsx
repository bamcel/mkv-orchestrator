import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Copy, ExternalLink, RefreshCw, Search, Wand2 } from "lucide-react";
import {
  applyRenamePreview,
  buildRenamePreview,
  getCurrentScanFiles,
  getWebSettings,
  loadRenameScopes,
  MediaFileRow,
  RenamePreviewRow,
  RenameScopeRow,
  RenameSearchResult,
  saveWebSettings,
  searchRenameMetadata
} from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

const renamePreviewCompactStorageKey = "mkvo.web.renamePreviewCompactView";
const renameStateStorageKey = "mkvo.web.renameState";

type StoredRenameState = {
  provider?: string;
  language?: string;
  searchTitle?: string;
  template?: string;
  selectedIndex?: string;
  scopeKey?: string;
  previewRows?: RenamePreviewRow[];
  previewSummary?: string;
  statusText?: string;
  searchResults?: RenameSearchResult[];
  scopeRows?: RenameScopeRow[];
};

function loadRenameState(): StoredRenameState {
  try {
    const saved = window.sessionStorage.getItem(renameStateStorageKey);
    if (!saved) return {};

    const parsed = JSON.parse(saved);
    return parsed && typeof parsed === "object" ? parsed as StoredRenameState : {};
  } catch {
    return {};
  }
}

export function RenamePage() {
  const { files, setFiles, updateFilesAfterRename } = useMediaLibrary();
  const settings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [storedRenameState] = useState<StoredRenameState>(() => loadRenameState());
  const [provider, setProvider] = useState(storedRenameState.provider || "TVDB");
  const [language, setLanguage] = useState(storedRenameState.language || "eng");
  const [searchTitle, setSearchTitle] = useState(storedRenameState.searchTitle || "");
  const [template, setTemplate] = useState(storedRenameState.template || "{series} - S{season:00}E{episode:00} - {episodeTitle}");
  const [selectedIndex, setSelectedIndex] = useState(storedRenameState.selectedIndex || "0");
  const [scopeKey, setScopeKey] = useState(storedRenameState.scopeKey || "");
  const [previewRows, setPreviewRows] = useState<RenamePreviewRow[]>(storedRenameState.previewRows ?? []);
  const [previewSummary, setPreviewSummary] = useState(storedRenameState.previewSummary || "");
  const [statusText, setStatusText] = useState(storedRenameState.statusText || "Scan files on Dashboard, then search metadata.");
  const [searchResults, setSearchResults] = useState<RenameSearchResult[]>(storedRenameState.searchResults ?? []);
  const [scopeRows, setScopeRows] = useState<RenameScopeRow[]>(storedRenameState.scopeRows ?? []);
  const [settingsDefaultsApplied, setSettingsDefaultsApplied] = useState(false);
  const [compactPreview, setCompactPreview] = useState(() => {
    try {
      return window.localStorage.getItem(renamePreviewCompactStorageKey) === "true";
    } catch {
      return false;
    }
  });
  const selectedFiles = files;

  useEffect(() => {
    try {
      window.localStorage.setItem(renamePreviewCompactStorageKey, String(compactPreview));
    } catch {
      // View preference is optional; the page still works without local storage.
    }
  }, [compactPreview]);

  useEffect(() => {
    if (!settings.data || settingsDefaultsApplied) return;
    if (storedRenameState.provider === undefined) setProvider(settings.data.renameLookupProvider || "TVDB");
    if (storedRenameState.language === undefined) setLanguage(settings.data.tvdbLanguage || "eng");
    if (storedRenameState.template === undefined) setTemplate(settings.data.renameTemplate || "{series} - S{season:00}E{episode:00} - {episodeTitle}");
    setSettingsDefaultsApplied(true);
  }, [settings.data, settingsDefaultsApplied, storedRenameState]);

  useEffect(() => {
    try {
      window.sessionStorage.setItem(renameStateStorageKey, JSON.stringify({
        provider,
        language,
        searchTitle,
        template,
        selectedIndex,
        scopeKey,
        previewRows,
        previewSummary,
        statusText,
        searchResults,
        scopeRows
      }));
    } catch {
      // Session restore is optional; the page still works without storage access.
    }
  }, [provider, language, searchTitle, template, selectedIndex, scopeKey, previewRows, previewSummary, statusText, searchResults, scopeRows]);

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
    if (storedRenameState.statusText === undefined) {
      setStatusText(`Loaded ${currentScan.data.files.length} scanned file(s) from Dashboard.`);
    }
  }, [currentScan.data, files.length, setFiles, storedRenameState.statusText]);

  useEffect(() => {
    if (searchTitle || files.length === 0) return;
    const guessed = guessSearchTitle(files.map((file) => file.fileName));
    if (guessed) setSearchTitle(guessed);
  }, [files, searchTitle]);

  useEffect(() => {
    if (files.length === 0 || previewRowsMatchFiles(previewRows, files)) return;

    setPreviewRows(buildScannedFilePreviewRows(files));
    setPreviewSummary("");
    if (searchResults.length === 0) {
      setStatusText(`${files.length} scanned file(s) ready for metadata search.`);
    }
  }, [files, previewRows, searchResults.length]);

  const search = useMutation({
    mutationFn: searchRenameMetadata,
    onSuccess: (response) => {
      setSearchResults(response.results);
      setScopeRows([]);
      setSelectedIndex("0");
      setScopeKey("");
      setPreviewRows([]);
      setPreviewSummary("");
      setStatusText(response.results.length > 0 ? `${provider} results: ${response.results.length}` : `No ${provider} results found.`);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Search failed.")
  });

  const results = searchResults;
  const selectedResult = results[Number(selectedIndex)] ?? null;

  const preview = useMutation({
    mutationFn: buildRenamePreview,
    onSuccess: (response) => {
      setPreviewRows(response.items);
      setPreviewSummary(response.summary);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.")
  });

  const scopes = useMutation({
    mutationFn: loadRenameScopes,
    onSuccess: (response, variables) => {
      setScopeRows(response.scopes);
      const selected = response.scopes.find((scope) => scope.isSelected) ?? response.scopes[0];
      const selectedScopeKey = selected?.key ?? "";
      setScopeKey(selectedScopeKey);

      if (selectedFiles.length > 0) {
        preview.mutate({
          files: selectedFiles,
          selectedResult: variables.selectedResult,
          provider: variables.provider,
          language: variables.language,
          scopeKey: selectedScopeKey,
          template
        });
      }
    }
  });

  useEffect(() => {
    if (!selectedResult) return;
    if (scopeRows.length > 0) return;
    scopes.mutate({ selectedResult, provider, language });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedResult?.id, selectedResult?.format, provider, language, scopeRows.length]);

  const apply = useMutation({
    mutationFn: applyRenamePreview,
    onSuccess: (response) => {
      const renames = response.items.flatMap((result, index) => {
        const original = previewRows[index];
        return result.status === "Renamed" && original
          ? [{ oldPath: original.sourcePath, newPath: result.sourcePath, newFileName: result.currentFileName }]
          : [];
      });

      updateFilesAfterRename(renames);
      setPreviewRows(response.items);
      setPreviewSummary(response.summary);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Rename apply failed.")
  });

  const selectedCount = useMemo(() => previewRows.filter((row) => row.selected).length, [previewRows]);
  const renameTemplates = settings.data?.renameTemplates ?? [
    "{title}",
    "{title} ({year})",
    "{series} - S{season:00}E{episode:00} - {episodeTitle}",
    "{series} ({year}) - S{season:00}E{episode:00} - {episodeTitle}",
    "S{season:00}E{episode:00} - {episodeTitle}",
    "{series} - {absolute:000} - {episodeTitle}"
  ];
  const providerConfigured = provider === "TMDB" ? settings.data?.hasTmdbApiKey : settings.data?.hasTvdbApiKey;

  function runSearch() {
    if (!searchTitle.trim()) {
      setStatusText("Enter a title to search.");
      return;
    }

    saveWebSettings({
      tvdbLanguage: language,
      renameLookupProvider: provider,
      renameTemplate: template
    }).catch(() => undefined);

    search.mutate({ query: searchTitle, provider, language });
  }

  async function refreshScannedFiles() {
    const result = await currentScan.refetch();
    if (result.data?.files.length) {
      setFiles(result.data.files);
      setStatusText(`Loaded ${result.data.files.length} scanned file(s) from Dashboard.`);
    } else {
      setStatusText("No Dashboard scan is available yet.");
    }
  }

  function runPreview() {
    if (!selectedResult) {
      setStatusText("Select a database result first.");
      return;
    }

    preview.mutate({
      files: selectedFiles,
      selectedResult,
      provider,
      language,
      scopeKey,
      template
    });
  }

  function runApply() {
    if (selectedCount === 0) {
      setStatusText("No preview rows selected.");
      return;
    }

    apply.mutate(previewRows);
  }

  function toggleRow(row: RenamePreviewRow) {
    setPreviewRows((current) => current.map((item) =>
      item.sourcePath === row.sourcePath ? { ...item, selected: !item.selected } : item
    ));
  }

  function toggleAll(checked: boolean) {
    setPreviewRows((current) => current.map((row) => ({ ...row, selected: checked && row.canApply })));
  }

  async function copyUrl() {
    if (!selectedResult?.databaseUrl) return;
    await navigator.clipboard.writeText(selectedResult.databaseUrl);
    setStatusText("Database URL copied.");
  }

  function showUndoPending() {
    setStatusText("Undo Batch is not available in the web beta yet.");
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Rename" description="Match files to provider metadata and preview safe destination names." />
      <div className="grid min-h-0 flex-1 grid-cols-[370px_1fr] gap-3">
        <section className="min-h-0 overflow-hidden rounded-lg border border-border bg-card p-3 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex items-center justify-between gap-3">
            <h2 className="text-base font-semibold">Rename Options</h2>
            <button
              type="button"
              onClick={showUndoPending}
              className="h-8 rounded-md border border-border bg-button px-4 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              Undo Batch
            </button>
          </div>
          <p className="mt-3 text-xs leading-5 text-muted">Search, select result, choose scope, pick naming template, then build preview.</p>

          <label className="mt-3 block text-sm font-semibold">Search Title</label>
          <div className="mt-1.5 flex gap-2">
            <input
              value={searchTitle}
              onChange={(event) => setSearchTitle(event.target.value)}
              className="h-9 min-w-0 flex-1 rounded-md border border-border bg-input px-3 text-sm text-text outline-none transition focus:border-accent"
            />
            <button
              type="button"
              onClick={runSearch}
              disabled={search.isPending}
              className="inline-flex h-9 items-center gap-2 rounded-md bg-accent px-3 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:bg-button disabled:text-disabled"
            >
              {search.isPending ? <RefreshCw size={15} className="animate-spin" /> : <Search size={15} />}
              Search
            </button>
          </div>

          <div className="mt-3 text-sm font-semibold">Database Options</div>
          <div className="mt-3 grid grid-cols-2 gap-2">
            <label className="block">
              <select
                value={provider}
                onChange={(event) => {
                  setProvider(event.target.value);
                  setSearchResults([]);
                  setScopeRows([]);
                  setSelectedIndex("0");
                  setScopeKey("");
                  setPreviewRows([]);
                  setPreviewSummary("");
                }}
                className="h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
              >
                <option value="TVDB">TVDB</option>
                <option value="TMDB">TMDB</option>
              </select>
            </label>
            <label className="block">
              <input
                value={language}
                onChange={(event) => {
                  setLanguage(event.target.value);
                  setScopeRows([]);
                  setScopeKey("");
                  setPreviewRows([]);
                  setPreviewSummary("");
                }}
                className="h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
              />
            </label>
          </div>

          {providerConfigured === false ? (
            <div className="mt-3 rounded-md border border-warning bg-input p-3 text-xs text-warning">
              {provider} API key is not configured. Add it in Settings before searching.
            </div>
          ) : null}

          <label className="mt-3 block text-sm font-semibold">Series/Movie</label>
          <select
            value={selectedIndex}
            onChange={(event) => {
              setSelectedIndex(event.target.value);
              setScopeRows([]);
              setScopeKey("");
              setPreviewRows([]);
              setPreviewSummary("");
            }}
            disabled={results.length === 0}
            className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent disabled:text-disabled"
          >
            {results.length === 0 ? (
              <option>No result selected</option>
            ) : results.map((result, index) => (
              <option key={`${result.provider}-${result.format}-${result.id}`} value={String(index)}>
                {result.displayName || result.name} - {result.providerDisplay || `${result.provider} ${result.format}`}
              </option>
            ))}
          </select>

          {selectedResult?.databaseUrl ? (
            <div className="mt-2 flex min-w-0 items-center gap-2 text-xs text-success">
              <span className="min-w-0 truncate">{selectedResult.databaseUrl}</span>
              <a href={selectedResult.databaseUrl} target="_blank" rel="noreferrer" className="shrink-0 text-muted hover:text-text" title="Open">
                <ExternalLink size={14} />
              </a>
              <button type="button" onClick={copyUrl} className="shrink-0 text-muted hover:text-text" title="Copy">
                <Copy size={14} />
              </button>
            </div>
          ) : null}

          <label className="mt-3 block text-sm font-semibold">Episodes</label>
          <select
            value={scopeKey}
            onChange={(event) => setScopeKey(event.target.value)}
            disabled={scopeRows.length === 0}
            className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent disabled:text-disabled"
          >
            {scopeRows.length === 0 ? (
              <option>N/A</option>
            ) : scopeRows.map((scope: RenameScopeRow) => (
              <option key={scope.key} value={scope.key}>{scope.label}</option>
            ))} 
          </select>

          <label className="mt-3 block text-sm font-semibold">Naming Template</label>
          <select
            value={template}
            onChange={(event) => setTemplate(event.target.value)}
            className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
          >
            {renameTemplates.map((item) => (
              <option key={item} value={item}>{item}</option>
            ))}
          </select>
          <input
            value={template}
            onChange={(event) => setTemplate(event.target.value)}
            className="hidden"
          />
          <div className="mt-1 text-[11px] text-muted">Manage templates in Settings &gt; Rename.</div>

          <div className="mt-3 text-sm font-semibold">Execution</div>
          <div className="mt-3 flex gap-2">
            <button
              type="button"
              onClick={runPreview}
              disabled={preview.isPending || selectedFiles.length === 0 || !selectedResult}
              className="inline-flex h-9 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              {preview.isPending ? <RefreshCw size={15} className="animate-spin" /> : <Wand2 size={15} />}
              Preview
            </button>
            <button
              type="button"
              onClick={runApply}
              disabled={apply.isPending || previewRows.length === 0 || selectedCount === 0}
              className="inline-flex h-9 flex-1 items-center justify-center rounded-md bg-accent px-3 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:bg-button disabled:text-disabled"
            >
              Apply
            </button>
          </div>

          <div className="mt-3 line-clamp-2 text-sm text-success">{statusText}</div>
        </section>

        <div className="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_190px] gap-3">
        <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex shrink-0 items-center justify-between">
            <h2 className="text-base font-semibold">Rename Preview</h2>
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={() => setCompactPreview((current) => !current)}
                className="inline-flex h-8 min-w-32 items-center justify-center whitespace-nowrap rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                {compactPreview ? "Detailed View" : "Compact View"}
              </button>
              <div className="text-xs text-muted">{selectedCount} selected</div>
            </div>
          </div>

          <div className="mt-4 min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-panel">
            {previewRows.length === 0 ? (
              <div className="flex h-full min-h-[260px] flex-col items-center justify-center text-center">
                <div className="text-xl font-semibold">No preview built yet</div>
                <div className="mt-2 text-sm text-subtle">Search metadata, select a result, then click Preview.</div>
              </div>
            ) : (
              <div className="h-full overflow-auto">
                <table className={["w-full table-fixed border-collapse text-left text-sm", compactPreview ? "min-w-[760px]" : "min-w-[1180px]"].join(" ")}>
                  <thead className="sticky top-0 bg-panel text-xs uppercase tracking-wide text-subtle">
                    {compactPreview ? (
                      <tr>
                        <th className="border-b border-border px-3 py-2">Current File</th>
                        <th className="border-b border-border px-3 py-2">New Filename</th>
                      </tr>
                    ) : (
                      <tr>
                        <th className="w-[280px] border-b border-border px-3 py-2">Current File</th>
                        <th className="w-24 border-b border-border px-3 py-2">Detected</th>
                        <th className="w-[220px] border-b border-border px-3 py-2">Episode Name</th>
                        <th className="w-[340px] border-b border-border px-3 py-2">New Filename</th>
                        <th className="w-28 border-b border-border px-3 py-2">Confidence</th>
                        <th className="w-[180px] border-b border-border px-3 py-2">Status</th>
                      </tr>
                    )}
                  </thead>
                  <tbody>
                    {previewRows.map((row) => {
                      const changedTextClass = hasFilenameChange(row) ? "text-success" : "";
                      const statusDisplay = getRenameStatusDisplay(row.status);

                      return compactPreview ? (
                        <tr key={row.sourcePath} className={["bg-card hover:bg-selected", changedTextClass].join(" ")}>
                          <td className="truncate border-b border-border px-3 py-2" title={row.sourcePath}>
                            <div className="flex min-w-0 items-center gap-3">
                              <input type="checkbox" checked={row.selected} disabled={!row.canApply} onChange={() => toggleRow(row)} />
                              <span className="truncate">{row.currentFileName}</span>
                            </div>
                          </td>
                          <td className="truncate border-b border-border px-3 py-2" title={row.newFileName}>{row.newFileName || "-"}</td>
                        </tr>
                      ) : (
                        <tr key={row.sourcePath} className={["bg-card hover:bg-selected", changedTextClass].join(" ")}>
                          <td className="max-w-[280px] truncate border-b border-border px-3 py-2" title={row.sourcePath}>
                            <div className="flex min-w-0 items-center gap-3">
                              <input type="checkbox" checked={row.selected} disabled={!row.canApply} onChange={() => toggleRow(row)} />
                              <span className="truncate">{row.currentFileName}</span>
                            </div>
                          </td>
                          <td className="truncate whitespace-nowrap border-b border-border px-3 py-2" title={row.detected}>{row.detected}</td>
                          <td className="max-w-[240px] truncate border-b border-border px-3 py-2" title={row.episodeName}>{row.episodeName || "-"}</td>
                          <td className="max-w-[340px] truncate border-b border-border px-3 py-2" title={row.newFileName}>{row.newFileName || "-"}</td>
                          <td className="truncate whitespace-nowrap border-b border-border px-3 py-2" title={row.confidence}>{row.confidence}</td>
                          <td className={["truncate whitespace-nowrap border-b border-border px-3 py-2", changedTextClass || "text-muted"].join(" ")} title={row.status}>{statusDisplay}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </section>
        <section className="rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-semibold">Preview Summary</h3>
            <button className="h-7 rounded-md bg-button px-3 text-xs font-semibold text-muted">Expand</button>
          </div>
          <pre className="mt-3 h-[125px] overflow-auto whitespace-pre-wrap break-words rounded-md bg-input p-3 font-mono text-xs leading-5 text-muted">
            {previewSummary || "Build a preview to see planned filename changes."}
          </pre>
        </section>
        </div>
      </div>
    </div>
  );
}

function guessSearchTitle(fileNames: string[]) {
  const candidates = fileNames
    .map(detectSearchTitle)
    .filter((title) => title.length > 0);

  if (candidates.length === 0) return "";

  return candidates
    .reduce<Array<{ title: string; count: number }>>((groups, title) => {
      const existing = groups.find((group) => group.title.localeCompare(title, undefined, { sensitivity: "accent" }) === 0);
      if (existing) {
        existing.count += 1;
      } else {
        groups.push({ title, count: 1 });
      }

      return groups;
    }, [])
    .sort((left, right) => right.count - left.count || left.title.localeCompare(right.title))[0].title;
}

function detectSearchTitle(fileName: string) {
  const name = fileName.replace(/\.[^.]+$/, "");
  const patterns = [
    /^(?<title>.*?)(?:[\s._\-[({]+)S\d{1,3}\s*E\d{1,4}\b/i,
    /^(?<title>.*?)(?:[\s._\-[({]+)\d{1,3}x\d{1,4}\b/i,
    /^(?<title>.*?)(?:[\s._\-[({]+)(?:ep|episode)\s*\d{1,4}\b/i,
    /^(?<title>.*?)(?:[\s._\-[({]+)\d{1,4}(?:v\d+)?(?:\s*[-_.].*)?$/i
  ];

  for (const pattern of patterns) {
    const match = name.match(pattern);
    const title = match?.groups?.title ? cleanSearchTitle(match.groups.title) : "";
    if (title) return title;
  }

  return cleanSearchTitle(name);
}

function cleanSearchTitle(value: string) {
  return value
    .replace(/\[[^\]]*\]|\([^\)]*\)/g, " ")
    .replace(/\b(1080p|720p|2160p|480p|bluray|blu[- ]?ray|bdrip|bdremux|web[- ]?dl|webrip|hdtv|dvdrip|x264|x265|h264|h265|hevc|avc|aac|flac|opus|dts|truehd|atmos|10bit|8bit)\b/gi, " ")
    .replace(/\b(season|complete|batch|multi|dual[- ]?audio|remux|proper|repack)\b/gi, " ")
    .replace(/[._-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function buildScannedFilePreviewRows(files: MediaFileRow[]): RenamePreviewRow[] {
  return files.map((file) => ({
    selected: false,
    sourcePath: file.path,
    currentFileName: file.fileName,
    detected: "-",
    episodeName: "-",
    newFileName: "",
    confidence: "-",
    status: "Scanned",
    canApply: false
  }));
}

function previewRowsMatchFiles(rows: RenamePreviewRow[], files: MediaFileRow[]) {
  if (rows.length !== files.length) return false;

  const filePaths = new Set(files.map((file) => file.path));
  return rows.every((row) => filePaths.has(row.sourcePath));
}

function getRenameStatusDisplay(status: string) {
  if (status.startsWith("List order match:")) return "List order match";
  return status;
}

function hasFilenameChange(row: RenamePreviewRow) {
  return row.newFileName.trim().length > 0 && row.currentFileName.trim() !== row.newFileName.trim();
}
