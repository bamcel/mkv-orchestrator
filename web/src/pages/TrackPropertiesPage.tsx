import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, Wand2 } from "lucide-react";
import {
  buildPropEditPreview,
  cancelOperationJob,
  getCurrentScanFiles,
  getWebSettings,
  PropEditPreviewRequest,
  PropEditPreviewResponse,
  PropEditTemplateResponse,
  PropEditTrackConfigRow,
  startPropEditApply
} from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { PreviewSummaryModal } from "../components/PreviewSummaryModal";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useInvalidatePropEditTemplate, usePropEditTemplate } from "../state/propEditTemplate";
import { useOperationJob } from "../state/OperationJobContext";

type TitleMode = "keep" | "remove" | "file" | "episode_title";
type TrackType = "audio" | "subtitle";

const audioNamePresets = ["English", "Japanese", "Commentary"];
const subtitleNamePresets = ["Dialogue", "English", "English Forced", "English SDH", "Signs & Songs", "Fansub"];
const languagePresets = ["eng", "jpn", "kor", "und"];
const configurationStorageKey = "mkvo.web.trackPropertiesConfiguration";
const configurationVersion = 7;
const metadataTrackNameValue = "__mkvo_metadata_track_name__";

type StoredTrackPropertiesConfiguration = {
  configurationVersion?: number;
  scanUpdatedUtc: string | null;
  templatePath: string;
  containerMode: TitleMode;
  videoMode: TitleMode;
  videoLanguageEnabled: boolean;
  videoLanguage: string;
  defaultVideo: string;
  audioTracks: PropEditTrackConfigRow[];
  subtitleTracks: PropEditTrackConfigRow[];
  defaultAudio: string;
  forcedAudio: string;
  defaultSubtitle: string;
  forcedSubtitle: string;
  customTrackKeys: string[];
};

function readStoredConfiguration(): StoredTrackPropertiesConfiguration | null {
  try {
    const value = sessionStorage.getItem(configurationStorageKey);
    return value ? JSON.parse(value) as StoredTrackPropertiesConfiguration : null;
  } catch {
    return null;
  }
}

