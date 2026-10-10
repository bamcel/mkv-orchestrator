import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ArrowDown, ArrowUp } from "lucide-react";

import { buildMuxPreview, getCurrentScanFiles, startMuxApply, type MuxPreviewRequest, type MuxPreviewResponse } from "../api";
import { FileName } from "../components/FileName";
import { MobileOptions } from "../components/MobileOptions";
import { PanelDivider } from "../components/PanelDivider";
import { PreviewSummaryModal } from "../components/PreviewSummaryModal";
import { SectionHeader } from "../components/SectionHeader";
import { SortableColumnHeader, type SortDirection } from "../components/SortableColumnHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useOperationJob } from "../state/OperationJobContext";

type FileSortKey = "file" | "reader" | "codec" | "audio" | "subtitles" | "status";

export function ReorderTracksPage() {
  const { files, selectedPaths, setSelectedPaths, toggleSelectedPath, templateFilePath, syncFromBackend, isWorkingView } = useMediaLibrary();
  const operation = useOperationJob();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const mkvFiles = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mkv"), [files]);
  const initializedSelectionScope = useRef("");
  const panelRef = useRef<HTMLDivElement>(null);
  const [templatePath, setTemplatePath] = useState("");
  const [order, setOrder] = useState<number[]>([]);
  const [selectedDetailPath, setSelectedDetailPath] = useState("");
  const [filePanelPercent, setFilePanelPercent] = useState(56);
  const [fileSort, setFileSort] = useState<{ key: FileSortKey; direction: SortDirection }>({ key: "file", direction: "asc" });
  const [previewResult, setPreviewResult] = useState<MuxPreviewResponse | null>(null);
  const [isSummaryExpanded, setIsSummaryExpanded] = useState(false);
  const [statusText, setStatusText] = useState("Select batch files, arrange the template tracks, then preview the changes.");

  useEffect(() => {
    if (!currentScan.data) return;
    const scan = currentScan.data;
    const scope = scan.updatedUtc ?? scan.files.map((file) => file.path).join("|");
    syncFromBackend(scan);
    if (scan.files.length === 0) {
      setPreviewResult(null);
      setSelectedDetailPath("");
    } else if (initializedSelectionScope.current !== scope) {
      initializedSelectionScope.current = scope;
      setSelectedPaths((isWorkingView ? files : scan.files).filter((file) => file.extension.toLowerCase() === ".mkv").map((file) => file.path));
    }
  }, [currentScan.data, isWorkingView]);

  useEffect(() => {
    const preferred = mkvFiles.find((file) => normalizePath(file.path) === normalizePath(templateFilePath))?.path;
    setTemplatePath((current) => mkvFiles.some((file) => normalizePath(file.path) === normalizePath(current)) ? current : preferred ?? mkvFiles[0]?.path ?? "");
    setSelectedDetailPath((current) => mkvFiles.some((file) => normalizePath(file.path) === normalizePath(current)) ? current : preferred ?? mkvFiles[0]?.path ?? "");
  }, [mkvFiles, templateFilePath]);

  const template = useMemo(() => mkvFiles.find((file) => normalizePath(file.path) === normalizePath(templatePath)) ?? null, [mkvFiles, templatePath]);
  const selectedMkvPaths = useMemo(() => selectedPaths.filter((path) => mkvFiles.some((file) => normalizePath(file.path) === normalizePath(path))), [mkvFiles, selectedPaths]);
  const displayedFiles = useMemo(() => sortFiles(mkvFiles, fileSort, templatePath), [mkvFiles, fileSort, templatePath]);

  useEffect(() => {
    setOrder(template?.tracks.slice().sort((left, right) => left.trackNumber - right.trackNumber).map((track) => track.id) ?? []);
    setPreviewResult(null);
  }, [template?.path]);

  const orderedTracks = order
    .map((id) => template?.tracks.find((track) => track.id === id))
    .filter(Boolean) as NonNullable<typeof template>["tracks"];
  const preview = useMutation({ mutationFn: buildMuxPreview, onSuccess: (result) => { setPreviewResult(result); setStatusText(result.status); }, onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.") });
  const apply = useMutation({ mutationFn: startMuxApply, onSuccess: (job) => { operation.trackJob(job.id, "Reorder Tracks"); setStatusText(`Reordering tracks in ${job.total} file(s)...`); }, onError: (error) => setStatusText(error instanceof Error ? error.message : "Apply failed.") });

  function chooseTemplate(path: string) {
    setTemplatePath(path);
    setSelectedDetailPath(path);
    if (!selectedPaths.some((selected) => normalizePath(selected) === normalizePath(path))) setSelectedPaths([...selectedPaths, path]);
  }

  function move(index: number, delta: number) {
    const target = index + delta;
    if (target < 0 || target >= order.length) return;
    setOrder((current) => { const next = [...current]; [next[index], next[target]] = [next[target], next[index]]; return next; });
    setPreviewResult(null);
  }

  function buildRequest(plan?: MuxPreviewResponse): MuxPreviewRequest {
    return {
      files, selectedPaths: selectedMkvPaths,
      removeUnwantedAudioLanguages: false, keepAudioLanguages: "", removeUnwantedSubtitleLanguages: false, keepSubtitleLanguages: "", removeUnwantedTrackIds: false, removeTrackIdsText: "",
      preserveChapters: true, preserveAttachments: true, preserveOriginal: false, remuxOutputSuffix: ".remuxed",
      reorderTracks: true, reorderTemplatePath: templatePath, reorderTemplateTrackIds: order,
      muxMatchingExternalSubtitles: false, manualSubtitleSelections: [], externalSubtitleLanguage: "und", externalSubtitleTrackName: null, externalSubtitleFormats: "", preserveExternalSubtitleFiles: true, skipMuxIfSubtitleAlreadyExists: false,
      extractSubtitles: false, extractSubtitleLanguages: "", extractOverwriteExistingFiles: false, convertMp4ToMkv: false, deleteMp4AfterConvert: false,
      planId: plan?.planId ?? null, planFingerprint: plan?.planFingerprint ?? null, idempotencyKey: plan?.idempotencyKey ?? null
    };
  }

  const canRun = Boolean(template && selectedMkvPaths.some((path) => normalizePath(path) === normalizePath(template.path)));
  function runPreview() { if (!canRun) { setStatusText("Select a template file and include it in the batch."); return; } preview.mutate(buildRequest()); }
  async function runApply() {
    if (!canRun) return;
    try {
      const plan = await preview.mutateAsync(buildRequest());
      if (!plan.actions.length) { setStatusText("No track-order changes are needed for the selected files."); return; }
      await apply.mutateAsync({ ...buildRequest(plan), idempotencyKey: plan.idempotencyKey ?? crypto.randomUUID() });
    } catch { /* Mutation handlers display the error. */ }
  }
  const busy = preview.isPending || apply.isPending || (operation.activeOperation?.label === "Reorder Tracks" && operation.isRunning);

  return (
    <div className="workspace-page flex h-full min-h-0 flex-col">
      <SectionHeader title="Reorder Tracks" description="Use one template to reorder tracks consistently across all selected MKV files." />
      <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[minmax(14rem,18.75rem)_minmax(0,1fr)] gap-3">
        <section className="operation-sidebar flex min-h-0 flex-col overflow-hidden rounded-lg border border-border bg-card p-3 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
          <MobileOptions><div className="space-y-3">
            <h2 className="text-base font-semibold">Reorder Options</h2>
            <label className="block text-sm font-semibold" htmlFor="reorder-template">Template file</label>
            <select id="reorder-template" className="h-9 w-full rounded-md border border-border bg-input px-2 text-sm" value={templatePath} onChange={(event) => chooseTemplate(event.target.value)}>{mkvFiles.map((file) => <option key={file.path} value={file.path}>{file.fileName}</option>)}</select>
            <p className="text-xs leading-5 text-muted">Arrange the template below. Every checked file will be matched by track type, language, name, codec, and audio channels.</p>
            <div className="rounded-md border border-border bg-panel p-3 text-xs leading-5 text-muted">The original files are replaced using validated temporary files with automatic rollback. Files that cannot be matched safely are blocked.</div>
          </div></MobileOptions>
          <div className="mt-3 flex shrink-0 flex-wrap items-center gap-2 border-t border-border pt-3" aria-label="Operation actions">
            <button type="button" onClick={() => { setIsSummaryExpanded(true); runPreview(); }} disabled={busy || !canRun} className="h-9 min-w-0 flex-1 rounded-md border border-border bg-button px-1.5 text-xs font-semibold disabled:text-disabled">Preview Summary</button>
            <button type="button" onClick={runApply} disabled={busy || !canRun} className="h-9 min-w-0 flex-1 rounded-md bg-accent px-1.5 text-xs font-semibold disabled:bg-button disabled:text-disabled">Apply Changes</button>
            <span className="w-full text-xs text-muted">{selectedMkvPaths.length} files selected</span><span role="status" className="w-full min-w-0 break-words text-xs text-muted [overflow-wrap:anywhere]">{statusText}</span>
          </div>
        </section>

        <div ref={panelRef} className="grid min-h-0 min-w-0" style={{ gridTemplateRows: `minmax(0,${filePanelPercent}fr) 12px minmax(0,${100 - filePanelPercent}fr)` }}>
          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
            <div className="flex shrink-0 items-center justify-between gap-3"><h2 className="text-base font-semibold">File Info</h2><div className="flex gap-2 text-xs"><button type="button" className="rounded-md border border-border px-3 py-1.5" onClick={() => setSelectedPaths(mkvFiles.map((file) => file.path))}>Select all</button><button type="button" className="rounded-md border border-border px-3 py-1.5" onClick={() => setSelectedPaths(template ? [template.path] : [])}>Clear batch</button></div></div>
            <div className="mt-3 min-h-0 flex-1 overflow-auto" aria-label="Reorder Tracks file selection"><table className="file-info-table border-collapse text-left text-sm">
              <thead className="sticky top-0 bg-card text-xs text-text"><tr>{(["file", "reader", "codec", "audio", "subtitles", "status"] as FileSortKey[]).map((key) => <SortableColumnHeader key={key} active={fileSort.key === key} direction={fileSort.direction} label={{ file: "File", reader: "Reader", codec: "Codec", audio: "Audio", subtitles: "Subtitles", status: "Status" }[key]} onSort={() => setFileSort((current) => ({ key, direction: current.key === key && current.direction === "asc" ? "desc" : "asc" }))} />)}</tr></thead>
              <tbody>{displayedFiles.map((file) => {
                const isTemplate = normalizePath(file.path) === normalizePath(templatePath);
                const selected = selectedMkvPaths.some((path) => normalizePath(path) === normalizePath(file.path));
                return <tr key={file.path} onClick={() => setSelectedDetailPath(file.path)} className={normalizePath(selectedDetailPath) === normalizePath(file.path) ? "cursor-pointer bg-selected" : "cursor-pointer bg-card hover:bg-selected"}>
                  <td className="border-b border-border px-3 py-2"><div className="flex min-w-0 items-center gap-3"><input aria-label={`Select ${file.fileName}`} type="checkbox" checked={selected} disabled={isTemplate} onClick={(event) => event.stopPropagation()} onChange={() => toggleSelectedPath(file.path)} /><FileName value={file.fileName} /></div></td>
                  <td className="border-b border-border px-3 py-2">{file.reader}</td><td className="border-b border-border px-3 py-2">{file.codec || "Unknown"}</td><td className="max-w-[14rem] truncate border-b border-border px-3 py-2" title={file.audioSummary}>{file.audioSummary || "None"}</td><td className="max-w-[14rem] truncate border-b border-border px-3 py-2" title={file.subtitleSummary}>{file.subtitleSummary || "None"}</td><td className={["border-b border-border px-3 py-2", isTemplate ? "text-template" : selected ? "text-success" : "text-muted"].join(" ")}>{isTemplate ? "Template" : selected ? "Selected" : "Not selected"}</td>
                </tr>;
              })}</tbody>
            </table></div>
          </section>

          <PanelDivider containerRef={panelRef} value={filePanelPercent} onChange={setFilePanelPercent} />
          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
            <div className="flex shrink-0 items-center justify-between gap-3"><h2 className="text-sm font-semibold">Template Track Order</h2><span className="truncate text-xs text-muted">{template?.fileName ?? "No template"}</span></div>
            <div className="mt-3 min-h-0 flex-1 overflow-auto"><table className="w-full min-w-[45rem] table-fixed border-collapse text-left text-sm">
              <thead className="sticky top-0 bg-card text-xs text-text"><tr><th className="w-16 border-b border-border px-3 py-2">Order</th><th className="w-24 border-b border-border px-3 py-2">Type</th><th className="w-20 border-b border-border px-3 py-2">Lang</th><th className="w-40 border-b border-border px-3 py-2">Codec</th><th className="border-b border-border px-3 py-2">Name</th><th className="w-24 border-b border-border px-3 py-2">Move</th></tr></thead>
              <tbody>{orderedTracks.map((track: NonNullable<typeof template>["tracks"][number], index: number) => <tr key={track.id} className="bg-card hover:bg-selected"><td className="border-b border-border px-3 py-2 font-mono text-accent">#{index + 1}</td><td className="border-b border-border px-3 py-2">{track.type}</td><td className="border-b border-border px-3 py-2">{track.language || "und"}</td><td className="truncate border-b border-border px-3 py-2">{track.codec || "Unknown"}</td><td className="truncate border-b border-border px-3 py-2">{track.name || "-"}</td><td className="border-b border-border px-3 py-2"><div className="flex gap-1"><button aria-label={`Move ${track.name || `track ${track.id}`} up`} disabled={index === 0} onClick={() => move(index, -1)} className="rounded border border-border p-1 disabled:text-disabled"><ArrowUp size={14}/></button><button aria-label={`Move ${track.name || `track ${track.id}`} down`} disabled={index === orderedTracks.length - 1} onClick={() => move(index, 1)} className="rounded border border-border p-1 disabled:text-disabled"><ArrowDown size={14}/></button></div></td></tr>)}</tbody>
            </table>{!template ? <div className="flex h-full items-center justify-center text-sm text-muted">Load MKV files from Dashboard or Library first.</div> : null}</div>
          </section>
        </div>
      </div>
      {isSummaryExpanded ? <PreviewSummaryModal title="Reorder Tracks Preview Summary" emptyText="Build a preview to see track-order changes." available={previewResult !== null} status={previewResult?.status ?? ""} summary={previewResult?.summary ?? ""} metrics={[{ label: "Files changing", value: new Set(previewResult?.actions.map((action) => action.filePath) ?? []).size, tone: "text-success" }, { label: "No change", value: previewResult?.noChangeFiles.length ?? 0, tone: "text-muted" }]} sections={[{ title: "Planned operations", emptyText: "No track-order changes are needed.", rows: previewResult?.actions.map((action) => ({ key: `${action.filePath}-${action.index}`, title: action.fileName, detail: action.description, tone: "text-success" })) ?? [] }]} onClose={() => setIsSummaryExpanded(false)} /> : null}
    </div>
  );
}

function normalizePath(path: string) { return path.replace(/\\/g, "/").toLowerCase(); }
function sortFiles(files: ReturnType<typeof useMediaLibrary>["files"], sort: { key: FileSortKey; direction: SortDirection }, templatePath: string) {
  const multiplier = sort.direction === "asc" ? 1 : -1;
  return files.map((file, index) => ({ file, index })).sort((left, right) => {
    const value = (file: typeof left.file) => sort.key === "file" ? file.fileName : sort.key === "reader" ? file.reader : sort.key === "codec" ? file.codec : sort.key === "audio" ? file.audioSummary : sort.key === "subtitles" ? file.subtitleSummary : normalizePath(file.path) === normalizePath(templatePath) ? "Template" : "Selected";
    const compared = (value(left.file) || "").localeCompare(value(right.file) || "", undefined, { numeric: true, sensitivity: "base" });
    return compared === 0 ? left.index - right.index : compared * multiplier;
  }).map(({ file }) => file);
}
