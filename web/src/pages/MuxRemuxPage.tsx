import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, Wand2 } from "lucide-react";
import {
  buildMuxPreview,
  cancelOperationJob,
  getCurrentScanFiles,
  getWebSettings,
  MuxPreviewRequest,
  MuxPreviewResponse,
  startMuxApply
} from "../api";
import { PreviewSummaryModal } from "../components/PreviewSummaryModal";
import { SectionHeader } from "../components/SectionHeader";
import { SortableColumnHeader, type SortDirection } from "../components/SortableColumnHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useOperationJob } from "../state/OperationJobContext";

type FileSortKey = "file" | "reader" | "codec" | "resolution" | "audio" | "subtitles" | "status";

export function MuxRemuxPage() {
  const { files, selectedPaths, setSelectedPaths, toggleSelectedPath, templateFilePath, syncFromBackend, isWorkingView } = useMediaLibrary();
  const operation = useOperationJob();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const settings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const [activeTab, setActiveTab] = useState<"remux" | "subtitles">("remux");
  const [detailTab, setDetailTab] = useState<"tracks" | "attachments">("tracks");
  const [selectedDetailPath, setSelectedDetailPath] = useState("");
  const [removeAudio, setRemoveAudio] = useState(false);
  const [audioLanguages, setAudioLanguages] = useState("eng,jpn");
  const [removeSubtitles, setRemoveSubtitles] = useState(false);
  const [subtitleLanguages, setSubtitleLanguages] = useState("eng");
  const [removeTrackIds, setRemoveTrackIds] = useState(false);
  const [trackIds, setTrackIds] = useState("");
  const [preserveChapters, setPreserveChapters] = useState(true);
  const [preserveAttachments, setPreserveAttachments] = useState(true);
  const [muxExternal, setMuxExternal] = useState(false);
  const [externalLanguage, setExternalLanguage] = useState("eng");
  const [externalFormats, setExternalFormats] = useState("srt,ass,ssa,sub,idx");
  const [preserveSidecars, setPreserveSidecars] = useState(true);
  const [skipExistingSubtitle, setSkipExistingSubtitle] = useState(true);
  const [extractSubtitles, setExtractSubtitles] = useState(false);
  const [extractLanguages, setExtractLanguages] = useState("eng");
  const [extractOverwrite, setExtractOverwrite] = useState(false);
  const [convertMp4, setConvertMp4] = useState(false);
  const [deleteMp4AfterConvert, setDeleteMp4AfterConvert] = useState(false);
  const [previewResult, setPreviewResult] = useState<MuxPreviewResponse | null>(null);
  const [previewRemovedTracks, setPreviewRemovedTracks] = useState<Record<string, string[]>>({});
  const [statusText, setStatusText] = useState("Load scanned files from Dashboard, then build a preview.");
  const [settingsDefaultsApplied, setSettingsDefaultsApplied] = useState(false);
  const [isSummaryExpanded, setIsSummaryExpanded] = useState(false);
  const [selectionMenu, setSelectionMenu] = useState<{ x: number; y: number } | null>(null);
  const [highlightedPaths, setHighlightedPaths] = useState<string[]>([]);
  const [selectionAnchorPath, setSelectionAnchorPath] = useState("");
  const [fileSort, setFileSort] = useState<{ key: FileSortKey; direction: SortDirection }>({ key: "file", direction: "asc" });
  const initializedSelectionScope = useRef("");

  useEffect(() => {
    if (!currentScan.data) return;
    const scan = currentScan.data;
    const selectionScope = scan.updatedUtc ?? scan.files.map((file) => file.path).join("|");

    syncFromBackend(scan);
    if (scan.files.length === 0) {
      setPreviewResult(null);
      setSelectedDetailPath("");
      setStatusText("Load scanned files from Dashboard, then build a preview.");
    } else if (initializedSelectionScope.current !== selectionScope) {
      // MKV Operations starts every newly loaded scan as a full batch. Record
      // the scan identity first so later user deselection is preserved for the
      // remainder of this batch instead of being immediately undone.
      initializedSelectionScope.current = selectionScope;
      setSelectedPaths((isWorkingView ? files : scan.files).map((file) => file.path));
    }
  }, [currentScan.data, isWorkingView]);

  useEffect(() => {
    if (!settings.data || settingsDefaultsApplied) return;
    setAudioLanguages(settings.data.mkvMergeDefaultAudioLanguages || "eng,jpn");
    setSubtitleLanguages(settings.data.mkvMergeDefaultSubtitleLanguages || "eng");
    setSettingsDefaultsApplied(true);
  }, [settings.data, settingsDefaultsApplied]);

  useEffect(() => {
    setSelectedDetailPath((current) => current && files.some((file) => file.path === current) ? current : files[0]?.path ?? "");
  }, [files]);

  useEffect(() => {
    if (!selectionMenu) return;
    const close = () => setSelectionMenu(null);
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [selectionMenu]);

  const mkvFiles = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mkv"), [files]);
  const mp4Files = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mp4"), [files]);
  const selectedMkvPaths = useMemo(
    () => selectedPaths.filter((path) => files.some((file) => file.path === path && file.extension.toLowerCase() === ".mkv")),
    [files, selectedPaths]
  );
  const selectedMp4Paths = useMemo(
    () => selectedPaths.filter((path) => files.some((file) => file.path === path && file.extension.toLowerCase() === ".mp4")),
    [files, selectedPaths]
  );
  const selectedNonMkvCount = selectedPaths.length - selectedMkvPaths.length;
  const selectedDetailFile = useMemo(
    () => files.find((file) => file.path === selectedDetailPath) ?? files[0] ?? null,
    [files, selectedDetailPath]
  );
  const templateFile = useMemo(
    () => files.find((file) => normalizePath(file.path) === normalizePath(templateFilePath)) ?? files[0] ?? null,
    [files, templateFilePath]
  );
  const displayedFiles = useMemo(() => sortOperationFiles(files, fileSort.key, fileSort.direction, templateFile), [files, fileSort, templateFile]);
  const selectedCount = selectedPaths.length;

  const preview = useMutation({
    mutationFn: buildMuxPreview,
    onSuccess: (response, request) => {
      setPreviewResult(response);
      setPreviewRemovedTracks(buildRemovedTrackDetails(request));
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.")
  });

  const apply = useMutation({
    mutationFn: startMuxApply,
    onSuccess: (job) => {
      operation.trackJob(job.id, "MKV Operations");
      setStatusText(`Applying ${job.total} MKV operation(s)...`);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Apply failed.")
  });

  const cancelApply = useMutation({ mutationFn: cancelOperationJob });
  const runningJob = operation.activeOperation?.label === "MKV Operations" ? operation.job : undefined;
  const isApplying = apply.isPending || (operation.activeOperation?.label === "MKV Operations" && operation.isRunning);

  useEffect(() => {
    if (!runningJob) return;

    if (runningJob.status === "Running" || runningJob.status === "Queued" || runningJob.status === "WaitingForResources" || runningJob.status === "Canceling") {
      const fileInfo = runningJob.currentFile ? ` | ${runningJob.currentFile} ${runningJob.currentFilePercent}%` : "";
      setStatusText(`Applying ${runningJob.completed + runningJob.failed + runningJob.skipped}/${runningJob.total}${fileInfo}`);
      return;
    }

    if (runningJob.muxResult) {
      setPreviewResult(runningJob.muxResult);
      setStatusText(runningJob.muxResult.status);
    } else if (runningJob.status === "Failed") {
      setStatusText(runningJob.error || "Apply failed.");
    }

    currentScan.refetch().then((result) => {
      if (result.data) syncFromBackend(result.data);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runningJob?.status, runningJob?.completed, runningJob?.currentFile, runningJob?.currentFilePercent]);

  function buildRequest(): MuxPreviewRequest {
    return {
      files,
      selectedPaths: convertMp4 ? [...selectedMkvPaths, ...selectedMp4Paths] : selectedMkvPaths,
      removeUnwantedAudioLanguages: removeAudio,
      keepAudioLanguages: audioLanguages,
      removeUnwantedSubtitleLanguages: removeSubtitles,
      keepSubtitleLanguages: subtitleLanguages,
      removeUnwantedTrackIds: removeTrackIds,
      removeTrackIdsText: trackIds,
      preserveChapters,
      preserveAttachments,
      muxMatchingExternalSubtitles: muxExternal,
      externalSubtitleLanguage: externalLanguage,
      externalSubtitleFormats: externalFormats,
      preserveExternalSubtitleFiles: preserveSidecars,
      skipMuxIfSubtitleAlreadyExists: skipExistingSubtitle,
      extractSubtitles,
      extractSubtitleLanguages: extractLanguages,
      extractOverwriteExistingFiles: extractOverwrite,
      convertMp4ToMkv: convertMp4,
      deleteMp4AfterConvert
    };
  }

  async function refreshFiles() {
    const result = await currentScan.refetch();
    if (result.data?.files.length) {
      syncFromBackend(result.data);
      setStatusText(`Loaded ${result.data.files.length} scanned file(s).`);
    } else {
      setStatusText("No Dashboard scan is available yet.");
    }
  }

  function togglePath(path: string) {
    toggleSelectedPath(path);
  }

  function highlightFile(path: string, index: number, modifiers: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) {
    setSelectedDetailPath(path);
    if (modifiers.shiftKey && selectionAnchorPath) {
      const anchorIndex = displayedFiles.findIndex((file) => normalizePath(file.path) === normalizePath(selectionAnchorPath));
      if (anchorIndex >= 0) {
        const start = Math.min(anchorIndex, index);
        const end = Math.max(anchorIndex, index);
        setHighlightedPaths(displayedFiles.slice(start, end + 1).map((file) => file.path));
        return;
      }
    }
    if (modifiers.ctrlKey || modifiers.metaKey) {
      setHighlightedPaths((current) => current.some((item) => normalizePath(item) === normalizePath(path))
        ? current.filter((item) => normalizePath(item) !== normalizePath(path))
        : [...current, path]);
      setSelectionAnchorPath(path);
      return;
    }
    setHighlightedPaths([path]);
    setSelectionAnchorPath(path);
  }

  function setHighlightedSelection(checked: boolean) {
    if (highlightedPaths.length === 0) return;
    const targets = new Set(highlightedPaths.map(normalizePath));
    const next = checked
      ? [...selectedPaths, ...highlightedPaths.filter((path) => !selectedPaths.some((selected) => normalizePath(selected) === normalizePath(path)))]
      : selectedPaths.filter((path) => !targets.has(normalizePath(path)));
    setSelectedPaths(next);
  }

  function toggleHighlightedSelection() {
    if (highlightedPaths.length === 0) return;
    const allSelected = highlightedPaths.every((path) => selectedPaths.some((selected) => normalizePath(selected) === normalizePath(path)));
    setHighlightedSelection(!allSelected);
  }

  function runPreview() {
    const hasConvertibleMp4s = convertMp4 && selectedMp4Paths.length > 0;
    if (selectedMkvPaths.length === 0 && !hasConvertibleMp4s) {
      setStatusText("MKV Operations requires at least one selected MKV file (or MP4 with conversion enabled).");
      return;
    }

    preview.mutate(buildRequest());
  }

  function runApply() {
    if (!previewResult?.actions.length) {
      setStatusText("Build a preview with planned actions before applying.");
      return;
    }

    apply.mutate({
      ...buildRequest(),
      planId: previewResult?.planId,
      planFingerprint: previewResult?.planFingerprint,
      idempotencyKey: previewResult?.idempotencyKey ?? crypto.randomUUID()
    });
  }

  function cancelRunningApply() {
    if (!operation.activeOperation || operation.activeOperation.label !== "MKV Operations") return;
    cancelApply.mutate(operation.activeOperation.id);
    setStatusText("Canceling MKV operation job...");
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="MKV Operations" description="Remove tracks, mux matching subtitle sidecars, extract subtitles, or convert containers with MKVToolNix." />
      <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[18.75rem_minmax(0,1fr)] gap-3">
        <section className="min-h-0 overflow-x-hidden overflow-y-auto rounded-lg border border-border bg-card p-3 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
          <div className="flex justify-end">
            <button onClick={refreshFiles} className="h-9 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text">Refresh</button>
          </div>

          <div className="mt-2 flex gap-5 text-sm">
            {(["remux", "subtitles"] as const).map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                className={["pb-1 font-semibold", activeTab === tab ? "border-b border-accent text-text" : "text-muted hover:text-text"].join(" ")}
              >
                {tab === "remux" ? "Tracks" : "Subtitles"}
              </button>
            ))}
          </div>

          {activeTab === "remux" ? (
            <div className="mt-4 space-y-3">
              {mp4Files.length > 0 ? (
                <div className="space-y-3 rounded-md border border-accent bg-panel p-3">
                  <h2 className="text-sm font-semibold text-accent">MP4 Conversion</h2>
                  <p className="text-xs text-muted">{mp4Files.length === 1 ? "1 MP4 file detected" : `${mp4Files.length} MP4 files detected`}</p>
                  <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={convertMp4} onChange={(event) => setConvertMp4(event.target.checked)} /> Convert selected MP4 files to MKV</label>
                  <label className={["flex items-center gap-2 pl-5 text-sm", convertMp4 ? "" : "text-disabled"].join(" ")}>
                    <input type="checkbox" checked={deleteMp4AfterConvert} disabled={!convertMp4} onChange={(event) => setDeleteMp4AfterConvert(event.target.checked)} /> Delete original MP4 after success
                  </label>
                  <p className="text-xs leading-5 text-muted">Lossless container copy via mkvmerge - no re-encoding. The new .mkv is created next to the source file; files whose .mkv already exists are skipped.</p>
                </div>
              ) : null}
              <h2 className="text-sm font-semibold">Track Removal</h2>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={removeAudio} onChange={(event) => setRemoveAudio(event.target.checked)} /> Remove unwanted audio languages</label>
              <Field label="Audio languages to keep" value={audioLanguages} onChange={setAudioLanguages} placeholder="eng,jpn" />
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={removeSubtitles} onChange={(event) => setRemoveSubtitles(event.target.checked)} /> Remove unwanted subtitle languages</label>
              <Field label="Subtitle languages to keep" value={subtitleLanguages} onChange={setSubtitleLanguages} placeholder="eng" />
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={removeTrackIds} onChange={(event) => setRemoveTrackIds(event.target.checked)} /> Remove unwanted track IDs</label>
              <Field label="Track IDs to remove" value={trackIds} onChange={setTrackIds} placeholder="1 or 1, 3" />
              <h2 className="pt-1 text-sm font-semibold">Preservation Options</h2>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={preserveChapters} onChange={(event) => setPreserveChapters(event.target.checked)} /> Preserve chapters</label>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={preserveAttachments} onChange={(event) => setPreserveAttachments(event.target.checked)} /> Preserve attachments/fonts</label>
              <p className="text-xs leading-5 text-muted">Originals are always replaced through a safe temp file with automatic backup.</p>
            </div>
          ) : null}

          {activeTab === "subtitles" ? (
            <div className="mt-4 space-y-3">
              <h2 className="text-sm font-semibold">Subtitle Mux</h2>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={muxExternal} onChange={(event) => setMuxExternal(event.target.checked)} /> Mux matching external subtitles</label>
              <div className="text-sm text-muted">File Format: <span className="text-accent">file_name.language.tag.ext</span></div>
              <Field label="Fallback language" value={externalLanguage} onChange={setExternalLanguage} placeholder="eng" />
              <Field label="Subtitle formats" value={externalFormats} onChange={setExternalFormats} placeholder="srt,ass,ssa,sub,idx" />
              <h2 className="pt-1 text-sm font-semibold">Mux Options</h2>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={preserveSidecars} onChange={(event) => setPreserveSidecars(event.target.checked)} /> Preserve external subtitle files</label>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={skipExistingSubtitle} onChange={(event) => setSkipExistingSubtitle(event.target.checked)} /> Skip if matching subtitle already exists</label>
              <p className="text-xs leading-5 text-muted">Example: Episode 01.eng.Dialogue.ass. See Settings for detailed usage.</p>
              <h2 className="pt-1 text-sm font-semibold">Subtitle Extract</h2>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={extractSubtitles} onChange={(event) => setExtractSubtitles(event.target.checked)} /> Extract subtitles</label>
              <Field label="Subtitle languages" value={extractLanguages} onChange={setExtractLanguages} placeholder="eng or all" />
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={extractOverwrite} onChange={(event) => setExtractOverwrite(event.target.checked)} /> Overwrite existing extracted files</label>
            </div>
          ) : null}

          <h2 className="mt-4 text-sm font-semibold">Execution</h2>
          <div className="mt-2 flex gap-2">
            <button onClick={runPreview} disabled={preview.isPending || (selectedMkvPaths.length === 0 && !(convertMp4 && selectedMp4Paths.length > 0))} className="inline-flex h-9 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              {preview.isPending ? <RefreshCw size={15} className="animate-spin" /> : <Wand2 size={15} />}
              Preview
            </button>
            {isApplying ? (
              <button onClick={cancelRunningApply} disabled={cancelApply.isPending} className="h-9 flex-1 rounded-md border border-warning bg-button px-3 text-sm font-semibold text-warning hover:bg-button-hover disabled:text-disabled">
                Cancel
              </button>
            ) : (
              <button onClick={runApply} disabled={selectedMkvPaths.length === 0 || !previewResult?.actions.length} className="h-9 flex-1 rounded-md bg-accent px-3 text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
                Apply
              </button>
            )}
          </div>
          <div className="mt-3 line-clamp-2 text-sm text-success">{statusText}</div>
          <div className="mt-1 text-xs text-muted">
            {selectedCount} selected | {selectedMkvPaths.length} selected MKV | {mkvFiles.length} MKV available
          </div>
          {selectedNonMkvCount > 0 ? (
            <div className="mt-1 text-xs text-warning">{selectedNonMkvCount} selected non-MKV file(s) are visible for context and excluded from MKV operations.</div>
          ) : null}
        </section>

        <div className="grid min-h-0 min-w-0 grid-rows-[1.3fr_1fr] gap-3">
          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
            <div className="flex shrink-0 items-center justify-between gap-3">
              <h2 className="text-base font-semibold">File Info</h2>
              <button
                type="button"
                onClick={() => setIsSummaryExpanded(true)}
                className="inline-flex h-9 min-w-32 items-center justify-center whitespace-nowrap rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                Preview Summary
              </button>
            </div>
            <div
              className="mt-3 min-h-0 flex-1 overflow-auto"
              tabIndex={0}
              onKeyDown={(event) => {
                if (event.key !== " " || highlightedPaths.length === 0) return;
                event.preventDefault();
                toggleHighlightedSelection();
              }}
              aria-label="MKV Operations file selection"
            >
              <table className="w-full min-w-[68.75rem] border-collapse text-left text-sm">
                <thead className="sticky top-0 bg-card text-xs text-text">
                  <tr>
                    {(["file", "reader", "codec", "resolution", "audio", "subtitles", "status"] as FileSortKey[]).map((key) => (
                      <SortableColumnHeader key={key} active={fileSort.key === key} direction={fileSort.direction} label={{ file: "File", reader: "Reader", codec: "Codec", resolution: "Resolution", audio: "Audio", subtitles: "Subtitles", status: "Status" }[key]} onSort={() => setFileSort((current) => ({ key, direction: current.key === key && current.direction === "asc" ? "desc" : "asc" }))} />
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {displayedFiles.map((file, index) => {
                    const fileStatus = operationFileStatus(file, templateFile);
                    return (
                    <tr
                      key={file.path}
                      onClick={(event) => highlightFile(file.path, index, event)}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        if (!highlightedPaths.some((path) => normalizePath(path) === normalizePath(file.path))) {
                          setHighlightedPaths([file.path]);
                          setSelectionAnchorPath(file.path);
                        }
                        setSelectedDetailPath(file.path);
                        setSelectionMenu({ x: event.clientX, y: event.clientY });
                      }}
                      className={[highlightedPaths.some((path) => normalizePath(path) === normalizePath(file.path)) ? "bg-selected" : "bg-card", "cursor-pointer hover:bg-selected"].join(" ")}
                    >
                      <td className="border-b border-border px-3 py-2">
                        <div className="flex min-w-0 items-center gap-3">
                          <input type="checkbox" checked={selectedPaths.includes(file.path)} onClick={(event) => event.stopPropagation()} onChange={() => togglePath(file.path)} />
                          <span className="truncate" title={file.path}>{file.fileName}</span>
                        </div>
                      </td>
                      <td className="border-b border-border px-3 py-2">{file.reader}</td>
                      <td className="border-b border-border px-3 py-2">{file.codec || "Unknown"}</td>
                      <td className="border-b border-border px-3 py-2">{file.resolution || "Unknown"}</td>
                      <td className="max-w-[15.625rem] truncate border-b border-border px-3 py-2" title={file.audioSummary}>{file.audioSummary || "None"}</td>
                      <td className="max-w-[15.625rem] truncate border-b border-border px-3 py-2" title={file.subtitleSummary}>{file.subtitleSummary || "None"}</td>
                      <td className={["truncate border-b border-border px-3 py-2", fileStatus === "Warning" ? "text-warning" : fileStatus === "Template" ? "text-template" : "text-success"].join(" ")}>{fileStatus}</td>
                    </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </section>

          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
            <div className="flex shrink-0 gap-6 text-sm">
              <button onClick={() => setDetailTab("tracks")} className={detailTab === "tracks" ? "border-b border-accent pb-1 font-semibold text-text" : "pb-1 font-semibold text-muted"}>File Details: Tracks</button>
              <button onClick={() => setDetailTab("attachments")} className={detailTab === "attachments" ? "border-b border-accent pb-1 font-semibold text-text" : "pb-1 font-semibold text-muted"}>File Details: Attachments</button>
            </div>
            <div className="mt-3 min-h-0 flex-1 overflow-auto">
              {detailTab === "tracks" ? (
                <table className="w-full min-w-[45rem] table-fixed border-collapse text-left text-sm">
                  <thead className="sticky top-0 bg-card text-xs text-text">
                    <tr>
                      <th className="w-12 border-b border-border px-3 py-2">#</th>
                      <th className="w-24 border-b border-border px-3 py-2">Type</th>
                      <th className="w-20 border-b border-border px-3 py-2">Lang</th>
                      <th className="w-44 border-b border-border px-3 py-2">Codec</th>
                      <th className="border-b border-border px-3 py-2">Name</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(selectedDetailFile?.tracks ?? []).map((track) => (
                      <tr key={`${track.id}-${track.trackNumber}`} className="bg-card hover:bg-selected">
                        <td className="border-b border-border px-3 py-2">{track.id}</td>
                        <td className="border-b border-border px-3 py-2">{track.type}</td>
                        <td className="border-b border-border px-3 py-2">{track.language || "und"}</td>
                        <td className="truncate border-b border-border px-3 py-2" title={track.codec}>{track.codec || "Unknown"}</td>
                        <td className="truncate border-b border-border px-3 py-2" title={track.name}>{track.name || "-"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : selectedDetailFile?.attachments?.length ? (
                <table className="w-full min-w-[47.5rem] table-fixed border-collapse text-left text-sm">
                  <thead className="sticky top-0 bg-card text-xs text-text">
                    <tr>
                      <th className="w-14 border-b border-border px-3 py-2">#</th>
                      <th className="w-[16.25rem] border-b border-border px-3 py-2">File</th>
                      <th className="w-52 border-b border-border px-3 py-2">Content Type</th>
                      <th className="w-28 border-b border-border px-3 py-2">Size</th>
                      <th className="border-b border-border px-3 py-2">Description</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(selectedDetailFile.attachments ?? []).map((attachment) => (
                      <tr key={`${attachment.id}-${attachment.fileName}`} className="bg-card hover:bg-selected">
                        <td className="border-b border-border px-3 py-2">{attachment.id}</td>
                        <td className="truncate border-b border-border px-3 py-2" title={attachment.fileName}>{attachment.fileName || "-"}</td>
                        <td className="truncate border-b border-border px-3 py-2" title={attachment.contentType}>{attachment.contentType || "-"}</td>
                        <td className="border-b border-border px-3 py-2">{formatBytes(attachment.sizeBytes)}</td>
                        <td className="truncate border-b border-border px-3 py-2" title={attachment.description}>{attachment.description || "-"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : (
                <div className="flex h-full min-h-[7.5rem] items-center justify-center text-sm text-subtle">
                  No attachments or fonts were detected for the selected file.
                </div>
              )}
            </div>
          </section>

        </div>
      </div>
      {isSummaryExpanded ? (
        <PreviewSummaryModal
          title="MKV Operations Preview Summary"
          emptyText="Build a preview to see planned MKV operations."
          available={previewResult !== null}
          status={previewResult?.status ?? ""}
          summary={previewResult?.summary ?? ""}
          metrics={[
            { label: "Files changing", value: new Set(previewResult?.actions.map((action) => action.filePath) ?? []).size, tone: "text-success" },
            { label: "Planned actions", value: previewResult?.actions.length ?? 0, tone: "text-accent" },
            { label: "No change", value: previewResult?.noChangeFiles.length ?? 0, tone: "text-muted" },
            { label: "Tools involved", value: new Set(previewResult?.actions.map((action) => action.toolName) ?? []).size, tone: "text-text" }
          ]}
          sections={[
            {
              title: "Planned operations",
              emptyText: "No MKV operations are needed.",
              rows: previewResult?.actions.map((action) => ({
                key: `${action.filePath}-${action.index}`,
                title: action.fileName,
                detail: [
                  action.description,
                  ...(previewRemovedTracks[normalizePath(action.filePath)] ?? [])
                ].join("\n"),
                meta: `${action.operation} · ${action.toolName}`
              })) ?? []
            },
            {
              title: "No change",
              emptyText: "Every selected file requires an operation.",
              rows: previewResult?.noChangeFiles.map((filePath) => ({
                key: filePath,
                title: filePath.split(/[\\/]/).pop() || filePath,
                detail: "No MKV operations are required for this file."
              })) ?? []
            }
          ]}
          onClose={() => setIsSummaryExpanded(false)}
        />
      ) : null}
      {selectionMenu ? (
        <div
          role="menu"
          aria-label="File selection options"
          className="fixed z-[60] min-w-40 overflow-hidden rounded-lg border border-border bg-card p-1 shadow-[0_0.75rem_2.5rem_rgba(0,0,0,0.45)]"
          style={{
            left: Math.min(selectionMenu.x, window.innerWidth - 180),
            top: Math.min(selectionMenu.y, window.innerHeight - 180)
          }}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            role="menuitem"
            disabled={highlightedPaths.length === 0}
            onClick={() => {
              setHighlightedSelection(true);
              setSelectionMenu(null);
            }}
            className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text disabled:text-disabled"
          >
            Select highlighted rows
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={highlightedPaths.length === 0}
            onClick={() => {
              setHighlightedSelection(false);
              setSelectionMenu(null);
            }}
            className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text disabled:text-disabled"
          >
            Deselect highlighted rows
          </button>
          <div className="my-1 border-t border-border" />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setSelectedPaths(files.map((file) => file.path));
              setSelectionMenu(null);
            }}
            className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text"
          >
            Select all
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setSelectedPaths([]);
              setSelectionMenu(null);
            }}
            className="flex w-full rounded-md px-3 py-2 text-left text-sm text-muted hover:bg-selected hover:text-text"
          >
            Deselect all
          </button>
        </div>
      ) : null}
    </div>
  );
}

function normalizePath(path: string) {
  return path.replace(/\\/g, "/").toLowerCase();
}

function sortOperationFiles(files: ReturnType<typeof useMediaLibrary>["files"], key: FileSortKey, direction: SortDirection, templateFile: ReturnType<typeof useMediaLibrary>["files"][number] | null) {
  const multiplier = direction === "asc" ? 1 : -1;
  return files.map((file, index) => ({ file, index })).sort((left, right) => {
    const value = (file: typeof left.file) => {
      if (key === "file") return file.fileName;
      if (key === "reader") return file.reader;
      if (key === "codec") return file.codec || "Unknown";
      if (key === "resolution") return file.resolution || "Unknown";
      if (key === "audio") return file.audioSummary || "None";
      if (key === "subtitles") return file.subtitleSummary || "None";
      return operationFileStatus(file, templateFile);
    };
    const compared = value(left.file).localeCompare(value(right.file), undefined, { numeric: true, sensitivity: "base" });
    return compared === 0 ? left.index - right.index : compared * multiplier;
  }).map(({ file }) => file);
}

function operationFileStatus(file: ReturnType<typeof useMediaLibrary>["files"][number], templateFile: ReturnType<typeof useMediaLibrary>["files"][number] | null) {
  if (!templateFile) return file.status;
  if (normalizePath(file.path) === normalizePath(templateFile.path)) return "Template";
  const values = ["codec", "resolution", "bitDepth", "audioSummary", "subtitleSummary"] as const;
  const valueMismatch = values.some((key) => normalizeValue(file[key]) !== normalizeValue(templateFile[key]));
  const trackMismatch = normalizeOperationTracks(file) !== normalizeOperationTracks(templateFile);
  return valueMismatch || trackMismatch ? "Warning" : file.status;
}

function normalizeOperationTracks(file: ReturnType<typeof useMediaLibrary>["files"][number]) {
  return file.tracks.map((track) => [
    track.id,
    track.trackNumber,
    track.type,
    track.codec,
    track.language,
    track.type.toLowerCase() === "video" ? "" : track.name,
    track.default,
    track.forced
  ].map(String).map(normalizeValue).join("|")).join(";");
}

function normalizeValue(value: string) {
  return value.trim().toLowerCase();
}

type RemovalPreviewInput = Pick<MuxPreviewRequest,
  "files"
  | "selectedPaths"
  | "removeUnwantedAudioLanguages"
  | "keepAudioLanguages"
  | "removeUnwantedSubtitleLanguages"
  | "keepSubtitleLanguages"
  | "removeUnwantedTrackIds"
  | "removeTrackIdsText"
>;

export function buildRemovedTrackDetails(request: RemovalPreviewInput): Record<string, string[]> {
  const selected = new Set(request.selectedPaths.map(normalizePath));
  const explicitIds = request.removeUnwantedTrackIds
    ? new Set((request.removeTrackIdsText.match(/\d+/g) ?? []).map(Number))
    : new Set<number>();
  const audioLanguages = parseLanguageSet(request.keepAudioLanguages);
  const subtitleLanguages = parseLanguageSet(request.keepSubtitleLanguages);
  const details: Record<string, string[]> = {};

  for (const file of request.files) {
    const key = normalizePath(file.path);
    if (!selected.has(key)) continue;
    const removed = file.tracks.filter((track) => {
      if (explicitIds.has(track.id)) return true;
      const type = track.type.toLowerCase();
      const language = (track.language || "und").toLowerCase();
      if (type === "audio" && request.removeUnwantedAudioLanguages) return !audioLanguages.has(language);
      if ((type === "subtitle" || type === "subtitles") && request.removeUnwantedSubtitleLanguages) return !subtitleLanguages.has(language);
      return false;
    });
    if (removed.length === 0) continue;
    details[key] = [
      `Removed tracks (${removed.length}):`,
      ...removed.map((track) => {
        const parts = [
          `ID ${track.id}`,
          track.type || "Track",
          track.language || "und",
          track.codec || "Unknown codec",
          track.name || "No name"
        ];
        if (track.default) parts.push("default");
        if (track.forced) parts.push("forced");
        return `• ${parts.join(" · ")}`;
      })
    ];
  }
  return details;
}

function parseLanguageSet(value: string) {
  return new Set(value.split(/[\s,;]+/).map((part) => part.trim().toLowerCase()).filter(Boolean));
}

function Field({ label, value, onChange, placeholder }: { label: string; value: string; onChange: (value: string) => void; placeholder: string }) {
  return (
    <label className="block">
      <span className="text-xs font-semibold text-muted">{label}</span>
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
      />
    </label>
  );
}

function formatBytes(value: number | null | undefined) {
  if (!value || value <= 0) return "-";

  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }

  return `${size >= 10 || unitIndex === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[unitIndex]}`;
}