export function TrackPropertiesPage() {
  const { files, templateFilePath, setTemplateFilePath, syncFromBackend } = useMediaLibrary();
  const operation = useOperationJob();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const settings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const storedConfiguration = useRef(readStoredConfiguration());
  const [templatePath, setTemplatePath] = useState(storedConfiguration.current?.templatePath ?? "");
  const [containerMode, setContainerMode] = useState<TitleMode>(storedConfiguration.current?.containerMode ?? "keep");
  const [videoMode, setVideoMode] = useState<TitleMode>(storedConfiguration.current?.videoMode ?? "keep");
  const [videoLanguageEnabled, setVideoLanguageEnabled] = useState(storedConfiguration.current?.videoLanguageEnabled ?? false);
  const [videoLanguage, setVideoLanguage] = useState(storedConfiguration.current?.videoLanguage ?? "und");
  const [defaultVideo, setDefaultVideo] = useState(storedConfiguration.current?.defaultVideo ?? "Keep existing");
  const [template, setTemplate] = useState<PropEditTemplateResponse | null>(null);
  const [audioTracks, setAudioTracks] = useState<PropEditTrackConfigRow[]>([]);
  const [subtitleTracks, setSubtitleTracks] = useState<PropEditTrackConfigRow[]>([]);
  const [defaultAudio, setDefaultAudio] = useState("Keep existing");
  const [forcedAudio, setForcedAudio] = useState("None");
  const [defaultSubtitle, setDefaultSubtitle] = useState("Keep existing");
  const [forcedSubtitle, setForcedSubtitle] = useState("None");
  const [customTrackKeys, setCustomTrackKeys] = useState<Set<string>>(new Set());
  const [previewResult, setPreviewResult] = useState<PropEditPreviewResponse | null>(null);
  const [statusText, setStatusText] = useState("Load scanned files from Dashboard, then select a template.");
  const [isSummaryExpanded, setIsSummaryExpanded] = useState(false);
  const [configurationReady, setConfigurationReady] = useState(false);

  useEffect(() => {
    if (!currentScan.data) return;
    syncFromBackend(currentScan.data);
  }, [currentScan.data]);

  useEffect(() => {
    const sharedTemplate = files.some((file) => file.path === templateFilePath) ? templateFilePath : "";
    const currentTemplate = files.some((file) => file.path === templatePath) ? templatePath : "";
    const nextTemplate = sharedTemplate || currentTemplate || files.find((file) => file.extension.toLowerCase() === ".mkv")?.path || "";
    if (nextTemplate !== templatePath) setTemplatePath(nextTemplate);
    if (nextTemplate && nextTemplate !== templateFilePath) setTemplateFilePath(nextTemplate);
  }, [files, templateFilePath, templatePath]);

  const mkvFiles = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mkv"), [files]);
  // Track Properties is always a complete MKV batch. Selections made in
  // Rename, Mux/Remux, or the library do not narrow this workflow.
  const selectedMkvPaths = useMemo(() => mkvFiles.map((file) => file.path), [mkvFiles]);
  const nonMkvCount = files.length - mkvFiles.length;

  // Cached and warmed while the library loads, so arriving here usually finds
  // the tracks already read rather than starting the read.
  const templateLoad = usePropEditTemplate(templatePath, files);
  const invalidateTemplate = useInvalidatePropEditTemplate();

  // Seeds the editable copies from whatever the cache holds. The query returns
  // the same object until the template genuinely changes, so this does not
  // clobber edits in progress; when it does change, re-seeding is exactly the
  // reload that reflects it.
  const loadedTemplate = templateLoad.data;
  useEffect(() => {
    if (!loadedTemplate) return;
    const stored = storedConfiguration.current;
    const canRestore = Boolean(
      stored
      && stored.configurationVersion === configurationVersion
      && stored.scanUpdatedUtc === (currentScan.data?.updatedUtc ?? null)
      && stored.templatePath === loadedTemplate.templatePath
    );

    setTemplate(loadedTemplate);
    setAudioTracks(canRestore ? stored!.audioTracks : loadedTemplate.audioTracks);
    setSubtitleTracks(canRestore ? stored!.subtitleTracks : loadedTemplate.subtitleTracks);
    setDefaultAudio(canRestore ? stored!.defaultAudio : loadedTemplate.defaultAudio || "Keep existing");
    setForcedAudio(canRestore ? stored!.forcedAudio : loadedTemplate.forcedAudio || "None");
    setDefaultSubtitle(canRestore ? stored!.defaultSubtitle : loadedTemplate.defaultSubtitle || "Keep existing");
    setForcedSubtitle(canRestore ? stored!.forcedSubtitle : loadedTemplate.forcedSubtitle || "None");
    setCustomTrackKeys(new Set(canRestore ? stored!.customTrackKeys : []));
    setContainerMode(canRestore ? stored!.containerMode : "keep");
    setVideoMode(canRestore ? stored!.videoMode : "keep");
    setVideoLanguageEnabled(canRestore ? stored!.videoLanguageEnabled ?? false : false);
    setVideoLanguage(canRestore ? stored!.videoLanguage || "und" : "und");
    setDefaultVideo(canRestore ? stored!.defaultVideo || "Keep existing" : "Keep existing");
    storedConfiguration.current = canRestore ? stored : null;
    setPreviewResult(null);
    setConfigurationReady(true);
    setStatusText(`Template loaded: ${loadedTemplate.templateFileName}`);
  }, [loadedTemplate, currentScan.data?.updatedUtc]);

  useEffect(() => {
    if (!currentScan.data) return;
    if (!currentScan.data.updatedUtc || currentScan.data.files.length === 0) {
      try {
        sessionStorage.removeItem(configurationStorageKey);
      } catch {
        // Persistence is a convenience; track editing still works without it.
      }
      storedConfiguration.current = null;
      setConfigurationReady(false);
      return;
    }
    if (!configurationReady || !template) return;

    const value: StoredTrackPropertiesConfiguration = {
      configurationVersion,
      scanUpdatedUtc: currentScan.data.updatedUtc,
      templatePath,
      containerMode,
      videoMode,
      videoLanguageEnabled,
      videoLanguage,
      defaultVideo,
      audioTracks,
      subtitleTracks,
      defaultAudio,
      forcedAudio,
      defaultSubtitle,
      forcedSubtitle,
      customTrackKeys: [...customTrackKeys]
    };
    try {
      sessionStorage.setItem(configurationStorageKey, JSON.stringify(value));
    } catch {
      // Persistence is a convenience; track editing still works without it.
    }
  }, [
    currentScan.data, configurationReady, template, templatePath, containerMode, videoMode,
    videoLanguageEnabled, videoLanguage, defaultVideo, audioTracks, subtitleTracks, defaultAudio,
    forcedAudio, defaultSubtitle, forcedSubtitle, customTrackKeys
  ]);

  useEffect(() => {
    if (!templateLoad.error) return;
    setStatusText(
      templateLoad.error instanceof Error ? templateLoad.error.message : "Template load failed."
    );
  }, [templateLoad.error]);

  const preview = useMutation({
    mutationFn: buildPropEditPreview,
    onSuccess: (response) => {
      setPreviewResult(response);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.")
  });

  const apply = useMutation({
    mutationFn: startPropEditApply,
    onSuccess: (job) => {
      operation.trackJob(job.id, "Track Properties");
      setStatusText(`Applying ${job.total} track property edit(s)...`);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Apply failed.")
  });

  const cancelApply = useMutation({ mutationFn: cancelOperationJob });
  const runningJob = operation.activeOperation?.label === "Track Properties" ? operation.job : undefined;
  const isApplying = apply.isPending || (operation.activeOperation?.label === "Track Properties" && operation.isRunning);

  useEffect(() => {
    if (!runningJob) return;

    if (runningJob.status === "Running" || runningJob.status === "Queued" || runningJob.status === "WaitingForResources" || runningJob.status === "Canceling") {
      const progress = runningJob.currentFile ? ` (${runningJob.currentFile})` : "";
      setStatusText(`Applying ${runningJob.completed + runningJob.failed + runningJob.skipped}/${runningJob.total}${progress}`);
      return;
    }

    // The edits have landed on disk, so the cached track layout now describes
    // how the files used to look. Dropping it re-reads them.
    void invalidateTemplate();

    if (runningJob.propEditResult) {
      setPreviewResult(runningJob.propEditResult);
      setStatusText(runningJob.propEditResult.status);
    } else if (runningJob.status === "Failed") {
      setStatusText(runningJob.error || "Apply failed.");
    }

    currentScan.refetch().then((result) => {
      if (result.data) syncFromBackend(result.data);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runningJob?.status, runningJob?.completed, runningJob?.currentFile]);

  function buildRequest(): PropEditPreviewRequest {
    return {
      files,
      selectedPaths: selectedMkvPaths,
      templatePath,
      containerTitleMode: containerMode,
      customContainerTitle: "",
      videoTitleMode: videoMode,
      customVideoTitle: "",
      videoTrackLanguage: videoLanguageEnabled ? videoLanguage.trim() || "und" : null,
      selectedDefaultVideo: defaultVideo,
      audioTracks,
      subtitleTracks,
      selectedDefaultAudio: defaultAudio,
      selectedForcedAudio: forcedAudio,
      selectedDefaultSubtitle: defaultSubtitle,
      selectedForcedSubtitle: forcedSubtitle
    };
  }

  function runPreview() {
    if (mkvFiles.length === 0) {
      setStatusText("Track Properties requires scanned MKV files. MP4 files can be inspected and renamed, but cannot be edited with mkvpropedit.");
      return;
    }

    if (!template) {
      setStatusText("Select and load a scanned MKV template file first.");
      return;
    }

    preview.mutate(buildRequest());
  }

  function runApply() {
    if (!previewResult?.actions.length) {
      setStatusText("Build a preview with planned property edits before applying.");
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
    if (!operation.activeOperation || operation.activeOperation.label !== "Track Properties") return;
    cancelApply.mutate(operation.activeOperation.id);
    setStatusText("Canceling property edit job...");
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

  function updateTrack(type: TrackType, trackNumber: number, patch: Partial<PropEditTrackConfigRow>) {
    const setter = type === "audio" ? setAudioTracks : setSubtitleTracks;
    setter((current) => current.map((track) => track.trackNumber === trackNumber ? { ...track, ...patch } : track));
  }

  function setTrackCustom(type: TrackType, trackNumber: number, value: boolean) {
    const key = getTrackKey(type, trackNumber);
    setCustomTrackKeys((current) => {
      const next = new Set(current);
      if (value) next.add(key);
      else next.delete(key);
      return next;
    });
    if (value) updateTrack(type, trackNumber, { nameFromMetadata: false });
  }

  const audioFlagOptions = ["Keep existing", ...audioTracks.map((track) => track.trackLabel), "None"];
  const subtitleFlagOptions = ["Keep existing", ...subtitleTracks.map((track) => track.trackLabel), "None"];
  const audioPresetOptions = settings.data?.audioNamePresets?.length ? settings.data.audioNamePresets : audioNamePresets;
  const subtitlePresetOptions = settings.data?.subtitleNamePresets?.length ? settings.data.subtitleNamePresets : subtitleNamePresets;
  const languagePresetOptions = settings.data?.languagePresets?.length ? settings.data.languagePresets : languagePresets;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Track Properties" description="Edit container, track title, language, default, and forced flags." />
      <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[18.75rem_minmax(0,1fr)] gap-3">
        <section className="min-h-0 overflow-x-hidden overflow-y-auto rounded-lg border border-border bg-card p-3 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
            <div className="flex items-center justify-between">
              <h2 className="text-base font-semibold">Properties Configuration</h2>
              <button onClick={refreshFiles} className="h-9 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text">Refresh</button>
            </div>
            <p className="mt-3 text-xs leading-5 text-muted">Configure container title, video track name, and property edit behavior.</p>

            <label htmlFor="track-properties-template" className="mt-3 block text-xs font-semibold text-muted">Template File</label>
            <select id="track-properties-template" value={templatePath} onChange={(event) => {
              const nextTemplate = event.target.value;
              setTemplatePath(nextTemplate);
              setTemplateFilePath(nextTemplate);
            }} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent">
              {mkvFiles.length === 0 ? <option value="">No MKV files scanned</option> : mkvFiles.map((file) => <option key={file.path} value={file.path}>{file.fileName}</option>)}
            </select>
            <div className="mt-1 text-[0.6875rem] text-muted">Uses template track order; validates before editing.</div>
            {nonMkvCount > 0 ? (
              <div className="mt-2 rounded-md border border-warning bg-input p-2 text-xs leading-5 text-warning">
                {nonMkvCount} non-MKV file(s) are excluded. Track Properties uses mkvpropedit and supports MKV files only.
              </div>
            ) : null}

            <TitleModeGroup
              title="Container Title"
              value={containerMode}
              onChange={setContainerMode}
              labels={{ remove: "Remove title", keep: "Keep existing title", file: "Use file name", episode_title: "Use episode title" }}
            />
            <TitleModeGroup
              title="Video Track Name"
              value={videoMode}
              onChange={setVideoMode}
              labels={{ remove: "Remove video name", keep: "Keep existing name", file: "Use file name", episode_title: "Use episode title" }}
            />

            <FlagSelect
              label="Set video default flag"
              value={defaultVideo}
              onChange={setDefaultVideo}
              options={["Keep existing", "Default", "None"]}
            />

            <div className="mt-3 text-xs font-semibold text-muted">Video Track Language</div>
            <label className="mt-2 flex items-center gap-2 text-sm text-text">
              <input
                type="checkbox"
                checked={videoLanguageEnabled}
                onChange={(event) => setVideoLanguageEnabled(event.target.checked)}
              />
              Set video track language
            </label>
            <select
              value={videoLanguage}
              disabled={!videoLanguageEnabled}
              onChange={(event) => setVideoLanguage(event.target.value)}
              className="mt-2 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent disabled:text-disabled"
            >
              {[...new Set(["und", ...languagePresetOptions])].map((language) => (
                <option key={language} value={language}>{language}</option>
              ))}
            </select>

            <div className="mt-3 text-xs font-semibold text-muted">Execution</div>
            <div className="mt-2 flex gap-2">
              <button onClick={runPreview} disabled={preview.isPending || selectedMkvPaths.length === 0 || !template} className="inline-flex h-9 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
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
            <div className="mt-2 text-xs text-muted">All {selectedMkvPaths.length} MKV file(s) are included in this batch.</div>
            <div className="mt-3 line-clamp-2 text-sm text-success">{statusText}</div>
        </section>

        <div className="min-h-0 min-w-0">
          <section className="flex h-full min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
            <div className="flex shrink-0 items-center justify-between gap-3">
              <h2 className="text-base font-semibold">Track Properties</h2>
              <button
                type="button"
                onClick={() => setIsSummaryExpanded(true)}
                className="inline-flex h-9 min-w-32 items-center justify-center whitespace-nowrap rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
              >
                Preview Summary
              </button>
            </div>
            <div className="mt-4 grid min-h-0 flex-1 auto-rows-[minmax(16.25rem,1fr)] gap-3 overflow-y-auto pr-1">
              <TrackEditor
                  title="Audio Tracks"
                  rows={audioTracks}
                  type="audio"
                  defaultValue={defaultAudio}
                  onDefaultChange={setDefaultAudio}
                  forcedValue={forcedAudio}
                  onForcedChange={setForcedAudio}
                  flagOptions={audioFlagOptions}
                  namePresets={audioPresetOptions}
                  languagePresets={languagePresetOptions}
                  customTrackKeys={customTrackKeys}
                  onCustomChange={setTrackCustom}
                  onChange={updateTrack}
                />
                <TrackEditor
                  title="Subtitle Tracks"
                  rows={subtitleTracks}
                  type="subtitle"
                  defaultValue={defaultSubtitle}
                  onDefaultChange={setDefaultSubtitle}
                  forcedValue={forcedSubtitle}
                  onForcedChange={setForcedSubtitle}
                  flagOptions={subtitleFlagOptions}
                  namePresets={subtitlePresetOptions}
                  languagePresets={languagePresetOptions}
                  customTrackKeys={customTrackKeys}
                  onCustomChange={setTrackCustom}
                  onChange={updateTrack}
                />
            </div>
          </section>
        </div>
      </div>
      {isSummaryExpanded ? (
        <PreviewSummaryModal
          title="Track Properties Preview Summary"
          emptyText="Build a preview to see planned property edits."
          available={previewResult !== null}
          status={previewResult?.status ?? ""}
          summary={previewResult?.summary ?? ""}
          metrics={[
            { label: "Files changing", value: new Set(previewResult?.actions.map((action) => action.filePath) ?? []).size, tone: "text-success" },
            { label: "Planned edits", value: previewResult?.actions.length ?? 0, tone: "text-accent" },
            { label: "No change", value: previewResult?.noChange.length ?? 0, tone: "text-muted" },
            { label: "Skipped", value: previewResult?.skipped.length ?? 0, tone: "text-warning" }
          ]}
          sections={[
            {
              title: "Planned changes",
              emptyText: "No property changes are needed.",
              rows: previewResult?.actions.map((action) => ({ key: `${action.filePath}-${action.index}`, title: action.fileName, detail: action.description })) ?? []
            },
            {
              title: "No change",
              emptyText: "Every selected file requires a change.",
              rows: previewResult?.noChange.map((row) => ({ key: row.filePath, title: row.fileName, detail: row.reason })) ?? []
            },
            {
              title: "Skipped",
              emptyText: "No files were skipped.",
              rows: previewResult?.skipped.map((row) => ({ key: row.filePath, title: row.fileName, detail: row.reason })) ?? []
            }
          ]}
          onClose={() => setIsSummaryExpanded(false)}
        />
      ) : null}
    </div>
  );
}

function TitleModeGroup({ title, value, onChange, labels }: {
  title: string;
  value: TitleMode;
  onChange: (value: TitleMode) => void;
  labels: Record<TitleMode, string>;
}) {
  return (
    <div className="mt-3">
      <div className="text-sm font-semibold">{title}</div>
      <div className="mt-2 space-y-1.5 text-sm">
        {(["remove", "keep", "file", "episode_title"] as TitleMode[]).map((mode) => (
          <label key={mode} className="flex h-7 items-center gap-2 px-2">
            <input type="radio" checked={value === mode} onChange={() => onChange(mode)} />
            {labels[mode]}
          </label>
        ))}
      </div>
    </div>
  );
}

function FlagSelect({ label, value, onChange, options }: { label: string; value: string; onChange: (value: string) => void; options: string[] }) {
  return (
    <label className="flex items-center gap-2">
      <span className="text-sm">{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)} className="h-9 w-44 rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent">
        {options.map((option) => <option key={option} value={option}>{option}</option>)}
      </select>
    </label>
  );
}

function TrackEditor({ title, rows, type, defaultValue, onDefaultChange, forcedValue, onForcedChange, flagOptions, namePresets, languagePresets, customTrackKeys, onCustomChange, onChange }: {
  title: string;
  rows: PropEditTrackConfigRow[];
  type: TrackType;
  defaultValue: string;
  onDefaultChange: (value: string) => void;
  forcedValue: string;
  onForcedChange: (value: string) => void;
  flagOptions: string[];
  namePresets: string[];
  languagePresets: string[];
  customTrackKeys: Set<string>;
  onCustomChange: (type: TrackType, trackNumber: number, value: boolean) => void;
  onChange: (type: TrackType, trackNumber: number, patch: Partial<PropEditTrackConfigRow>) => void;
}) {
  return (
    <section className="flex min-h-[16.25rem] min-w-0 flex-col rounded-lg border border-border bg-panel p-3">
      <h3 className="text-base font-semibold">{title}</h3>
      <div className="mt-2 flex shrink-0 flex-wrap gap-3">
        <FlagSelect label="Set default track" value={defaultValue} onChange={onDefaultChange} options={flagOptions} />
        <FlagSelect label="Set forced track" value={forcedValue} onChange={onForcedChange} options={flagOptions} />
      </div>

      <div className="mt-2 min-h-[8.75rem] min-w-0 flex-1 overflow-auto rounded-md border border-border bg-card">
        {rows.length === 0 ? (
          <div className="flex h-full min-h-[7.5rem] items-center justify-center px-4 text-center text-sm text-subtle">
            No embedded {type} tracks were found in the selected template file.
          </div>
        ) : (
          <table className="w-full min-w-[51.25rem] table-fixed border-collapse text-left text-sm">
            <thead className="sticky top-0 bg-panel text-xs text-text">
              <tr>
                <th className="w-24 border-b border-border px-3 py-2">Track</th>
                <th className="w-20 border-b border-border px-3 py-2">Custom</th>
                <th className="w-[16.25rem] border-b border-border px-3 py-2">Name</th>
                <th className="w-32 border-b border-border px-3 py-2">Language</th>
                <th className="border-b border-border px-3 py-2">Current</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((track) => {
                const isCustom = customTrackKeys.has(getTrackKey(type, track.trackNumber));
                const channelNameOption = type === "audio" && track.currentChannels != null
                  ? descriptiveChannelName(track.currentChannels)
                  : "";
                const nameOptions = buildTrackOptions(namePresets, track.editedName, track.currentName, channelNameOption);
                const languageOptions = buildTrackOptions(languagePresets, track.editedLanguage, track.currentLanguage);
                const metadataName = buildMetadataTrackName(track);
                const selectedName = track.nameFromMetadata ? metadataTrackNameValue : track.editedName;

                return (
                  <tr key={`${type}-${track.trackNumber}`} className="bg-card hover:bg-selected">
                    <td className="border-b border-border px-3 py-2 font-semibold">{track.trackLabel}</td>
                    <td className="border-b border-border px-3 py-2">
                      <input type="checkbox" checked={isCustom} onChange={(event) => onCustomChange(type, track.trackNumber, event.target.checked)} />
                    </td>
                    <td className="border-b border-border px-3 py-2">
                      {isCustom ? (
                        <input
                          value={track.editedName}
                          onChange={(event) => onChange(type, track.trackNumber, { editedName: event.target.value, nameFromMetadata: false })}
                          placeholder="Type custom name"
                          className="h-8 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                        />
                      ) : (
                        <select
                          value={selectedName}
                          onChange={(event) => onChange(
                            type,
                            track.trackNumber,
                            event.target.value === metadataTrackNameValue
                              ? { nameFromMetadata: true }
                              : { editedName: event.target.value, nameFromMetadata: false }
                          )}
                          className="h-8 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                        >
                          {type === "audio" && <option value={metadataTrackNameValue}>{metadataName}</option>}
                          {nameOptions.map((option) => <option key={option} value={option}>{option}</option>)}
                        </select>
                      )}
                    </td>
                    <td className="border-b border-border px-3 py-2">
                      <select
                        value={track.editedLanguage}
                        onChange={(event) => onChange(type, track.trackNumber, { editedLanguage: event.target.value })}
                        className="h-8 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                      >
                        {languageOptions.map((option) => <option key={option} value={option}>{option}</option>)}
                      </select>
                    </td>
                    <td className="truncate border-b border-border px-3 py-2 text-muted" title={buildCurrentTrackSummary(track)}>
                      {buildCurrentTrackSummary(track)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}

function getTrackKey(type: TrackType, trackNumber: number) {
  return `${type}:${trackNumber}`;
}

function buildTrackOptions(configuredValues: string[], ...priorityValues: string[]) {
  const seen = new Set<string>();
  const values = [...priorityValues, ...configuredValues]
    .map((value) => value.trim())
    .filter((value) => {
      if (!value || seen.has(value.toLowerCase())) return false;
      seen.add(value.toLowerCase());
      return true;
    });
  const hasBlank = [...priorityValues, ...configuredValues].some((value) => !value.trim());
  return hasBlank ? ["", ...values] : values;
}

function buildCurrentTrackSummary(track: PropEditTrackConfigRow) {
  const parts = [
    track.currentLanguage || "und",
    track.currentName || "No name"
  ];
  if (track.currentDefault) parts.push("default");
  return parts.join(" | ");
}

function buildMetadataTrackName(track: PropEditTrackConfigRow) {
  const language = languageDisplayName(track.editedLanguage || track.currentLanguage || "und");
  const codec = codecDisplayName(track.currentCodec);
  return [
    codec,
    language,
    track.currentChannels == null ? "" : channelDisplayName(track.currentChannels)
  ].filter(Boolean).join(" ");
}

function channelDisplayName(channels: number) {
  if (channels === 1) return "1.0";
  if (channels === 2) return "2.0";
  if (channels === 6) return "5.1";
  if (channels === 8) return "7.1";
  return `${channels}.0`;
}

function descriptiveChannelName(channels: number) {
  if (channels === 1) return "1.0 Mono";
  if (channels === 2) return "2.0 Stereo";
  if (channels === 6) return "5.1 Surround";
  if (channels === 8) return "7.1 Surround";
  return channelDisplayName(channels);
}

function codecDisplayName(codec: string) {
  const normalized = codec.trim().toLowerCase();
  if (normalized === "aac") return "AAC";
  if (normalized === "ac-3" || normalized === "ac3") return "AC-3";
  if (normalized === "e-ac-3" || normalized === "eac3") return "E-AC-3";
  if (normalized === "dts") return "DTS";
  if (normalized === "truehd") return "TrueHD";
  if (normalized === "opus") return "Opus";
  if (normalized === "flac") return "FLAC";
  return codec.trim();
}

function languageDisplayName(language: string) {
  const normalized = language.trim().toLowerCase();
  const names: Record<string, string> = {
    eng: "English", en: "English", jpn: "Japanese", ja: "Japanese",
    spa: "Spanish", es: "Spanish", fra: "French", fre: "French", fr: "French",
    deu: "German", ger: "German", de: "German", ita: "Italian", it: "Italian",
    por: "Portuguese", pt: "Portuguese", kor: "Korean", ko: "Korean",
    zho: "Chinese", chi: "Chinese", zh: "Chinese", rus: "Russian", ru: "Russian",
    ara: "Arabic", ar: "Arabic", hin: "Hindi", hi: "Hindi", nld: "Dutch",
    dut: "Dutch", nl: "Dutch", pol: "Polish", pl: "Polish", tur: "Turkish",
    tr: "Turkish", und: "Undetermined"
  };
  return names[normalized] ?? language.trim();
}
