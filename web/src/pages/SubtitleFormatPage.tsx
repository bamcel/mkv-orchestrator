import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { buildMuxPreview, getWebSettings, saveWebSettings, startMuxApply, type MuxPreviewRequest, type MuxPreviewResponse } from "../api";
import { FileName } from "../components/FileName";
import { PreviewSummaryModal } from "../components/PreviewSummaryModal";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useOperationJob } from "../state/OperationJobContext";
import { builtInSubtitleFonts, deleteImportedFont, fileToSubtitleFont, loadImportedFonts, registerPreviewFont, resolveFontData, type SubtitleFont } from "../lib/subtitleFonts";

export function SubtitleFormatPage() {
  const { files, selectedPaths, setSelectedPaths, toggleSelectedPath } = useMediaLibrary();
  const operation = useOperationJob();
  const mkvFiles = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mkv"), [files]);
  const selectedMkvPaths = useMemo(() => selectedPaths.filter((path) => mkvFiles.some((file) => samePath(file.path, path))), [mkvFiles, selectedPaths]);
  const [fontFamily, setFontFamily] = useState("Andika Basic");
  const [fontLibrary, setFontLibrary] = useState<SubtitleFont[]>(builtInSubtitleFonts);
  const [selectedFontId, setSelectedFontId] = useState(builtInSubtitleFonts[0].id);
  const [fontFileName, setFontFileName] = useState("");
  const [fontDataBase64, setFontDataBase64] = useState("");
  const [primaryColor, setPrimaryColor] = useState("#FFFFFF");
  const [outlineColor, setOutlineColor] = useState("#000000");
  const [outlineSize, setOutlineSize] = useState("2");
  const [keepTiming, setKeepTiming] = useState(true);
  const [timingOffset, setTimingOffset] = useState("0");
  const [selectedTrackIds, setSelectedTrackIds] = useState<Record<string, number[]>>({});
  const [previewResult, setPreviewResult] = useState<MuxPreviewResponse | null>(null);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const [status, setStatus] = useState("Select MKV files and choose the new ASS/SSA subtitle style.");
  const offsetMs = keepTiming ? 0 : Math.round((Number(timingOffset) || 0) * 1000);
  const selectedTrackCount = selectedMkvPaths.reduce((count, path) => count + (selectedTrackIds[pathKey(path)]?.length ?? 0), 0);
  const compatibleCount = selectedMkvPaths.filter((path) => (selectedTrackIds[pathKey(path)]?.length ?? 0) > 0).length;
  const batchTrackIds = useMemo(() => {
    const counts = new Map<number, { available: number; selected: number }>();
    for (const file of mkvFiles.filter((candidate) => selectedMkvPaths.some((path) => samePath(path, candidate.path)))) {
      const selected = selectedTrackIds[pathKey(file.path)] ?? [];
      for (const track of file.tracks.filter(isEditableSubtitle)) {
        const count = counts.get(track.id) ?? { available: 0, selected: 0 };
        count.available += 1;
        if (selected.includes(track.id)) count.selected += 1;
        counts.set(track.id, count);
      }
    }
    return [...counts.entries()].sort(([left], [right]) => left - right).map(([id, count]) => ({ id, ...count }));
  }, [mkvFiles, selectedMkvPaths, selectedTrackIds]);

  useEffect(() => {
    void loadSavedFonts();
    void selectFont(builtInSubtitleFonts[0]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function loadSavedFonts() {
    try {
      const settings = await getWebSettings();
      const serverFonts = (settings.subtitleFonts ?? []).map((font) => ({ ...font, builtIn: false } satisfies SubtitleFont));
      const legacyFonts = await loadImportedFonts().catch(() => []);
      const imported = uniqueFonts([...serverFonts, ...legacyFonts]);
      setFontLibrary([...builtInSubtitleFonts, ...imported]);
      if (legacyFonts.some((font) => !serverFonts.some((saved) => saved.id === font.id))) {
        await saveWebSettings({ subtitleFonts: imported.map(fontDto) });
      }
    } catch (error) {
      setStatus(error instanceof Error ? `Saved fonts could not be loaded: ${error.message}` : "Saved fonts could not be loaded from MKVO storage.");
    }
  }

  useEffect(() => {
    setSelectedTrackIds((current) => {
      const next = { ...current };
      for (const file of mkvFiles) {
        const key = pathKey(file.path);
        if (!(key in next)) next[key] = file.tracks.filter(isEditableSubtitle).map((track) => track.id);
      }
      return next;
    });
  }, [mkvFiles]);

  async function selectFont(font: SubtitleFont): Promise<boolean> {
    try {
      const resolved = await resolveFontData(font);
      setSelectedFontId(font.id); setFontFamily(font.family); setFontFileName(font.fileName); setFontDataBase64(resolved.dataBase64 ?? ""); setPreviewResult(null);
      try {
        await registerPreviewFont(resolved);
        return true;
      } catch {
        setStatus(`${font.family} is selected, but this browser cannot render a live preview of the font. It can still be attached and used for subtitle formatting.`);
        return false;
      }
    } catch (error) { setStatus(error instanceof Error ? error.message : "The font could not be loaded."); return false; }
  }

  async function importFont(file: File | undefined) {
    if (!file) return;
    if (!/\.(?:ttf|otf)$/i.test(file.name)) { setStatus("Choose a TrueType (.ttf) or OpenType (.otf) font."); return; }
    if (file.size > 1024 * 1024) { setStatus("Font files must be 1 MB or smaller."); return; }
    try {
      const font = await fileToSubtitleFont(file);
      setFontLibrary((current) => [...current.filter((item) => item.id !== font.id), font]);
      const previewAvailable = await selectFont(font);
      try {
        const imported = uniqueFonts([...fontLibrary.filter((item) => !item.builtIn), font]);
        await saveWebSettings({ subtitleFonts: imported.map(fontDto) });
        setStatus(`${font.family} was imported and saved for future operations.${previewAvailable ? "" : " Live preview is unavailable for this font, but subtitle formatting can still use it."}`);
      } catch {
        setStatus(`${font.family} was imported for this session, but MKVO storage could not save it. It will need to be imported again after a restart.`);
      }
    } catch (error) { setStatus(error instanceof Error ? error.message : "The font could not be imported."); }
  }

  async function removeSelectedFont() {
    const selected = fontLibrary.find((font) => font.id === selectedFontId);
    if (!selected || selected.builtIn) return;
    try {
      const remaining = fontLibrary.filter((font) => !font.builtIn && font.id !== selected.id);
      await saveWebSettings({ subtitleFonts: remaining.map(fontDto) });
      await deleteImportedFont(selected.id);
      setFontLibrary((current) => current.filter((font) => font.id !== selected.id));
      await selectFont(builtInSubtitleFonts[0]);
      setStatus(`${selected.family} was removed from saved fonts.`);
    } catch (error) { setStatus(error instanceof Error ? error.message : "The imported font could not be removed."); }
  }

  const preview = useMutation({ mutationFn: buildMuxPreview, onSuccess: (result) => { setPreviewResult(result); setStatus(result.status); }, onError: (error) => setStatus(error instanceof Error ? error.message : "Preview failed.") });
  const apply = useMutation({ mutationFn: startMuxApply, onSuccess: (job) => { operation.trackJob(job.id, "Subtitle Format"); setStatus(`Formatting subtitles in ${job.total} file(s)...`); }, onError: (error) => setStatus(error instanceof Error ? error.message : "Apply failed.") });

  function request(plan?: MuxPreviewResponse): MuxPreviewRequest {
    return {
      files, selectedPaths: selectedMkvPaths,
      removeUnwantedAudioLanguages: false, keepAudioLanguages: "", removeUnwantedSubtitleLanguages: false, keepSubtitleLanguages: "", removeUnwantedTrackIds: false, removeTrackIdsText: "",
      preserveChapters: true, preserveAttachments: true, preserveOriginal: false, remuxOutputSuffix: ".remuxed",
      reorderTracks: false, reorderTemplatePath: "", reorderTemplateTrackIds: [],
      formatSubtitles: true, subtitleTrackSelections: selectedMkvPaths.map((filePath) => ({ filePath, trackIds: selectedTrackIds[pathKey(filePath)] ?? [] })), subtitleFontFamily: fontFamily.trim(), subtitlePrimaryColor: primaryColor, subtitleOutlineColor: outlineColor, subtitleOutlineSize: outlineSize, subtitleFontFileName: fontFileName, subtitleFontDataBase64: fontDataBase64, subtitleTimingOffsetMs: offsetMs,
      muxMatchingExternalSubtitles: false, manualSubtitleSelections: [], externalSubtitleLanguage: "und", externalSubtitleTrackName: null, externalSubtitleFormats: "", preserveExternalSubtitleFiles: true, skipMuxIfSubtitleAlreadyExists: false,
      extractSubtitles: false, extractSubtitleLanguages: "", extractOverwriteExistingFiles: false, convertMp4ToMkv: false, deleteMp4AfterConvert: false,
      planId: plan?.planId ?? null, planFingerprint: plan?.planFingerprint ?? null, idempotencyKey: plan?.idempotencyKey ?? null
    };
  }

  function runPreview() {
    if (!fontFamily.trim() || selectedMkvPaths.length === 0 || selectedTrackCount === 0) return;
    setSummaryOpen(true);
    preview.mutate(request());
  }

  async function runApply() {
    if (!fontFamily.trim() || selectedMkvPaths.length === 0 || selectedTrackCount === 0) return;
    try {
      const plan = await preview.mutateAsync(request());
      if (!plan.actions.length) { setStatus("No compatible subtitle changes are available."); return; }
      await apply.mutateAsync({ ...request(plan), idempotencyKey: plan.idempotencyKey ?? crypto.randomUUID() });
    } catch { /* Mutation handlers show the failure. */ }
  }

  const busy = preview.isPending || apply.isPending || (operation.activeOperation?.label === "Subtitle Format" && operation.isRunning);
  const shiftedTime = formatOffset(offsetMs);

  function toggleTrack(filePath: string, trackId: number) {
    const key = pathKey(filePath);
    setSelectedTrackIds((current) => {
      const selected = current[key] ?? [];
      return { ...current, [key]: selected.includes(trackId) ? selected.filter((id) => id !== trackId) : [...selected, trackId] };
    });
    setPreviewResult(null);
  }

  function setBatchTrack(trackId: number, select: boolean) {
    setSelectedTrackIds((current) => {
      const next = { ...current };
      for (const file of mkvFiles) {
        if (!selectedMkvPaths.some((path) => samePath(path, file.path)) || !file.tracks.some((track) => track.id === trackId && isEditableSubtitle(track))) continue;
        const key = pathKey(file.path);
        const selected = current[key] ?? [];
        next[key] = select ? [...new Set([...selected, trackId])] : selected.filter((id) => id !== trackId);
      }
      return next;
    });
    setPreviewResult(null);
  }

  function setAllBatchTracks(select: boolean) {
    setSelectedTrackIds((current) => {
      const next = { ...current };
      for (const file of mkvFiles) {
        if (!selectedMkvPaths.some((path) => samePath(path, file.path))) continue;
        next[pathKey(file.path)] = select ? file.tracks.filter(isEditableSubtitle).map((track) => track.id) : [];
      }
      return next;
    });
    setPreviewResult(null);
  }

  return <div className="workspace-page flex h-full min-h-0 flex-col">
    <SectionHeader title="Subtitle Format" description="Batch-change ASS/SSA subtitle fonts and optionally shift cue timing." />
    <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[minmax(14rem,18.75rem)_minmax(0,1fr)] gap-3">
      <section className="operation-sidebar flex min-h-0 flex-col overflow-hidden rounded-lg border border-border bg-card p-3">
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto">
          <h2 className="text-base font-semibold">Format Options</h2>
          <label className="block"><span className="text-xs font-semibold text-muted">Saved font</span><select aria-label="Saved font" value={selectedFontId} onChange={(event) => { const font = fontLibrary.find((item) => item.id === event.target.value); if (font) void selectFont(font); }} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-2 text-sm"><optgroup label="Bundled fonts">{fontLibrary.filter((font) => font.builtIn).map((font) => <option key={font.id} value={font.id}>{font.family}</option>)}</optgroup>{fontLibrary.some((font) => !font.builtIn) ? <optgroup label="Imported fonts">{fontLibrary.filter((font) => !font.builtIn).map((font) => <option key={font.id} value={font.id}>{font.family}</option>)}</optgroup> : null}</select></label>
          <div className="flex gap-2"><label className="flex h-9 flex-1 cursor-pointer items-center justify-center rounded-md border border-border bg-button px-2 text-xs font-semibold">Import .ttf/.otf<input aria-label="Import font file" type="file" accept=".ttf,.otf,font/ttf,font/otf" className="sr-only" onChange={(event) => { void importFont(event.target.files?.[0]); event.target.value = ""; }} /></label><button type="button" onClick={() => void removeSelectedFont()} disabled={fontLibrary.find((font) => font.id === selectedFontId)?.builtIn !== false} className="h-9 rounded-md border border-border px-2 text-xs disabled:text-disabled">Remove imported</button></div>
          <label className="block"><span className="text-xs font-semibold text-muted">ASS font family name</span><input aria-label="New font family" value={fontFamily} onChange={(event) => { setFontFamily(event.target.value); setPreviewResult(null); }} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm" placeholder="Font family" /></label>
          <div className="grid grid-cols-2 gap-2"><ColorField label="Font color" value={primaryColor} onChange={(value) => { setPrimaryColor(value); setPreviewResult(null); }} /><ColorField label="Outline color" value={outlineColor} onChange={(value) => { setOutlineColor(value); setPreviewResult(null); }} /></div>
          <label className="block"><span className="text-xs font-semibold text-muted">Outline thickness</span><input aria-label="Outline thickness" type="number" min="0" max="20" step="0.5" value={outlineSize} onChange={(event) => { setOutlineSize(event.target.value); setPreviewResult(null); }} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm" /></label>
          <fieldset className="space-y-2"><legend className="mb-2 text-xs font-semibold text-muted">Subtitle timing</legend>
            <label className="flex items-center gap-2 text-sm"><input type="radio" name="subtitle-timing" checked={keepTiming} onChange={() => { setKeepTiming(true); setPreviewResult(null); }} /> Keep the same timing</label>
            <label className="flex items-center gap-2 text-sm"><input type="radio" name="subtitle-timing" checked={!keepTiming} onChange={() => { setKeepTiming(false); setPreviewResult(null); }} /> Shift all cues</label>
          </fieldset>
          {!keepTiming ? <label className="block"><span className="text-xs font-semibold text-muted">Offset in seconds</span><input aria-label="Timing offset in seconds" type="number" step="0.1" value={timingOffset} onChange={(event) => { setTimingOffset(event.target.value); setPreviewResult(null); }} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm" /><span className="mt-1 block text-xs text-muted">Positive delays subtitles; negative shows them earlier.</span></label> : null}
          <div className="rounded-md border border-border bg-panel p-3"><div className="flex items-center justify-between gap-2"><h3 className="text-xs font-semibold text-muted">Batch subtitle Track IDs</h3><div className="flex gap-1"><button type="button" onClick={() => setAllBatchTracks(true)} disabled={!batchTrackIds.length} className="rounded border border-border px-2 py-1 text-[0.6875rem] disabled:text-disabled">Select all</button><button type="button" onClick={() => setAllBatchTracks(false)} disabled={!batchTrackIds.length} className="rounded border border-border px-2 py-1 text-[0.6875rem] disabled:text-disabled">Clear all</button></div></div>
            {batchTrackIds.length ? <div className="mt-2 flex flex-wrap gap-x-4 gap-y-2">{batchTrackIds.map((track) => <BatchTrackCheckbox key={track.id} trackId={track.id} available={track.available} selected={track.selected} onChange={(select) => setBatchTrack(track.id, select)} />)}</div> : <p className="mt-2 text-xs text-muted">Select files with editable ASS/SSA tracks.</p>}
            <p className="mt-2 text-[0.6875rem] leading-4 text-muted">Changes each Track ID across every selected file where that editable ID exists.</p>
          </div>
          <div className="rounded-md border border-warning bg-panel p-3 text-xs leading-5 text-muted">Font editing supports ASS/SSA text subtitles. PGS, VobSub, and other bitmap subtitles have no editable font and will be blocked.</div>
        </div>
        <div className="mt-3 flex flex-wrap gap-2 border-t border-border pt-3"><button onClick={runPreview} disabled={busy || !fontFamily.trim() || selectedMkvPaths.length === 0 || selectedTrackCount === 0} className="h-9 flex-1 rounded-md border border-border bg-button text-xs font-semibold disabled:text-disabled">Preview Summary</button><button onClick={runApply} disabled={busy || !fontFamily.trim() || selectedMkvPaths.length === 0 || selectedTrackCount === 0} className="h-9 flex-1 rounded-md bg-accent text-xs font-semibold disabled:bg-button disabled:text-disabled">Apply Changes</button><span className="w-full text-xs text-muted">{selectedMkvPaths.length} files selected · {selectedTrackCount} tracks across {compatibleCount} files</span><span role="status" className="w-full text-xs text-muted">{status}</span></div>
      </section>

      <div className="grid min-h-0 gap-3 grid-rows-[minmax(0,1fr)_minmax(15rem,0.75fr)]">
        <section className="flex min-h-0 flex-col rounded-lg border border-border bg-card p-4"><div className="flex items-center justify-between"><h2 className="text-base font-semibold">File Info</h2><div className="flex gap-2 text-xs"><button className="rounded border border-border px-3 py-1.5" onClick={() => setSelectedPaths(mkvFiles.map((file) => file.path))}>Select all</button><button className="rounded border border-border px-3 py-1.5" onClick={() => setSelectedPaths([])}>Clear selection</button></div></div>
          <div className="mt-3 min-h-0 flex-1 overflow-auto"><table className="file-info-table border-collapse text-left text-sm"><thead className="sticky top-0 bg-card text-xs"><tr><th className="border-b border-border px-3 py-2">File</th><th className="border-b border-border px-3 py-2">Tracks to format</th><th className="border-b border-border px-3 py-2">Status</th></tr></thead><tbody>{mkvFiles.map((file) => {
            const editableTracks = file.tracks.filter(isEditableSubtitle); const selected = selectedMkvPaths.some((path) => samePath(path, file.path)); const chosenIds = selectedTrackIds[pathKey(file.path)] ?? [];
            return <tr key={file.path} className="bg-card hover:bg-selected"><td className="border-b border-border px-3 py-2"><div className="flex items-center gap-3"><input aria-label={`Select ${file.fileName}`} type="checkbox" checked={selected} onChange={() => toggleSelectedPath(file.path)} /><FileName value={file.fileName}/></div></td><td className="border-b border-border px-3 py-2">{editableTracks.length ? <div className="flex flex-wrap gap-x-4 gap-y-2">{editableTracks.map((track) => <label key={track.id} className={["flex items-center gap-2", selected ? "" : "text-disabled"].join(" ")}><input aria-label={`${file.fileName} track ${track.trackNumber} ${track.name || track.language || "Subtitle"}`} type="checkbox" checked={chosenIds.includes(track.id)} disabled={!selected} onChange={() => toggleTrack(file.path, track.id)} /><span>ID {track.id} (#{track.trackNumber}) · {track.name || "Subtitle"} · {track.language || "und"}</span></label>)}</div> : <span className="text-muted">No ASS/SSA tracks</span>}</td><td className={["border-b border-border px-3 py-2", editableTracks.length && chosenIds.length ? "text-success" : "text-warning"].join(" ")}>{!editableTracks.length ? "No editable subtitles" : chosenIds.length ? `${chosenIds.length} selected` : "Choose a track"}</td></tr>;
          })}</tbody></table></div>
        </section>

        <section className="min-h-0 overflow-auto rounded-lg border border-border bg-card p-4"><h2 className="text-base font-semibold">Subtitle Preview</h2><div className="mt-3 grid gap-3 md:grid-cols-2">
          <PreviewCard label="Old subtitle format" font="Arial" color="#FFFFFF" outlineColor="#000000" outlineSize={2} time="00:01:12.000 → 00:01:15.000" />
          <PreviewCard label="New subtitle format" font={fontFamily || "Arial"} color={primaryColor} outlineColor={outlineColor} outlineSize={Number(outlineSize) || 0} time={`${shiftedTime.start} → ${shiftedTime.end}`} />
        </div><p className="mt-3 text-xs text-muted">The preview demonstrates the font and timing change. Existing colors, size, positioning, outlines, and other ASS style properties are preserved.</p></section>
      </div>
    </div>
    {summaryOpen ? <PreviewSummaryModal title="Subtitle Format Preview Summary" emptyText="Build a preview to see subtitle changes." available={previewResult !== null} status={previewResult?.status ?? ""} summary={previewResult?.summary ?? ""} metrics={[{ label: "Files changing", value: new Set(previewResult?.actions.map((action) => action.filePath) ?? []).size, tone: "text-success" }, { label: "Blocked/no change", value: previewResult?.noChangeFiles.length ?? 0, tone: "text-muted" }]} sections={[{ title: "Planned operations", emptyText: "No compatible subtitle changes are available.", rows: previewResult?.actions.map((action) => ({ key: `${action.filePath}-${action.index}`, title: action.fileName, detail: action.description, tone: "text-success" })) ?? [] }]} onClose={() => setSummaryOpen(false)} /> : null}
  </div>;
}

function PreviewCard({ label, font, color, outlineColor, outlineSize, time }: { label: string; font: string; color: string; outlineColor: string; outlineSize: number; time: string }) { const shadow = outlineSize <= 0 ? "none" : `${outlineSize}px 0 ${outlineColor}, -${outlineSize}px 0 ${outlineColor}, 0 ${outlineSize}px ${outlineColor}, 0 -${outlineSize}px ${outlineColor}`; return <div className="overflow-hidden rounded-md border border-border bg-window"><div className="border-b border-border px-3 py-2 text-xs font-semibold text-muted">{label}</div><div className="flex min-h-28 items-end justify-center bg-black px-4 py-5 text-center"><span style={{ fontFamily: font, color, textShadow: shadow }} className="text-xl">This is a subtitle preview.</span></div><div className="px-3 py-2 text-xs text-muted">Font: <span className="text-text">{font}</span> · Fill: <span className="text-text">{color}</span> · Outline: <span className="text-text">{outlineColor} / {outlineSize}</span><br/>Timing: <span className="text-text">{time}</span></div></div>; }
function BatchTrackCheckbox({ trackId, available, selected, onChange }: { trackId: number; available: number; selected: number; onChange: (select: boolean) => void }) { const input = useRef<HTMLInputElement>(null); const allSelected = available > 0 && selected === available; useEffect(() => { if (input.current) input.current.indeterminate = selected > 0 && !allSelected; }, [allSelected, selected]); return <label className="flex items-center gap-2 text-xs"><input ref={input} aria-label={`Batch Track ID ${trackId}`} type="checkbox" checked={allSelected} onChange={() => onChange(!allSelected)} /><span>ID {trackId} <span className="text-muted">({selected}/{available})</span></span></label>; }
function ColorField({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) { return <label className="block"><span className="text-xs font-semibold text-muted">{label}</span><span className="mt-1.5 flex h-9 overflow-hidden rounded-md border border-border bg-input"><input aria-label={label} type="color" value={value} onChange={(event) => onChange(event.target.value.toUpperCase())} className="h-full w-11 border-0 bg-transparent p-1"/><input aria-label={`${label} hex`} value={value} onChange={(event) => onChange(event.target.value.toUpperCase())} className="min-w-0 flex-1 bg-transparent px-2 text-xs"/></span></label>; }
function isEditableSubtitle(track: { type: string; codec: string }) { const codec = track.codec.toLowerCase(); return ["subtitle", "subtitles"].includes(track.type.toLowerCase()) && (codec.includes("substation") || codec === "ass" || codec === "ssa"); }
function samePath(left: string, right: string) { return left.replace(/\\/g, "/").toLowerCase() === right.replace(/\\/g, "/").toLowerCase(); }
function pathKey(path: string) { return path.replace(/\\/g, "/").toLowerCase(); }
function fontDto(font: SubtitleFont) { return { id: font.id, family: font.family, fileName: font.fileName, dataBase64: font.dataBase64 ?? "" }; }
function uniqueFonts(fonts: SubtitleFont[]) { return [...new Map(fonts.map((font) => [font.id, font])).values()]; }
function formatOffset(offset: number) { const format = (millis: number) => { const value = Math.max(0, millis); return `${String(Math.floor(value / 3_600_000)).padStart(2, "0")}:${String(Math.floor(value / 60_000) % 60).padStart(2, "0")}:${String(Math.floor(value / 1000) % 60).padStart(2, "0")}.${String(value % 1000).padStart(3, "0")}`; }; return { start: format(72_000 + offset), end: format(75_000 + offset) }; }
