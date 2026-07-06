import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, Wand2 } from "lucide-react";
import { applyMuxPlan, buildMuxPreview, getCurrentScanFiles, getWebSettings, MuxPreviewRequest, MuxPreviewResponse } from "../api";
import { OutputModal } from "../components/OutputModal";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

export function MuxRemuxPage() {
  const { files, setFiles } = useMediaLibrary();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const settings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
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
  const [safeReplace, setSafeReplace] = useState(true);
  const [muxExternal, setMuxExternal] = useState(false);
  const [externalLanguage, setExternalLanguage] = useState("eng");
  const [externalFormats, setExternalFormats] = useState("srt,ass,ssa,sub,idx");
  const [preserveSidecars, setPreserveSidecars] = useState(true);
  const [skipExistingSubtitle, setSkipExistingSubtitle] = useState(true);
  const [extractSubtitles, setExtractSubtitles] = useState(false);
  const [extractLanguages, setExtractLanguages] = useState("eng");
  const [extractOverwrite, setExtractOverwrite] = useState(false);
  const [previewResult, setPreviewResult] = useState<MuxPreviewResponse | null>(null);
  const [statusText, setStatusText] = useState("Load scanned files from Dashboard, then build a preview.");
  const [settingsDefaultsApplied, setSettingsDefaultsApplied] = useState(false);
  const [isSummaryExpanded, setIsSummaryExpanded] = useState(false);

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
  }, [currentScan.data, files.length, setFiles]);

  useEffect(() => {
    if (!settings.data || settingsDefaultsApplied) return;
    setAudioLanguages(settings.data.mkvMergeDefaultAudioLanguages || "eng,jpn");
    setSubtitleLanguages(settings.data.mkvMergeDefaultSubtitleLanguages || "eng");
    setSettingsDefaultsApplied(true);
  }, [settings.data, settingsDefaultsApplied]);

  useEffect(() => {
    setSelectedPaths((current) => {
      if (files.length === 0) return [];
      const existing = current.filter((path) => files.some((file) => file.path === path));
      return existing.length > 0 ? existing : files.map((file) => file.path);
    });

    setSelectedDetailPath((current) => current && files.some((file) => file.path === current) ? current : files[0]?.path ?? "");
  }, [files]);

  const mkvFiles = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mkv"), [files]);
  const selectedMkvPaths = useMemo(
    () => selectedPaths.filter((path) => files.some((file) => file.path === path && file.extension.toLowerCase() === ".mkv")),
    [files, selectedPaths]
  );
  const selectedNonMkvCount = selectedPaths.length - selectedMkvPaths.length;
  const selectedDetailFile = useMemo(
    () => files.find((file) => file.path === selectedDetailPath) ?? files[0] ?? null,
    [files, selectedDetailPath]
  );
  const selectedCount = selectedPaths.length;

  const preview = useMutation({
    mutationFn: buildMuxPreview,
    onSuccess: (response) => {
      setPreviewResult(response);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.")
  });

  const apply = useMutation({
    mutationFn: applyMuxPlan,
    onSuccess: (response) => {
      setPreviewResult(response);
      setStatusText(response.status);
      currentScan.refetch().then((result) => {
        if (result.data?.files.length) setFiles(result.data.files);
      });
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Apply failed.")
  });

  function buildRequest(): MuxPreviewRequest {
    return {
      files,
      selectedPaths: selectedMkvPaths,
      removeUnwantedAudioLanguages: removeAudio,
      keepAudioLanguages: audioLanguages,
      removeUnwantedSubtitleLanguages: removeSubtitles,
      keepSubtitleLanguages: subtitleLanguages,
      removeUnwantedTrackIds: removeTrackIds,
      removeTrackIdsText: trackIds,
      preserveChapters,
      preserveAttachments,
      useSafeTempReplacement: safeReplace,
      muxMatchingExternalSubtitles: muxExternal,
      externalSubtitleLanguage: externalLanguage,
      externalSubtitleFormats: externalFormats,
      preserveExternalSubtitleFiles: preserveSidecars,
      skipMuxIfSubtitleAlreadyExists: skipExistingSubtitle,
      extractSubtitles,
      extractSubtitleLanguages: extractLanguages,
      extractOverwriteExistingFiles: extractOverwrite
    };
  }

  async function refreshFiles() {
    const result = await currentScan.refetch();
    if (result.data?.files.length) {
      setFiles(result.data.files);
      setStatusText(`Loaded ${result.data.files.length} scanned file(s).`);
    } else {
      setStatusText("No Dashboard scan is available yet.");
    }
  }

  function togglePath(path: string) {
    setSelectedPaths((current) =>
      current.includes(path) ? current.filter((item) => item !== path) : [...current, path]
    );
  }

  function runPreview() {
    if (selectedMkvPaths.length === 0) {
      setStatusText("Mux / Remux requires at least one selected MKV file.");
      return;
    }

    preview.mutate(buildRequest());
  }

  function runApply() {
    if (!previewResult?.actions.length) {
      setStatusText("Build a preview with planned actions before applying.");
      return;
    }

    setStatusText(`Applying ${previewResult.actions.length} mux/remux action(s)...`);
    apply.mutate(buildRequest());
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Mux / Remux" description="Remove tracks, mux matching subtitle sidecars, or extract subtitle tracks with MKVToolNix." />
      <div className="grid min-h-0 flex-1 grid-cols-[370px_1fr] gap-3">
        <section className="min-h-0 overflow-hidden rounded-lg border border-border bg-card p-3 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex justify-end">
            <button onClick={refreshFiles} className="rounded-md border border-border bg-button px-2.5 py-1.5 text-xs font-semibold text-muted hover:bg-button-hover hover:text-text">Refresh</button>
          </div>

          <div className="mt-2 flex gap-5 text-sm">
            {(["remux", "subtitles"] as const).map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                className={["pb-1 font-semibold capitalize", activeTab === tab ? "border-b border-accent text-text" : "text-muted hover:text-text"].join(" ")}
              >
                {tab}
              </button>
            ))}
          </div>

          {activeTab === "remux" ? (
            <div className="mt-4 space-y-3">
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
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={safeReplace} onChange={(event) => setSafeReplace(event.target.checked)} /> Use safe temp-file replacement</label>
            </div>
          ) : null}

          {activeTab === "subtitles" ? (
            <div className="mt-4 space-y-3">
              <h2 className="text-sm font-semibold">Subtitle Mux</h2>
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={muxExternal} onChange={(event) => setMuxExternal(event.target.checked)} /> Mux matching external subtitles</label>
              <div className="text-sm text-muted">File Format: <span className="text-accent">base_name.language.tag.ext</span></div>
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
            <button onClick={runPreview} disabled={preview.isPending || selectedMkvPaths.length === 0} className="inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              {preview.isPending ? <RefreshCw size={15} className="animate-spin" /> : <Wand2 size={15} />}
              Preview
            </button>
            <button onClick={runApply} disabled={apply.isPending || selectedMkvPaths.length === 0 || !previewResult?.actions.length} className="h-10 flex-1 rounded-md bg-accent text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
              {apply.isPending ? "Applying..." : "Apply"}
            </button>
          </div>
          <div className="mt-3 line-clamp-2 text-sm text-success">{statusText}</div>
          <div className="mt-1 text-xs text-muted">
            {selectedCount} selected | {selectedMkvPaths.length} selected MKV | {mkvFiles.length} MKV available
          </div>
          {selectedNonMkvCount > 0 ? (
            <div className="mt-1 text-xs text-warning">{selectedNonMkvCount} selected non-MKV file(s) are visible for context and excluded from mux/remux.</div>
          ) : null}
        </section>

        <div className="grid min-h-0 min-w-0 grid-rows-[1.3fr_1fr_190px] gap-3">
          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
            <h2 className="shrink-0 text-base font-semibold">File Info</h2>
            <div className="mt-3 min-h-0 flex-1 overflow-auto">
              <table className="w-full min-w-[900px] border-collapse text-left text-sm">
                <thead className="sticky top-0 bg-card text-xs text-text">
                  <tr>
                    <th className="border-b border-border px-3 py-2">File</th>
                    <th className="w-36 border-b border-border px-3 py-2">Audio</th>
                    <th className="w-36 border-b border-border px-3 py-2">Subtitles</th>
                  </tr>
                </thead>
                <tbody>
                  {files.map((file) => (
                    <tr
                      key={file.path}
                      onClick={() => setSelectedDetailPath(file.path)}
                      className={[selectedDetailFile?.path === file.path ? "bg-selected" : "bg-card", "cursor-pointer hover:bg-selected"].join(" ")}
                    >
                      <td className="border-b border-border px-3 py-2">
                        <div className="flex min-w-0 items-center gap-3">
                          <input type="checkbox" checked={selectedPaths.includes(file.path)} onClick={(event) => event.stopPropagation()} onChange={() => togglePath(file.path)} />
                          <span className="truncate" title={file.path}>{file.fileName}</span>
                        </div>
                      </td>
                      <td className="truncate border-b border-border px-3 py-2">{file.audioSummary || "None"}</td>
                      <td className="truncate border-b border-border px-3 py-2">{file.subtitleSummary || "None"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
            <div className="flex shrink-0 gap-6 text-sm">
              <button onClick={() => setDetailTab("tracks")} className={detailTab === "tracks" ? "border-b border-accent pb-1 font-semibold text-text" : "pb-1 font-semibold text-muted"}>File Details: Tracks</button>
              <button onClick={() => setDetailTab("attachments")} className={detailTab === "attachments" ? "border-b border-accent pb-1 font-semibold text-text" : "pb-1 font-semibold text-muted"}>File Details: Attachments</button>
            </div>
            <div className="mt-3 min-h-0 flex-1 overflow-auto">
              {detailTab === "tracks" ? (
                <table className="w-full min-w-[720px] table-fixed border-collapse text-left text-sm">
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
                <table className="w-full min-w-[760px] table-fixed border-collapse text-left text-sm">
                  <thead className="sticky top-0 bg-card text-xs text-text">
                    <tr>
                      <th className="w-14 border-b border-border px-3 py-2">#</th>
                      <th className="w-[260px] border-b border-border px-3 py-2">File</th>
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
                <div className="flex h-full min-h-[120px] items-center justify-center text-sm text-subtle">
                  No attachments or fonts were detected for the selected file.
                </div>
              )}
            </div>
          </section>

          <section className="rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-semibold">Preview Summary</h3>
              <button
                type="button"
                onClick={() => setIsSummaryExpanded(true)}
                className="h-7 rounded-md bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                Expand
              </button>
            </div>
            <pre className="mt-3 h-[125px] overflow-auto whitespace-pre-wrap break-words rounded-md bg-input p-3 font-mono text-xs leading-5 text-muted">
              {previewResult?.summary || "Build a preview to see planned mux/remux operations."}
            </pre>
          </section>
        </div>
      </div>
      {isSummaryExpanded ? (
        <OutputModal
          title="Mux / Remux Preview Summary"
          content={previewResult?.summary || "Build a preview to see planned mux/remux operations."}
          onClose={() => setIsSummaryExpanded(false)}
        />
      ) : null}
    </div>
  );
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

function formatBytes(value: number | null) {
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
