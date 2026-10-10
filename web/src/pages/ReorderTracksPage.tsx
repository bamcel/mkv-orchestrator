import { useEffect, useMemo, useState } from "react";
import { ArrowDown, ArrowUp, ListRestart } from "lucide-react";

import { buildMuxPreview, startMuxApply, type MuxPreviewRequest, type MuxPreviewResponse } from "../api";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useOperationJob } from "../state/OperationJobContext";

export function ReorderTracksPage() {
  const { files, selectedPaths, templateFilePath } = useMediaLibrary();
  const operation = useOperationJob();
  const selectedFiles = useMemo(() => files.filter((file) => selectedPaths.includes(file.path) && file.extension.toLowerCase() === ".mkv"), [files, selectedPaths]);
  const [templatePath, setTemplatePath] = useState("");
  const template = selectedFiles.find((file) => file.path === templatePath) ?? selectedFiles[0] ?? null;
  const [order, setOrder] = useState<number[]>([]);
  const [preview, setPreview] = useState<MuxPreviewResponse | null>(null);
  const [status, setStatus] = useState("Choose a template file and preview the batch.");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const next = selectedFiles.some((file) => file.path === templateFilePath) ? templateFilePath : selectedFiles[0]?.path ?? "";
    setTemplatePath((current) => selectedFiles.some((file) => file.path === current) ? current : next);
  }, [selectedFiles.map((file) => file.path).join("\0"), templateFilePath]);

  useEffect(() => {
    setOrder(template?.tracks.slice().sort((a, b) => a.trackNumber - b.trackNumber).map((track) => track.id) ?? []);
    setPreview(null);
  }, [template?.path]);

  const orderedTracks = order.map((id) => template?.tracks.find((track) => track.id === id)).filter(Boolean) as NonNullable<typeof template>["tracks"];

  function move(index: number, delta: number) {
    const target = index + delta;
    if (target < 0 || target >= order.length) return;
    setOrder((current) => {
      const next = [...current];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });
    setPreview(null);
  }

  function request(plan?: MuxPreviewResponse): MuxPreviewRequest {
    return {
      files,
      selectedPaths: selectedFiles.map((file) => file.path),
      removeUnwantedAudioLanguages: false, keepAudioLanguages: "",
      removeUnwantedSubtitleLanguages: false, keepSubtitleLanguages: "",
      removeUnwantedTrackIds: false, removeTrackIdsText: "",
      preserveChapters: true, preserveAttachments: true,
      preserveOriginal: false, remuxOutputSuffix: ".remuxed",
      reorderTracks: true, reorderTemplatePath: templatePath, reorderTemplateTrackIds: order,
      muxMatchingExternalSubtitles: false, manualSubtitleSelections: [],
      externalSubtitleLanguage: "und", externalSubtitleTrackName: null, externalSubtitleFormats: "",
      preserveExternalSubtitleFiles: true, skipMuxIfSubtitleAlreadyExists: false,
      extractSubtitles: false, extractSubtitleLanguages: "", extractOverwriteExistingFiles: false,
      convertMp4ToMkv: false, deleteMp4AfterConvert: false,
      planId: plan?.planId ?? null, planFingerprint: plan?.planFingerprint ?? null,
      idempotencyKey: plan?.idempotencyKey ?? null
    };
  }

  async function buildPreview() {
    if (!template || selectedFiles.length === 0) return;
    setBusy(true);
    try {
      const result = await buildMuxPreview(request());
      setPreview(result);
      setStatus(result.status);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Preview failed.");
    } finally { setBusy(false); }
  }

  async function apply() {
    if (!preview?.actions.length) return;
    setBusy(true);
    try {
      const job = await startMuxApply(request(preview));
      operation.trackJob(job.id, "Reorder Tracks");
      setStatus(`Reordering tracks in ${job.total} file(s)...`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Apply failed.");
    } finally { setBusy(false); }
  }

  return (
    <div className="flex h-full min-h-[38rem] flex-col gap-4">
      <header><h1 className="text-xl font-bold">Reorder Tracks</h1><p className="mt-1 text-sm text-muted">Make every selected MKV follow one template track order.</p></header>
      <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[22rem_minmax(0,1fr)]">
        <section className="rounded-lg border border-border bg-card p-4">
          <label className="text-sm font-semibold" htmlFor="reorder-template">Template file</label>
          <select id="reorder-template" className="mt-2 w-full rounded-md border border-border bg-input px-3 py-2 text-sm" value={templatePath} onChange={(event) => setTemplatePath(event.target.value)}>
            {selectedFiles.map((file) => <option key={file.path} value={file.path}>{file.fileName}</option>)}
          </select>
          <p className="mt-2 text-xs text-muted">Tracks are matched by type, language, name, codec, and audio channels. Ambiguous files are blocked.</p>
          <div className="mt-4 flex gap-2"><button disabled={busy || !template} onClick={buildPreview} className="h-9 flex-1 rounded-md border border-border bg-button text-sm font-semibold disabled:text-disabled">Preview</button><button disabled={busy || !preview?.actions.length} onClick={apply} className="h-9 flex-1 rounded-md bg-accent text-sm font-semibold text-window disabled:bg-button disabled:text-disabled">Apply</button></div>
          <p className="mt-3 text-xs text-muted">{selectedFiles.length} selected MKV file(s)</p><p className="mt-2 text-sm text-success">{status}</p>
        </section>
        <section className="min-h-0 overflow-auto rounded-lg border border-border bg-card p-4">
          <div className="flex items-center gap-2"><ListRestart size={17}/><h2 className="font-semibold">Desired track order</h2></div>
          <div className="mt-3 space-y-2">
            {orderedTracks.map((track, index) => <div key={track.id} className="grid grid-cols-[3rem_1fr_auto] items-center gap-3 rounded-md border border-border bg-panel px-3 py-2">
              <span className="font-mono text-sm text-accent">#{index + 1}</span>
              <div className="min-w-0"><div className="truncate text-sm font-semibold">{track.name || "Unnamed track"}</div><div className="truncate text-xs text-muted">{track.type} · {track.language || "und"} · {track.codec} · source ID {track.id}</div></div>
              <div className="flex gap-1"><button aria-label={`Move ${track.name || `track ${track.id}`} up`} disabled={index === 0} onClick={() => move(index, -1)} className="rounded border border-border p-1.5 disabled:text-disabled"><ArrowUp size={15}/></button><button aria-label={`Move ${track.name || `track ${track.id}`} down`} disabled={index === orderedTracks.length - 1} onClick={() => move(index, 1)} className="rounded border border-border p-1.5 disabled:text-disabled"><ArrowDown size={15}/></button></div>
            </div>)}
            {!template ? <p className="text-sm text-muted">Select MKV files on Dashboard first.</p> : null}
          </div>
          {preview ? <div className="mt-5 rounded-md border border-border bg-panel p-3 text-sm"><div>{preview.summary}</div>{preview.actions.map((action) => <div key={action.filePath} className="mt-2 text-success">{action.fileName}: {action.description}</div>)}{preview.noChangeFiles.map((path) => <div key={path} className="mt-2 text-muted">Skipped: {path}</div>)}</div> : null}
        </section>
      </div>
    </div>
  );
}
