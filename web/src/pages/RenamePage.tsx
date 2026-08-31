import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, ExternalLink, RefreshCw, RotateCcw, Search, Trash2, Wand2, X } from "lucide-react";
import {
  applyRenamePreview,
  buildRenamePreview,
  clearRenameBatches,
  getCurrentScanFiles,
  getRenameBatches,
  getWebSettings,
  loadRenameScopes,
  MediaFileRow,
  previewRenameBatchUndo,
  RenameBatchRecord,
  RenameBatchUndoPreviewResponse,
  RenamePreviewResponse,
  RenamePreviewRow,
  RenameScopeRow,
  RenameSearchResult,
  saveWebSettings,
  searchRenameMetadata,
  undoRenameBatch
} from "../api";
import { OutputModal } from "../components/OutputModal";
import { PreviewSummaryModal } from "../components/PreviewSummaryModal";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

const renamePreviewCompactStorageKey = "mkvo.web.renamePreviewCompactView";
const renameStateStorageKey = "mkvo.web.renameState";

type StoredRenameState = {
  provider?: string;
  language?: string;
  searchTitle?: string;
  /** The last title filled in from a scan, so a typed one is recognisable. */
  autoFilledTitle?: string;
  template?: string;
  customSeriesTitle?: string;
  selectedIndex?: string;
  scopeKey?: string;
  scopeKeys?: string[];
  previewRows?: RenamePreviewRow[];
  previewSummary?: string;
  statusText?: string;
  searchResults?: RenameSearchResult[];
  scopeRows?: RenameScopeRow[];
};

type BatchMovieMatch = {
  file: MediaFileRow;
  query: string;
  results: RenameSearchResult[];
  selectedIndex: number;
  status: string;
};

type BatchMoviePlan = {
  sourcePath: string;
  preview: RenamePreviewResponse;
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
  const { files, updateFilesAfterRename, syncFromBackend } = useMediaLibrary();
  const settings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const queryClient = useQueryClient();
  /**
   * Every page reads the working set through this query, so a rename that only
   * patched this page's copy left the others showing names that no longer
   * exist. Rust moves its own set with the files; this makes the rest of the
   * app re-read it.
   */
  const refreshAfterRename = async () => {
    await queryClient.invalidateQueries({ queryKey: ["current-scan-files"] });
    void queryClient.invalidateQueries({ queryKey: ["propedit-template"] });
  };
  const [storedRenameState] = useState<StoredRenameState>(() => loadRenameState());
  const [renameMode, setRenameMode] = useState<"single" | "batch-movies">("single");
  const [provider, setProvider] = useState(storedRenameState.provider || "TVDB");
  const [language, setLanguage] = useState(storedRenameState.language || "eng");
  const [searchTitle, setSearchTitle] = useState(storedRenameState.searchTitle || "");
  // Remembered so a later scan can tell its own guess from something the user
  // typed. Persisted with the rest of the page, or a reload would make every
  // title look hand-written and auto-fill would never fire again.
  const [autoFilledTitle, setAutoFilledTitle] = useState(storedRenameState.autoFilledTitle || "");
  const [template, setTemplate] = useState(storedRenameState.template || "{series} - S{season:00}E{episode:00} - {episodeTitle}");
  const [customSeriesTitle, setCustomSeriesTitle] = useState(storedRenameState.customSeriesTitle || storedRenameState.searchTitle || "");
  const [selectedIndex, setSelectedIndex] = useState(storedRenameState.selectedIndex || "0");
  const [scopeKeys, setScopeKeys] = useState<string[]>(
    storedRenameState.scopeKeys ?? (storedRenameState.scopeKey ? [storedRenameState.scopeKey] : [])
  );
  const [previewRows, setPreviewRows] = useState<RenamePreviewRow[]>(storedRenameState.previewRows ?? []);
  const [previewSummary, setPreviewSummary] = useState(storedRenameState.previewSummary || "");
  const [statusText, setStatusText] = useState(storedRenameState.statusText || "Scan files on Dashboard, then search metadata.");
  const [searchResults, setSearchResults] = useState<RenameSearchResult[]>(storedRenameState.searchResults ?? []);
  const [scopeRows, setScopeRows] = useState<RenameScopeRow[]>(storedRenameState.scopeRows ?? []);
  const [settingsDefaultsApplied, setSettingsDefaultsApplied] = useState(false);
  const [isUndoOpen, setIsUndoOpen] = useState(false);
  const [isApplyConfirmOpen, setIsApplyConfirmOpen] = useState(false);
  const [isSummaryExpanded, setIsSummaryExpanded] = useState(false);
  const [applyWarnings, setApplyWarnings] = useState<string[]>([]);
  const [selectedUndoBatchId, setSelectedUndoBatchId] = useState("");
  const [pendingProceedBatchId, setPendingProceedBatchId] = useState("");
  const [undoSummaryLines, setUndoSummaryLines] = useState<string[]>([]);
  const [highlightedPreviewPaths, setHighlightedPreviewPaths] = useState<string[]>([]);
  const [previewSelectionAnchor, setPreviewSelectionAnchor] = useState("");
  const [previewSelectionMenu, setPreviewSelectionMenu] = useState<{ x: number; y: number } | null>(null);
  const [batchMatches, setBatchMatches] = useState<BatchMovieMatch[]>([]);
  const [batchPlans, setBatchPlans] = useState<BatchMoviePlan[]>([]);
  const [batchBusy, setBatchBusy] = useState(false);
  const [batchApplying, setBatchApplying] = useState(false);
  const initialScanStatusApplied = useRef(false);
  const [compactPreview, setCompactPreview] = useState(() => {
    try {
      return window.localStorage.getItem(renamePreviewCompactStorageKey) === "true";
    } catch {
      return false;
    }
  });
  const selectedFiles = files;

  useEffect(() => {
    if (!previewSelectionMenu) return;
    const close = () => setPreviewSelectionMenu(null);
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [previewSelectionMenu]);

  useEffect(() => {
    const available = new Set(previewRows.map((row) => normalizeRenamePath(row.sourcePath)));
    setHighlightedPreviewPaths((current) => current.filter((path) => available.has(normalizeRenamePath(path))));
  }, [previewRows.length]);
  const renameBatches = useQuery({
    queryKey: ["rename-batches"],
    queryFn: getRenameBatches,
    enabled: isUndoOpen
  });
  const selectedUndoBatch = useMemo(() => {
    return (renameBatches.data?.batches ?? []).find((batch) => batch.id === selectedUndoBatchId) ?? null;
  }, [renameBatches.data, selectedUndoBatchId]);
  const undoPreview = useQuery({
    queryKey: ["rename-batch-preview", selectedUndoBatchId],
    queryFn: () => previewRenameBatchUndo(selectedUndoBatchId),
    enabled: isUndoOpen && selectedUndoBatchId.length > 0
  });

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
    setCompactPreview(settings.data.renamePreviewCompactView);
    setSettingsDefaultsApplied(true);
  }, [settings.data, settingsDefaultsApplied, storedRenameState]);

  useEffect(() => {
    try {
      window.sessionStorage.setItem(renameStateStorageKey, JSON.stringify({
        provider,
        language,
        searchTitle,
        autoFilledTitle,
        template,
        customSeriesTitle,
        selectedIndex,
        scopeKeys,
        previewRows,
        previewSummary,
        statusText,
        searchResults,
        scopeRows
      }));
    } catch {
      // Session restore is optional; the page still works without storage access.
    }
  }, [provider, language, searchTitle, autoFilledTitle, template, customSeriesTitle, selectedIndex, scopeKeys, previewRows, previewSummary, statusText, searchResults, scopeRows]);

  useEffect(() => {
    if (!currentScan.data) return;
    syncFromBackend(currentScan.data);
    if (currentScan.data.files.length > 0 && storedRenameState.statusText === undefined && !initialScanStatusApplied.current) {
      initialScanStatusApplied.current = true;
      setStatusText(`Loaded ${currentScan.data.files.length} scanned file(s) from Dashboard.`);
    }
  }, [currentScan.data, storedRenameState.statusText]);

  // Follow the scan. This used to fire only into an empty field, and the field
  // is persisted, so the title from the first scan stuck for the whole session
  // and a later scan of a different show kept searching for the old one.
  //
  // A title the user typed is theirs and is left alone; one that is still what
  // a previous scan put there is replaced.
  const scannedFileNames = files.map((file) => file.fileName).join("\u0000");
  useEffect(() => {
    if (files.length === 0) return;
    const guessed = guessSearchTitle(files.map((file) => file.fileName));
    if (!guessed || guessed === searchTitle) return;

    // A new scan replaces the title outright, including one typed by hand.
    // Keeping the old one meant scanning a second film and searching for the
    // first, which is never what was wanted; the field is still editable
    // afterwards, and nothing is searched until Search is pressed.
    setSearchTitle(guessed);
    setAutoFilledTitle(guessed);
    setCustomSeriesTitle(guessed);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scannedFileNames]);

  useEffect(() => {
    if (!isUndoOpen) return;

    const batches = renameBatches.data?.batches ?? [];
    if (batches.length === 0) {
      setSelectedUndoBatchId("");
      setPendingProceedBatchId("");
      setUndoSummaryLines(["No rename batches have been recorded yet."]);
      return;
    }

    if (!selectedUndoBatchId || !batches.some((batch) => batch.id === selectedUndoBatchId)) {
      setSelectedUndoBatchId(batches[0].id);
      setPendingProceedBatchId("");
    }
  }, [isUndoOpen, renameBatches.data, selectedUndoBatchId]);

  useEffect(() => {
    if (!isUndoOpen || !selectedUndoBatch || !undoPreview.data) return;

    setUndoSummaryLines(buildUndoSummaryLines(selectedUndoBatch, undoPreview.data));
  }, [isUndoOpen, selectedUndoBatch, undoPreview.data]);

  useEffect(() => {
    if (renameMode === "batch-movies") return;
    if (files.length === 0 || previewRowsMatchFiles(previewRows, files)) return;

    setPreviewRows(buildScannedFilePreviewRows(files));
    setPreviewSummary("");
    if (searchResults.length === 0) {
      setStatusText(`${files.length} scanned file(s) ready for metadata search.`);
    }
  }, [renameMode, files, previewRows, searchResults.length]);

  const search = useMutation({
    mutationFn: searchRenameMetadata,
    onSuccess: (response) => {
      setSearchResults(response.results);
      setScopeRows([]);
      setSelectedIndex("0");
      setCustomSeriesTitle(response.results[0]?.name ?? searchTitle);
      setScopeKeys([]);
      setPreviewRows([]);
      setPreviewSummary("");
      setStatusText(response.results.length > 0 ? `${provider} results: ${response.results.length}` : `No ${provider} results found.`);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Search failed.")
  });

  const results = searchResults;
  const selectedResult = results[Number(selectedIndex)] ?? null;
  // A film has no seasons or episodes to choose between, so the scope list is
  // disabled rather than left looking like a choice that was simply not made.
  const selectedIsMovie = selectedResult?.format.toLowerCase() === "movie";

  const preview = useMutation({
    mutationFn: buildRenamePreview,
    onSuccess: (response) => {
      setPreviewRows(response.items.map((item) => ({ ...item, selected: item.canApply })));
      setPreviewSummary(response.summary);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.")
  });

  const scopes = useMutation({
    mutationFn: loadRenameScopes,
    onSuccess: (response, variables) => {
      setScopeRows(response.scopes);
      const selected = response.scopes.filter((scope) => scope.isSelected).map((scope) => scope.key);
      const selectedScopeKeys = selected.length > 0 ? selected : response.scopes.slice(0, 1).map((scope) => scope.key);
      setScopeKeys(selectedScopeKeys);

      if (selectedFiles.length > 0) {
        preview.mutate({
          files: selectedFiles,
          selectedResult: variables.selectedResult,
          provider: variables.provider,
          language: variables.language,
          scopeKeys: selectedScopeKeys,
          template,
          customSeriesTitle
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
    onMutate: (request) => {
      const total = request.items.filter((row) => row.selected && row.canApply).length;
      setStatusText(`0 of ${total} complete`);
    },
    onSuccess: async (response) => {
      const renames = response.items.flatMap((result, index) => {
        const original = previewRows[index];
        return result.status === "Renamed" && original
          ? [{ oldPath: original.sourcePath, newPath: result.sourcePath, newFileName: result.currentFileName }]
          : [];
      });

      updateFilesAfterRename(renames);
      await refreshAfterRename();
      setPreviewRows(response.items);
      setPreviewSummary(response.summary);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Rename apply failed.")
  });

  useEffect(() => {
    if (!apply.isPending || !apply.variables) return;
    const pendingRows = apply.variables.items.filter((row) => row.selected && row.canApply);
    let stopped = false;
    let polling = false;

    const updateRenameProgress = async () => {
      if (polling) return;
      polling = true;
      try {
        const result = await currentScan.refetch();
        if (stopped || !result.data) return;
        syncFromBackend(result.data);
        const completed = countCompletedRenameRows(pendingRows, result.data.files);
        setStatusText(`${completed} of ${pendingRows.length} complete`);
      } finally {
        polling = false;
      }
    };

    void updateRenameProgress();
    const interval = window.setInterval(() => void updateRenameProgress(), 750);
    return () => {
      stopped = true;
      window.clearInterval(interval);
    };
    // Polling is tied to this one mutation. Library synchronization causes
    // renders but must not restart the timer or issue overlapping requests.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [apply.isPending]);

  const undoBatch = useMutation({
    mutationFn: undoRenameBatch,
    onSuccess: (response) => {
      const restoreMoves = response.restored.map((item) => ({
        oldPath: item.renamedPath,
        newPath: item.originalPath,
        newFileName: item.originalFileName,
        status: "Rename undone"
      }));

      updateFilesAfterRename(restoreMoves);
      void refreshAfterRename();
      setPreviewRows((current) => current.map((row) => {
        const move = restoreMoves.find((item) => item.oldPath.toLowerCase() === row.sourcePath.toLowerCase());
        return move
          ? {
              ...row,
              sourcePath: move.newPath,
              currentFileName: move.newFileName,
              newFileName: "",
              status: "Rename undone",
              selected: false,
              canApply: false
            }
          : row;
      }));
      setUndoSummaryLines(response.lines);
      setPendingProceedBatchId("");
      setStatusText(`Undo batch complete: ${response.renamed} restored, ${response.skipped} skipped`);
      renameBatches.refetch();
    },
    onError: (error) => setUndoSummaryLines([error instanceof Error ? error.message : "Undo batch failed."])
  });

  const clearBatches = useMutation({
    mutationFn: clearRenameBatches,
    onSuccess: () => {
      setSelectedUndoBatchId("");
      setPendingProceedBatchId("");
      setUndoSummaryLines(["Rename undo batch history cleared."]);
      renameBatches.refetch();
    },
    onError: (error) => setUndoSummaryLines([error instanceof Error ? error.message : "Clear batch list failed."])
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
  const languageOptions = useMemo(
    () => buildOptionList(settings.data?.languagePresets ?? ["eng", "jpn", "spa", "fre", "ger", "und"], language),
    [settings.data?.languagePresets, language]
  );

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

  async function matchBatchMovies() {
    if (selectedFiles.length === 0) {
      setStatusText("Scan movie files before matching a batch.");
      return;
    }
    setBatchBusy(true);
    setBatchPlans([]);
    setPreviewRows([]);
    setPreviewSummary("");
    setStatusText(`Matching ${selectedFiles.length} movie file(s)...`);
    const matches: BatchMovieMatch[] = [];
    for (const file of selectedFiles) {
      const query = batchMatches.find((item) => normalizeRenamePath(item.file.path) === normalizeRenamePath(file.path))?.query
        || guessSearchTitle([file.fileName])
        || file.fileName.replace(/\.[^.]+$/, "");
      try {
        const response = await searchRenameMetadata({ query, provider, language });
        const movieResults = response.results.filter((result) => result.format.toLowerCase() === "movie");
        matches.push({
          file,
          query,
          results: movieResults,
          selectedIndex: 0,
          status: movieResults.length > 0 ? `${movieResults.length} movie match(es)` : "No movie match"
        });
      } catch (error) {
        matches.push({
          file,
          query,
          results: [],
          selectedIndex: 0,
          status: error instanceof Error ? error.message : "Search failed"
        });
      }
    }
    setBatchMatches(matches);
    const matched = matches.filter((item) => item.results.length > 0).length;
    setStatusText(`Batch matching complete: ${matched} matched, ${matches.length - matched} need attention.`);
    setBatchBusy(false);
  }

  function updateBatchMatch(index: number, patch: Partial<BatchMovieMatch>) {
    setBatchMatches((current) => current.map((item, itemIndex) => itemIndex === index ? { ...item, ...patch } : item));
    setBatchPlans([]);
    setPreviewRows([]);
    setPreviewSummary("");
  }

  async function previewBatchMovies() {
    const matched = batchMatches.filter((item) => item.results[item.selectedIndex]);
    if (matched.length === 0) {
      setStatusText("Match at least one movie before building a preview.");
      return;
    }
    setBatchBusy(true);
    setStatusText(`Building ${matched.length} movie rename preview(s)...`);
    const plans: BatchMoviePlan[] = [];
    const rows: RenamePreviewRow[] = [];
    for (const item of matched) {
      const result = item.results[item.selectedIndex];
      try {
        const response = await buildRenamePreview({
          files: [item.file],
          selectedResult: result,
          provider,
          language,
          scopeKeys: ["all"],
          template,
          customSeriesTitle
        });
        plans.push({ sourcePath: item.file.path, preview: response });
        rows.push(...response.items.map((row) => ({ ...row, selected: row.canApply })));
      } catch (error) {
        rows.push(makeUnavailableRenameRow(
          item.file,
          error instanceof Error ? error.message : "Preview failed"
        ));
      }
    }
    setBatchPlans(plans);
    setPreviewRows(rows);
    setPreviewSummary(`${rows.filter((row) => row.canApply).length} movie rename(s) ready; ${rows.filter((row) => !row.canApply).length} need attention.`);
    setStatusText("Batch Movies preview ready.");
    setBatchBusy(false);
  }

  async function applyBatchMovies() {
    const selected = new Map(
      previewRows.map((row) => [normalizeRenamePath(row.sourcePath), row.selected && row.canApply])
    );
    const plans = batchPlans.filter((plan) => selected.get(normalizeRenamePath(plan.sourcePath)));
    if (plans.length === 0) {
      setStatusText("No batch movie preview rows are selected.");
      return;
    }
    setBatchApplying(true);
    let renamed = 0;
    let skipped = 0;
    const moves: Array<{ oldPath: string; newPath: string; newFileName: string }> = [];
    for (const plan of plans) {
      try {
        const response = await applyRenamePreview({
          items: plan.preview.items.map((row) => ({ ...row, selected: true })),
          provider,
          template,
          planId: plan.preview.planId ?? undefined,
          planFingerprint: plan.preview.planFingerprint ?? undefined,
          idempotencyKey: plan.preview.idempotencyKey ?? crypto.randomUUID()
        });
        const renamedItems = response.items.filter((item) => item.status === "Renamed").length;
        renamed += renamedItems;
        skipped += response.items.length - renamedItems;
        const original = plan.preview.items[0];
        const applied = response.items[0];
        if (original && applied?.status === "Renamed") {
          moves.push({ oldPath: original.sourcePath, newPath: applied.sourcePath, newFileName: applied.currentFileName });
        }
      } catch {
        skipped += 1;
      }
    }
    updateFilesAfterRename(moves);
    void refreshAfterRename();
    setBatchApplying(false);
    setStatusText(`Batch Movies complete: ${renamed} renamed, ${skipped} skipped.`);
    setPreviewRows([]);
    setBatchPlans([]);
  }

  function toggleCompactPreview() {
    setCompactPreview((current) => {
      const next = !current;
      void saveWebSettings({ renamePreviewCompactView: next }).catch((error: unknown) => {
        setStatusText(error instanceof Error ? error.message : "Compact-view preference could not be saved.");
      });
      return next;
    });
  }

  async function refreshScannedFiles() {
    const result = await currentScan.refetch();
    if (result.data?.files.length) {
      syncFromBackend(result.data);
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
      scopeKeys,
      template,
      customSeriesTitle
    });
  }

  function toggleScope(key: string) {
    setScopeKeys((current) => {
      // Scope keys are backend contracts (`all`, `season:N`). The all-episodes
      // shortcut is exclusive; concrete seasons combine with each other.
      if (key === "all") {
        return current.includes("all") ? [] : ["all"];
      }

      const withoutAll = current.filter((item) => item !== "all");
      return withoutAll.includes(key)
        ? withoutAll.filter((item) => item !== key)
        : [...withoutAll, key];
    });
  }

  function runApply() {
    if (selectedCount === 0) {
      setStatusText("No preview rows selected.");
      return;
    }

    const warnings = buildRenameApplyWarnings(previewRows);
    if (warnings.length > 0) {
      setApplyWarnings(warnings);
      setIsApplyConfirmOpen(true);
      return;
    }

    executeApply();
  }

  function executeApply() {
    setIsApplyConfirmOpen(false);
    apply.mutate({
      items: previewRows,
      provider,
      template,
      planId: preview.data?.planId ?? undefined,
      planFingerprint: preview.data?.planFingerprint ?? undefined,
      idempotencyKey: preview.data?.idempotencyKey ?? crypto.randomUUID()
    });
  }

  function toggleRow(row: RenamePreviewRow) {
    setPreviewRows((current) => current.map((item) =>
      item.sourcePath === row.sourcePath ? { ...item, selected: !item.selected } : item
    ));
  }

  function toggleAll(checked: boolean) {
    setPreviewRows((current) => current.map((row) => ({ ...row, selected: checked && row.canApply })));
  }

  function highlightPreviewRow(row: RenamePreviewRow, index: number, modifiers: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) {
    if (modifiers.shiftKey && previewSelectionAnchor) {
      const anchorIndex = previewRows.findIndex((item) => normalizeRenamePath(item.sourcePath) === normalizeRenamePath(previewSelectionAnchor));
      if (anchorIndex >= 0) {
        const start = Math.min(anchorIndex, index);
        const end = Math.max(anchorIndex, index);
        setHighlightedPreviewPaths(previewRows.slice(start, end + 1).map((item) => item.sourcePath));
        return;
      }
    }
    if (modifiers.ctrlKey || modifiers.metaKey) {
      setHighlightedPreviewPaths((current) => current.some((path) => normalizeRenamePath(path) === normalizeRenamePath(row.sourcePath))
        ? current.filter((path) => normalizeRenamePath(path) !== normalizeRenamePath(row.sourcePath))
        : [...current, row.sourcePath]);
      setPreviewSelectionAnchor(row.sourcePath);
      return;
    }
    setHighlightedPreviewPaths([row.sourcePath]);
    setPreviewSelectionAnchor(row.sourcePath);
  }

  function setHighlightedPreviewSelection(checked: boolean) {
    const targets = new Set(highlightedPreviewPaths.map(normalizeRenamePath));
    setPreviewRows((current) => current.map((row) => targets.has(normalizeRenamePath(row.sourcePath))
      ? { ...row, selected: checked && row.canApply }
      : row));
  }

  function toggleHighlightedPreviewSelection() {
    if (highlightedPreviewPaths.length === 0) return;
    const targets = new Set(highlightedPreviewPaths.map(normalizeRenamePath));
    const selectable = previewRows.filter((row) => row.canApply && targets.has(normalizeRenamePath(row.sourcePath)));
    setHighlightedPreviewSelection(!selectable.every((row) => row.selected));
  }

  async function copyUrl() {
    if (!selectedResult?.databaseUrl) return;
    await navigator.clipboard.writeText(selectedResult.databaseUrl);
    setStatusText("Database URL copied.");
  }

  function openUndoBatch() {
    setIsUndoOpen(true);
    renameBatches.refetch();
  }

  function selectUndoBatch(id: string) {
    setSelectedUndoBatchId(id);
    setPendingProceedBatchId("");
    setUndoSummaryLines(["Loading restore plan..."]);
  }

  function runUndoSelected() {
    if (!selectedUndoBatch) return;

    if (undoPreview.data?.hasSkippedFiles && pendingProceedBatchId !== selectedUndoBatch.id) {
      setPendingProceedBatchId(selectedUndoBatch.id);
      setUndoSummaryLines([
        ...buildUndoSummaryLines(selectedUndoBatch, undoPreview.data),
        "",
        "Some files will be skipped. Click Proceed Anyway to restore the remaining files, or Close to cancel."
      ]);
      return;
    }

    undoBatch.mutate(selectedUndoBatch.id);
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Rename" description="Match files to provider metadata and preview safe destination names." />
      <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[18.75rem_minmax(0,1fr)] gap-3">
        <section className="min-h-0 overflow-x-hidden overflow-y-auto rounded-lg border border-border bg-card p-3 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
          <div className="flex items-center justify-between gap-3">
            <h2 className="text-base font-semibold">Rename Options</h2>
            <button
              type="button"
              onClick={openUndoBatch}
              className="h-9 shrink-0 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              Undo Batch
            </button>
          </div>
          <p className="mt-3 text-xs leading-5 text-muted">Search, select result, choose scope, pick naming template, then build preview.</p>

          <div className="mt-3 flex gap-5 text-sm">
            <button
              type="button"
              onClick={() => setRenameMode("single")}
              className={["pb-1 font-semibold", renameMode === "single" ? "border-b border-accent text-text" : "text-muted hover:text-text"].join(" ")}
            >
              Series / Movie
            </button>
            <button
              type="button"
              onClick={() => setRenameMode("batch-movies")}
              className={["pb-1 font-semibold", renameMode === "batch-movies" ? "border-b border-accent text-text" : "text-muted hover:text-text"].join(" ")}
            >
              Batch Movies
            </button>
          </div>

          <div className={renameMode === "single" ? "" : "hidden"}>

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
              className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-md bg-accent px-2.5 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:bg-button disabled:text-disabled"
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
                  setScopeKeys([]);
                  setPreviewRows([]);
                  setPreviewSummary("");
                }}
                className="h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
              >
                <option value="TVDB">TVDB</option>
                <option value="TMDB">TMDB</option>
                <option value="AniDB">AniDB</option>
                <option value="AniList">AniList</option>
              </select>
            </label>
            <label className="block">
              <select
                value={language}
                onChange={(event) => {
                  setLanguage(event.target.value);
                  setScopeRows([]);
                  setScopeKeys([]);
                  setPreviewRows([]);
                  setPreviewSummary("");
                }}
                className="h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
              >
                {languageOptions.map((option) => <option key={option} value={option}>{option}</option>)}
              </select>
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
              const nextIndex = event.target.value;
              setSelectedIndex(nextIndex);
              setCustomSeriesTitle(results[Number(nextIndex)]?.name ?? searchTitle);
              setScopeRows([]);
              setScopeKeys([]);
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

          <label
            className={["mt-3 block text-sm font-semibold", selectedIsMovie ? "text-disabled" : ""].join(" ")}
            id="episode-scope-label"
          >
            Episodes
          </label>
          <div
            className={[
              "mt-1.5 max-h-36 overflow-auto rounded-md border border-border px-3 py-2",
              selectedIsMovie ? "bg-panel opacity-60" : "bg-input"
            ].join(" ")}
            aria-labelledby="episode-scope-label"
            aria-disabled={selectedIsMovie}
          >
            {selectedIsMovie ? (
              <div className="text-sm text-disabled">Not applicable to a movie</div>
            ) : scopeRows.length === 0 ? (
              <div className="text-sm text-disabled">N/A</div>
            ) : scopeRows.map((scope: RenameScopeRow) => (
              <label key={scope.key} className="flex h-7 items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={scopeKeys.includes(scope.key)}
                  onChange={() => toggleScope(scope.key)}
                />
                <span className="truncate" title={scope.label}>{scope.label}</span>
              </label>
            ))}
          </div>

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
          <div className="mt-1 text-[0.6875rem] text-muted">Manage templates in Settings &gt; Rename.</div>

          <label className={["mt-3 block text-sm font-semibold", selectedIsMovie ? "text-disabled" : ""].join(" ")} htmlFor="rename-custom-series">
            Custom {"{series}"}
          </label>
          <input
            id="rename-custom-series"
            value={customSeriesTitle}
            disabled={selectedIsMovie}
            onChange={(event) => setCustomSeriesTitle(event.target.value)}
            placeholder="Series title used by {series}"
            className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent disabled:text-disabled"
          />

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
          </div>

          <div className={renameMode === "batch-movies" ? "" : "hidden"}>
            <p className="mt-3 text-xs leading-5 text-muted">Search each scanned filename independently, review its movie match, then preview the batch.</p>

            <div className="mt-3 text-sm font-semibold">Database Options</div>
            <div className="mt-2 grid grid-cols-2 gap-2">
              <select value={provider} onChange={(event) => { setProvider(event.target.value); setBatchMatches([]); setBatchPlans([]); setPreviewRows([]); }} className="h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent">
                <option value="TVDB">TVDB</option>
                <option value="TMDB">TMDB</option>
                <option value="AniDB">AniDB</option>
                <option value="AniList">AniList</option>
              </select>
              <select value={language} onChange={(event) => { setLanguage(event.target.value); setBatchMatches([]); setBatchPlans([]); setPreviewRows([]); }} className="h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent">
                {languageOptions.map((option) => <option key={option} value={option}>{option}</option>)}
              </select>
            </div>

            {providerConfigured === false ? (
              <div className="mt-3 rounded-md border border-warning bg-input p-3 text-xs text-warning">{provider} API key is not configured. Add it in Settings before matching.</div>
            ) : null}

            <button type="button" onClick={matchBatchMovies} disabled={batchBusy || selectedFiles.length === 0} className="mt-3 inline-flex h-9 w-full items-center justify-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              {batchBusy && batchMatches.length === 0 ? <RefreshCw size={15} className="animate-spin" /> : <Search size={15} />}
              Match {selectedFiles.length} Movie File(s)
            </button>

            <div className="mt-3 max-h-56 space-y-2 overflow-auto">
              {batchMatches.map((item, index) => (
                <div key={item.file.path} className="rounded-md border border-border bg-panel p-2">
                  <div className="truncate text-xs font-semibold text-text" title={item.file.fileName}>{item.file.fileName}</div>
                  <input value={item.query} onChange={(event) => updateBatchMatch(index, { query: event.target.value })} className="mt-2 h-8 w-full rounded-md border border-border bg-input px-2 text-xs text-text outline-none focus:border-accent" />
                  <select value={String(item.selectedIndex)} onChange={(event) => updateBatchMatch(index, { selectedIndex: Number(event.target.value) })} disabled={item.results.length === 0} className="mt-2 h-8 w-full rounded-md border border-border bg-input px-2 text-xs text-text outline-none focus:border-accent disabled:text-disabled">
                    {item.results.length === 0 ? <option>No movie match</option> : item.results.map((result, resultIndex) => (
                      <option key={`${result.provider}-${result.id}-${resultIndex}`} value={String(resultIndex)}>{result.displayName || `${result.name} ${result.year}`}</option>
                    ))}
                  </select>
                  <div className={["mt-1 text-[0.6875rem]", item.results.length > 0 ? "text-success" : "text-warning"].join(" ")}>{item.status}</div>
                </div>
              ))}
            </div>

            <label className="mt-3 block text-sm font-semibold">Naming Template</label>
            <select value={template} onChange={(event) => { setTemplate(event.target.value); setBatchPlans([]); setPreviewRows([]); }} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent">
              {renameTemplates.map((item) => <option key={item} value={item}>{item}</option>)}
            </select>

            <div className="mt-3 text-sm font-semibold">Execution</div>
            <div className="mt-3 flex gap-2">
              <button type="button" onClick={previewBatchMovies} disabled={batchBusy || batchMatches.every((item) => item.results.length === 0)} className="inline-flex h-9 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
                {batchBusy && batchMatches.length > 0 ? <RefreshCw size={15} className="animate-spin" /> : <Wand2 size={15} />}
                Preview
              </button>
              <button type="button" onClick={applyBatchMovies} disabled={batchApplying || batchPlans.length === 0 || selectedCount === 0} className="h-9 flex-1 rounded-md bg-accent px-3 text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
                {batchApplying ? "Applying..." : "Apply"}
              </button>
            </div>
            <div className="mt-3 line-clamp-2 text-sm text-success">{statusText}</div>
          </div>
        </section>

        <div className="min-h-0 min-w-0">
        <section className="flex h-full min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
          <div className="flex shrink-0 items-center justify-between">
            <h2 className="text-base font-semibold">Rename Preview</h2>
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={() => setIsSummaryExpanded(true)}
                className="inline-flex h-9 min-w-32 items-center justify-center whitespace-nowrap rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                Preview Summary
              </button>
              <button
                type="button"
                  onClick={toggleCompactPreview}
                className="inline-flex h-9 min-w-32 items-center justify-center whitespace-nowrap rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                {compactPreview ? "Detailed View" : "Compact View"}
              </button>
              <div className="text-xs text-muted">{selectedCount} selected</div>
            </div>
          </div>

          <div className="mt-4 min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-panel">
            {previewRows.length === 0 ? (
              <div className="flex h-full min-h-[16.25rem] flex-col items-center justify-center text-center">
                <div className="text-xl font-semibold">No preview built yet</div>
                <div className="mt-2 text-sm text-subtle">
                  {renameMode === "batch-movies" ? "Match the scanned movies, review each result, then click Preview." : "Search metadata, select a result, then click Preview."}
                </div>
              </div>
            ) : (
              <div
                className="h-full overflow-auto outline-none"
                tabIndex={0}
                aria-label="Rename file selection"
                onKeyDown={(event) => {
                  if (event.key !== " " || highlightedPreviewPaths.length === 0) return;
                  event.preventDefault();
                  toggleHighlightedPreviewSelection();
                }}
              >
                <table className={["w-full table-fixed border-collapse text-left text-sm", compactPreview ? "min-w-[47.5rem]" : "min-w-[73.75rem]"].join(" ")}>
                  <thead className="sticky top-0 bg-panel text-xs uppercase tracking-wide text-subtle">
                    {compactPreview ? (
                      <tr>
                        <th className="border-b border-border px-3 py-2">Current File</th>
                        <th className="border-b border-border px-3 py-2">New Filename</th>
                      </tr>
                    ) : (
                      <tr>
                        <th className="w-[17.5rem] border-b border-border px-3 py-2">Current File</th>
                        <th className="w-24 border-b border-border px-3 py-2">Detected</th>
                        <th className="w-[13.75rem] border-b border-border px-3 py-2">Episode Name</th>
                        <th className="w-[21.25rem] border-b border-border px-3 py-2">New Filename</th>
                        <th className="w-28 border-b border-border px-3 py-2">Confidence</th>
                        <th className="w-[11.25rem] border-b border-border px-3 py-2">Status</th>
                      </tr>
                    )}
                  </thead>
                  <tbody>
                    {previewRows.map((row, index) => {
                      // Only the destination is coloured, and only when it
                      // actually differs: green has to mean "this name is
                      // changing", not "this row exists".
                      const changedTextClass = hasFilenameChange(row) ? "text-success" : "";
                      const statusDisplay = getRenameStatusDisplay(row.status);

                      return compactPreview ? (
                        <tr
                          key={row.sourcePath}
                          onClick={(event) => highlightPreviewRow(row, index, event)}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            if (!highlightedPreviewPaths.some((path) => normalizeRenamePath(path) === normalizeRenamePath(row.sourcePath))) {
                              setHighlightedPreviewPaths([row.sourcePath]);
                              setPreviewSelectionAnchor(row.sourcePath);
                            }
                            setPreviewSelectionMenu({ x: event.clientX, y: event.clientY });
                          }}
                          className={[highlightedPreviewPaths.some((path) => normalizeRenamePath(path) === normalizeRenamePath(row.sourcePath)) ? "bg-selected" : "bg-card", "cursor-pointer hover:bg-selected"].join(" ")}
                        >
                          <td className="truncate border-b border-border px-3 py-2" title={row.sourcePath}>
                            <div className="flex min-w-0 items-center gap-3">
                              <input type="checkbox" checked={row.selected} disabled={!row.canApply} onClick={(event) => event.stopPropagation()} onChange={() => toggleRow(row)} />
                              <span className="truncate">{row.currentFileName}</span>
                            </div>
                          </td>
                          <td className={["truncate border-b border-border px-3 py-2", changedTextClass].join(" ")} title={row.newFileName}>{row.newFileName || "-"}</td>
                        </tr>
                      ) : (
                        <tr
                          key={row.sourcePath}
                          onClick={(event) => highlightPreviewRow(row, index, event)}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            if (!highlightedPreviewPaths.some((path) => normalizeRenamePath(path) === normalizeRenamePath(row.sourcePath))) {
                              setHighlightedPreviewPaths([row.sourcePath]);
                              setPreviewSelectionAnchor(row.sourcePath);
                            }
                            setPreviewSelectionMenu({ x: event.clientX, y: event.clientY });
                          }}
                          className={[highlightedPreviewPaths.some((path) => normalizeRenamePath(path) === normalizeRenamePath(row.sourcePath)) ? "bg-selected" : "bg-card", "cursor-pointer hover:bg-selected"].join(" ")}
                        >
                          <td className="max-w-[17.5rem] truncate border-b border-border px-3 py-2" title={row.sourcePath}>
                            <div className="flex min-w-0 items-center gap-3">
                              <input type="checkbox" checked={row.selected} disabled={!row.canApply} onClick={(event) => event.stopPropagation()} onChange={() => toggleRow(row)} />
                              <span className="truncate">{row.currentFileName}</span>
                            </div>
                          </td>
                          <td className="truncate whitespace-nowrap border-b border-border px-3 py-2" title={row.detected}>{row.detected}</td>
                          <td className="max-w-[15rem] truncate border-b border-border px-3 py-2" title={row.episodeName}>{row.episodeName || "-"}</td>
                          <td className={["max-w-[21.25rem] truncate border-b border-border px-3 py-2", changedTextClass].join(" ")} title={row.newFileName}>{row.newFileName || "-"}</td>
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
        </div>
      </div>
      {isUndoOpen ? (
        <RenameUndoBatchModal
          batches={renameBatches.data?.batches ?? []}
          selectedBatch={selectedUndoBatch}
          selectedBatchId={selectedUndoBatchId}
          summaryLines={undoSummaryLines}
          preview={undoPreview.data}
          isLoadingBatches={renameBatches.isLoading}
          isLoadingPreview={undoPreview.isLoading}
          isUndoing={undoBatch.isPending}
          isClearing={clearBatches.isPending}
          isProceedPending={selectedUndoBatch !== null && pendingProceedBatchId === selectedUndoBatch.id}
          onSelectBatch={selectUndoBatch}
          onUndo={runUndoSelected}
          onClear={() => clearBatches.mutate()}
          onClose={() => setIsUndoOpen(false)}
        />
      ) : null}
      {isApplyConfirmOpen ? (
        <RenameApplyConfirmModal
          warnings={applyWarnings}
          selectedCount={selectedCount}
          isApplying={apply.isPending}
          onConfirm={executeApply}
          onClose={() => setIsApplyConfirmOpen(false)}
        />
      ) : null}
      {isSummaryExpanded ? (
        <PreviewSummaryModal
          title="Rename Preview Summary"
          emptyText="Build a preview to see planned filename changes."
          available={previewRows.length > 0}
          status={previewSummary}
          summary={previewSummary}
          metrics={[
            { label: "Files changing", value: previewRows.filter((row) => hasFilenameChange(row) && row.canApply).length, tone: "text-success" },
            { label: "Selected", value: previewRows.filter((row) => row.selected && row.canApply).length, tone: "text-accent" },
            { label: "No change", value: previewRows.filter((row) => !hasFilenameChange(row)).length, tone: "text-muted" },
            { label: "Needs attention", value: previewRows.filter((row) => !row.canApply).length, tone: "text-warning" }
          ]}
          sections={[
            {
              title: "Planned renames",
              emptyText: "No filename changes are planned.",
              rows: previewRows.filter((row) => hasFilenameChange(row) && row.canApply).map((row) => ({
                key: row.sourcePath,
                title: row.currentFileName,
                detail: `New filename: ${row.newFileName}`,
                meta: `${row.detected}${row.episodeName ? ` · ${row.episodeName}` : ""} · ${row.confidence}`
              }))
            },
            {
              title: "No change",
              emptyText: "Every previewed file has a new filename.",
              rows: previewRows.filter((row) => !hasFilenameChange(row)).map((row) => ({
                key: row.sourcePath,
                title: row.currentFileName,
                detail: row.status || "The generated filename matches the current filename."
              }))
            },
            {
              title: "Needs attention",
              emptyText: "No files require attention.",
              rows: previewRows.filter((row) => !row.canApply).map((row) => ({
                key: row.sourcePath,
                title: row.currentFileName,
                detail: row.status || "This rename cannot be applied.",
                meta: row.newFileName ? `Proposed filename: ${row.newFileName}` : undefined
              }))
            }
          ]}
          onClose={() => setIsSummaryExpanded(false)}
        />
      ) : null}
      {previewSelectionMenu ? (
        <div
          role="menu"
          aria-label="Rename selection options"
          className="fixed z-[60] min-w-48 overflow-hidden rounded-lg border border-border bg-card p-1 shadow-[0_0.75rem_2.5rem_rgba(0,0,0,0.45)]"
          style={{ left: Math.min(previewSelectionMenu.x, window.innerWidth - 210), top: Math.min(previewSelectionMenu.y, window.innerHeight - 180) }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button type="button" role="menuitem" onClick={() => { setHighlightedPreviewSelection(true); setPreviewSelectionMenu(null); }} className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text">Select highlighted rows</button>
          <button type="button" role="menuitem" onClick={() => { setHighlightedPreviewSelection(false); setPreviewSelectionMenu(null); }} className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text">Deselect highlighted rows</button>
          <div className="my-1 border-t border-border" />
          <button type="button" role="menuitem" onClick={() => { toggleAll(true); setPreviewSelectionMenu(null); }} className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text">Select all</button>
          <button type="button" role="menuitem" onClick={() => { toggleAll(false); setPreviewSelectionMenu(null); }} className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text">Deselect all</button>
        </div>
      ) : null}
    </div>
  );
}

function RenameApplyConfirmModal({ warnings, selectedCount, isApplying, onConfirm, onClose }: {
  warnings: string[];
  selectedCount: number;
  isApplying: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <section className="w-[min(40rem,calc(100vw-3rem))] rounded-lg border-2 border-window bg-card shadow-[0_1.875rem_5.625rem_rgba(0,0,0,0.55)]">
        <div className="flex h-10 items-center justify-between border-b border-border bg-window px-4">
          <div className="text-sm font-semibold text-muted">Confirm Rename Apply</div>
          <button type="button" onClick={onClose} className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text" title="Close">
            <X size={16} />
          </button>
        </div>
        <div className="p-5">
          <h2 className="text-lg font-semibold">Review skipped or conflicting rows</h2>
          <p className="mt-2 text-sm leading-5 text-muted">
            {selectedCount} row(s) are selected. Some rows may be skipped or need review before applying the rename batch.
          </p>
          <div className="mt-4 max-h-72 overflow-auto rounded-md border border-border bg-input p-3 font-mono text-xs leading-6 text-muted">
            {warnings.map((warning, index) => (
              <div key={`${index}-${warning}`} className="break-words">{warning}</div>
            ))}
          </div>
          <div className="mt-5 flex justify-end gap-2">
            <button type="button" onClick={onClose} className="h-9 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text">
              Cancel
            </button>
            <button type="button" onClick={onConfirm} disabled={isApplying} className="h-9 rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
              {isApplying ? "Applying..." : "Apply Anyway"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

type RenameUndoBatchModalProps = {
  batches: RenameBatchRecord[];
  selectedBatch: RenameBatchRecord | null;
  selectedBatchId: string;
  summaryLines: string[];
  preview?: RenameBatchUndoPreviewResponse;
  isLoadingBatches: boolean;
  isLoadingPreview: boolean;
  isUndoing: boolean;
  isClearing: boolean;
  isProceedPending: boolean;
  onSelectBatch: (id: string) => void;
  onUndo: () => void;
  onClear: () => void;
  onClose: () => void;
};

function RenameUndoBatchModal({
  batches,
  selectedBatch,
  selectedBatchId,
  summaryLines,
  preview,
  isLoadingBatches,
  isLoadingPreview,
  isUndoing,
  isClearing,
  isProceedPending,
  onSelectBatch,
  onUndo,
  onClear,
  onClose
}: RenameUndoBatchModalProps) {
  const canUndo = Boolean(selectedBatch && !selectedBatch.isUndone && !isUndoing && (!isProceedPending || (preview?.restorable ?? 0) > 0));
  const undoLabel = isUndoing ? "Undoing..." : isProceedPending ? "Proceed Anyway" : "Undo Selected";
  const visibleSummary = summaryLines.length > 0
    ? summaryLines
    : isLoadingPreview
      ? ["Loading restore plan..."]
      : ["Select a batch to review its restore plan."];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <section className="flex max-h-[min(47.5rem,calc(100vh-3rem))] w-[min(70rem,calc(100vw-3rem))] flex-col overflow-hidden rounded-lg border-2 border-window bg-card shadow-[0_1.875rem_5.625rem_rgba(0,0,0,0.55)]">
        <div className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-window px-4">
          <div className="text-sm font-semibold text-muted">Undo Rename Batch</div>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text"
            title="Close"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col gap-4 p-5">
          <div>
            <h2 className="text-xl font-semibold">Undo Rename Batch</h2>
            <p className="mt-1 text-sm leading-5 text-muted">
              Select a previous rename batch, review the planned reverse moves, then undo. Files are skipped if the current renamed file is missing, locked, or the original path already exists.
            </p>
          </div>

          <div className="grid min-h-0 flex-1 grid-cols-[19.375rem_minmax(0,1fr)_20.625rem] gap-3">
            <div className="flex min-h-0 flex-col rounded-lg border border-border-strong bg-panel p-3">
              <h3 className="text-sm font-semibold">Last 20 Batch Jobs</h3>
              <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-border bg-input p-1">
                {isLoadingBatches ? (
                  <div className="p-3 text-sm text-muted">Loading batches...</div>
                ) : batches.length === 0 ? (
                  <div className="p-3 text-sm text-muted">No rename batches have been recorded yet.</div>
                ) : batches.map((batch) => (
                  <button
                    key={batch.id}
                    type="button"
                    onClick={() => onSelectBatch(batch.id)}
                    className={[
                      "block w-full rounded-md px-3 py-2 text-left font-mono text-xs leading-5 transition",
                      batch.id === selectedBatchId ? "bg-selected text-text" : "text-muted hover:bg-card hover:text-text"
                    ].join(" ")}
                  >
                    {batch.displayName}
                  </button>
                ))}
              </div>
            </div>

            <div className="flex min-h-0 flex-col rounded-lg border border-border-strong bg-panel p-3">
              <h3 className="text-sm font-semibold">Files To Restore</h3>
              <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-border bg-input p-2">
                {!selectedBatch ? (
                  <div className="p-2 text-sm text-muted">Select a batch to review its files.</div>
                ) : selectedBatch.entries.map((entry, index) => (
                  <div key={`${entry.renamedPath}-${entry.originalPath}`} className="mb-2 rounded-md border border-border bg-card p-3 font-mono text-xs leading-5">
                    <div className="font-semibold text-muted">{String(index + 1).padStart(2, "0")}</div>
                    <div className="mt-2 grid grid-cols-[4.875rem_minmax(0,1fr)] gap-2">
                      <div className="text-subtle">Current</div>
                      <div className="break-words text-text">{entry.renamedFileName}</div>
                      <div className="text-subtle">Restore To</div>
                      <div className="break-words text-text">{entry.originalFileName}</div>
                    </div>
                  </div>
                ))}
              </div>
            </div>

            <div className="flex min-h-0 flex-col rounded-lg border border-border-strong bg-panel p-3">
              <h3 className="text-sm font-semibold">Summary</h3>
              <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-border bg-input p-3 font-mono text-xs leading-6 text-muted">
                {visibleSummary.map((line, index) => (
                  <div key={`${index}-${line}`} className="break-words whitespace-pre-wrap">
                    {line || "\u00A0"}
                  </div>
                ))}
              </div>
            </div>
          </div>

          <div className="grid shrink-0 grid-cols-[Auto_1fr_Auto] items-center gap-3">
            <button
              type="button"
              onClick={onClear}
              disabled={isClearing || batches.length === 0}
              className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              <Trash2 size={15} />
              Clear Batch List
            </button>

            <div />

            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={onUndo}
                disabled={!canUndo}
                className="inline-flex h-9 items-center gap-2 rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:bg-button disabled:text-disabled"
              >
                <RotateCcw size={15} />
                {undoLabel}
              </button>
              <button
                type="button"
                onClick={onClose}
                className="h-9 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                Close
              </button>
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}

function buildUndoSummaryLines(batch: RenameBatchRecord, preview: RenameBatchUndoPreviewResponse) {
  return [
    `Created: ${formatBatchDate(batch.createdAt)}`,
    `Provider: ${batch.provider || "N/A"}`,
    `Files: ${batch.totalFiles}`,
    `Status: ${batch.isUndone ? "Already undone" : preview.hasSkippedFiles ? `${preview.restorable} restorable, ${preview.skipped} skipped` : "Ready to undo"}`,
    "",
    ...preview.lines
  ];
}

function formatBatchDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  const pad = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
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

function buildRenameApplyWarnings(rows: RenamePreviewRow[]) {
  const selectedRows = rows.filter((row) => row.selected);
  const warnings: string[] = [];
  const targetCounts = new Map<string, number>();

  for (const row of selectedRows) {
    const targetKey = buildRenameTargetKey(row);
    if (!targetKey) continue;
    targetCounts.set(targetKey, (targetCounts.get(targetKey) ?? 0) + 1);
  }

  for (const row of selectedRows) {
    const label = row.currentFileName || row.sourcePath;
    const targetKey = buildRenameTargetKey(row);

    if (!row.canApply) {
      warnings.push(`${label}: ${row.status || "row is marked as not applicable"}`);
      continue;
    }

    if (!row.newFileName.trim()) {
      warnings.push(`${label}: no destination filename was generated`);
      continue;
    }

    if (!hasFilenameChange(row)) {
      warnings.push(`${label}: destination matches the current filename`);
    }

    if (targetKey && (targetCounts.get(targetKey) ?? 0) > 1) {
      warnings.push(`${label}: duplicate destination in selected rows (${row.newFileName})`);
    }
  }

  return warnings;
}

function buildRenameTargetKey(row: RenamePreviewRow) {
  const newFileName = row.newFileName.trim();
  if (!newFileName) return "";

  const slash = Math.max(row.sourcePath.lastIndexOf("/"), row.sourcePath.lastIndexOf("\\"));
  const directory = slash >= 0 ? row.sourcePath.slice(0, slash) : "";
  return `${directory}/${newFileName}`.toLowerCase();
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

function makeUnavailableRenameRow(file: MediaFileRow, status: string): RenamePreviewRow {
  return {
    selected: false,
    sourcePath: file.path,
    currentFileName: file.fileName,
    detected: "Movie",
    episodeName: "-",
    newFileName: "",
    confidence: "-",
    status,
    canApply: false
  };
}

function normalizeRenamePath(path: string) {
  return path.replace(/\\/g, "/").toLowerCase();
}

export function countCompletedRenameRows(rows: RenamePreviewRow[], files: MediaFileRow[]) {
  const currentPaths = new Set(files.map((file) => normalizeRenamePath(file.path)));
  return rows.filter((row) => !currentPaths.has(normalizeRenamePath(row.sourcePath))).length;
}

function buildOptionList(values: string[], current: string) {
  const seen = new Set<string>();
  return [current, ...values]
    .map((value) => value.trim())
    .filter((value) => {
      if (!value || seen.has(value.toLowerCase())) return false;
      seen.add(value.toLowerCase());
      return true;
    });
}
