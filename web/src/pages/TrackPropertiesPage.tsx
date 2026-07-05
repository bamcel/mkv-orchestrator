import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, Wand2 } from "lucide-react";
import {
  applyPropEditPlan,
  buildPropEditPreview,
  getCurrentScanFiles,
  loadPropEditTemplate,
  PropEditPreviewRequest,
  PropEditPreviewResponse,
  PropEditTemplateResponse,
  PropEditTrackConfigRow
} from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

type TitleMode = "keep" | "remove" | "file" | "custom";
type TrackType = "audio" | "subtitle";

const audioNamePresets = ["English", "Japanese", "Commentary", "Director Commentary", "Signs & Songs"];
const subtitleNamePresets = ["English", "English Forced", "English SDH", "Signs & Songs", "Commentary"];
const languagePresets = ["eng", "jpn", "spa", "fre", "ger", "und", "en", "ja", "es", "fr", "de"];

export function TrackPropertiesPage() {
  const { files, setFiles, templateFilePath, setTemplateFilePath } = useMediaLibrary();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [templatePath, setTemplatePath] = useState("");
  const [containerMode, setContainerMode] = useState<TitleMode>("keep");
  const [videoMode, setVideoMode] = useState<TitleMode>("keep");
  const [customContainerTitle, setCustomContainerTitle] = useState("");
  const [customVideoTitle, setCustomVideoTitle] = useState("");
  const [template, setTemplate] = useState<PropEditTemplateResponse | null>(null);
  const [audioTracks, setAudioTracks] = useState<PropEditTrackConfigRow[]>([]);
  const [subtitleTracks, setSubtitleTracks] = useState<PropEditTrackConfigRow[]>([]);
  const [defaultAudio, setDefaultAudio] = useState("Keep existing");
  const [forcedAudio, setForcedAudio] = useState("Keep existing");
  const [defaultSubtitle, setDefaultSubtitle] = useState("Keep existing");
  const [forcedSubtitle, setForcedSubtitle] = useState("Keep existing");
  const [customTrackKeys, setCustomTrackKeys] = useState<Set<string>>(new Set());
  const [previewResult, setPreviewResult] = useState<PropEditPreviewResponse | null>(null);
  const [statusText, setStatusText] = useState("Load scanned files from Dashboard, then select a template.");

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
  }, [currentScan.data, files.length, setFiles]);

  useEffect(() => {
    setSelectedPaths((current) => {
      if (files.length === 0) return [];
      const mkvPaths = files.filter((file) => file.extension.toLowerCase() === ".mkv").map((file) => file.path);
      const existing = current.filter((path) => mkvPaths.includes(path));
      return existing.length > 0 ? existing : mkvPaths;
    });

    if (!templatePath) {
      const nextTemplate = templateFilePath || files.find((file) => file.extension.toLowerCase() === ".mkv")?.path || "";
      setTemplatePath(nextTemplate);
    }
  }, [files, templateFilePath, templatePath]);

  const mkvFiles = useMemo(() => files.filter((file) => file.extension.toLowerCase() === ".mkv"), [files]);

  const templateLoad = useMutation({
    mutationFn: loadPropEditTemplate,
    onSuccess: (response) => {
      setTemplate(response);
      setAudioTracks(response.audioTracks);
      setSubtitleTracks(response.subtitleTracks);
      setDefaultAudio(response.defaultAudio || "Keep existing");
      setForcedAudio(response.forcedAudio || "Keep existing");
      setDefaultSubtitle(response.defaultSubtitle || "Keep existing");
      setForcedSubtitle(response.forcedSubtitle || "Keep existing");
      setCustomTrackKeys(new Set());
      setCustomContainerTitle(response.templateFileName.replace(/\.[^.]+$/, ""));
      setCustomVideoTitle(response.templateFileName.replace(/\.[^.]+$/, ""));
      setStatusText(`Template loaded: ${response.templateFileName}`);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Template load failed.")
  });

  useEffect(() => {
    if (files.length === 0 || !templatePath) return;
    templateLoad.mutate({ files, templatePath });
    setTemplateFilePath(templatePath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [templatePath, files.length]);

  const preview = useMutation({
    mutationFn: buildPropEditPreview,
    onSuccess: (response) => {
      setPreviewResult(response);
      setStatusText(response.status);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Preview failed.")
  });

  const apply = useMutation({
    mutationFn: applyPropEditPlan,
    onSuccess: (response) => {
      setPreviewResult(response);
      setStatusText(response.status);
      currentScan.refetch().then((result) => {
        if (result.data?.files.length) setFiles(result.data.files);
      });
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Apply failed.")
  });

  function buildRequest(): PropEditPreviewRequest {
    return {
      files,
      selectedPaths,
      templatePath,
      containerTitleMode: containerMode,
      customContainerTitle,
      videoTitleMode: videoMode,
      customVideoTitle,
      audioTracks,
      subtitleTracks,
      selectedDefaultAudio: defaultAudio,
      selectedForcedAudio: forcedAudio,
      selectedDefaultSubtitle: defaultSubtitle,
      selectedForcedSubtitle: forcedSubtitle
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
  }

  const audioFlagOptions = ["Keep existing", ...audioTracks.map((track) => track.trackLabel), "None"];
  const subtitleFlagOptions = ["Keep existing", ...subtitleTracks.map((track) => track.trackLabel), "None"];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Track Properties" description="Edit container, track title, language, default, and forced flags." />
      <div className="grid min-h-0 flex-1 grid-cols-[370px_1fr] gap-3">
        <section className="min-h-0 overflow-hidden rounded-lg border border-border bg-card p-3 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
            <div className="flex items-center justify-between">
              <h2 className="text-base font-semibold">Properties Configuration</h2>
              <button onClick={refreshFiles} className="rounded-md border border-border bg-button px-2.5 py-1.5 text-xs font-semibold text-muted hover:bg-button-hover hover:text-text">Refresh</button>
            </div>
            <p className="mt-3 text-xs leading-5 text-muted">Configure container title, video track name, and property edit behavior.</p>

            <label className="mt-3 block text-xs font-semibold text-muted">Template File</label>
            <select value={templatePath} onChange={(event) => setTemplatePath(event.target.value)} className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent">
              {mkvFiles.length === 0 ? <option value="">No MKV files scanned</option> : mkvFiles.map((file) => <option key={file.path} value={file.path}>{file.fileName}</option>)}
            </select>
            <div className="mt-1 text-[11px] text-muted">Uses template track order; validates before editing.</div>

            <TitleModeGroup
              title="Container Title"
              value={containerMode}
              onChange={setContainerMode}
              customValue={customContainerTitle}
              onCustomChange={setCustomContainerTitle}
              labels={{ remove: "Remove title", keep: "Keep existing title", file: "Use file name", custom: "Custom title" }}
            />
            <TitleModeGroup
              title="Video Track Name"
              value={videoMode}
              onChange={setVideoMode}
              customValue={customVideoTitle}
              onCustomChange={setCustomVideoTitle}
              labels={{ remove: "Remove video name", keep: "Keep existing name", file: "Use file name", custom: "Custom video name" }}
            />

            <div className="mt-3 text-xs font-semibold text-muted">Execution</div>
            <div className="mt-2 flex gap-2">
              <button onClick={() => preview.mutate(buildRequest())} disabled={preview.isPending || selectedPaths.length === 0 || !template} className="inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
                {preview.isPending ? <RefreshCw size={15} className="animate-spin" /> : <Wand2 size={15} />}
                Preview
              </button>
              <button onClick={() => apply.mutate(buildRequest())} disabled={apply.isPending || selectedPaths.length === 0 || !previewResult?.actions.length} className="h-10 flex-1 rounded-md bg-accent text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
                Apply
              </button>
            </div>
            <div className="mt-3 line-clamp-2 text-sm text-success">{statusText}</div>
        </section>

        <div className="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_190px] gap-3">
          <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
            <h2 className="shrink-0 text-base font-semibold">Track Properties</h2>
            <div className="mt-4 grid min-h-0 flex-1 grid-rows-2 gap-3">
              <TrackEditor
                  title="Audio Tracks"
                  rows={audioTracks}
                  type="audio"
                  defaultValue={defaultAudio}
                  onDefaultChange={setDefaultAudio}
                  flagOptions={audioFlagOptions}
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
                  flagOptions={subtitleFlagOptions}
                  customTrackKeys={customTrackKeys}
                  onCustomChange={setTrackCustom}
                  onChange={updateTrack}
                />
            </div>
          </section>

          <section className="rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-semibold">Preview Summary</h3>
              <button className="h-7 rounded-md bg-button px-3 text-xs font-semibold text-muted">Expand</button>
            </div>
            <pre className="mt-3 h-[125px] overflow-auto whitespace-pre-wrap break-words rounded-md bg-input p-3 font-mono text-xs leading-5 text-muted">
              {previewResult?.summary || "Build a preview to see planned property edits."}
            </pre>
          </section>
        </div>
      </div>
    </div>
  );
}

function TitleModeGroup({ title, value, onChange, customValue, onCustomChange, labels }: {
  title: string;
  value: TitleMode;
  onChange: (value: TitleMode) => void;
  customValue: string;
  onCustomChange: (value: string) => void;
  labels: Record<TitleMode, string>;
}) {
  return (
    <div className="mt-3">
      <div className="text-sm font-semibold">{title}</div>
      <div className="mt-2 space-y-1.5 text-sm">
        {(["remove", "keep", "file", "custom"] as TitleMode[]).map((mode) => (
          <label key={mode} className="flex h-7 items-center gap-2 px-2">
            <input type="radio" checked={value === mode} onChange={() => onChange(mode)} />
            {labels[mode]}
          </label>
        ))}
      </div>
      <input
        value={customValue}
        onChange={(event) => onCustomChange(event.target.value)}
        className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
      />
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

function TrackEditor({ title, rows, type, defaultValue, onDefaultChange, flagOptions, customTrackKeys, onCustomChange, onChange }: {
  title: string;
  rows: PropEditTrackConfigRow[];
  type: TrackType;
  defaultValue: string;
  onDefaultChange: (value: string) => void;
  flagOptions: string[];
  customTrackKeys: Set<string>;
  onCustomChange: (type: TrackType, trackNumber: number, value: boolean) => void;
  onChange: (type: TrackType, trackNumber: number, patch: Partial<PropEditTrackConfigRow>) => void;
}) {
  return (
    <section className="flex min-h-0 flex-col rounded-lg border border-border bg-panel p-3">
      <h3 className="text-base font-semibold">{title}</h3>
      <div className="mt-2 shrink-0">
        <FlagSelect label="Set default track" value={defaultValue} onChange={onDefaultChange} options={flagOptions} />
      </div>

      <div className="mt-2 min-h-0 flex-1 overflow-auto bg-card">
        {rows.length === 0 ? (
          <div className="flex h-full min-h-[120px] items-center justify-center text-sm text-subtle">No tracks available.</div>
        ) : (
          <table className="w-full min-w-[820px] table-fixed border-collapse text-left text-sm">
            <thead className="sticky top-0 bg-panel text-xs text-text">
              <tr>
                <th className="w-24 border-b border-border px-3 py-2">Track</th>
                <th className="w-20 border-b border-border px-3 py-2">Custom</th>
                <th className="w-[260px] border-b border-border px-3 py-2">Name</th>
                <th className="w-32 border-b border-border px-3 py-2">Language</th>
                <th className="border-b border-border px-3 py-2">Current</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((track) => {
                const isCustom = customTrackKeys.has(getTrackKey(type, track.trackNumber));
                const nameOptions = buildTrackOptions(type === "audio" ? audioNamePresets : subtitleNamePresets, track.editedName, track.currentName);
                const languageOptions = buildTrackOptions(languagePresets, track.editedLanguage, track.currentLanguage);

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
                          onChange={(event) => onChange(type, track.trackNumber, { editedName: event.target.value })}
                          placeholder="Type custom name"
                          className="h-8 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                        />
                      ) : (
                        <select
                          value={track.editedName}
                          onChange={(event) => onChange(type, track.trackNumber, { editedName: event.target.value })}
                          className="h-8 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                        >
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
  return [...priorityValues, ...configuredValues]
    .map((value) => value.trim())
    .filter((value) => {
      if (!value || seen.has(value.toLowerCase())) return false;
      seen.add(value.toLowerCase());
      return true;
    });
}

function buildCurrentTrackSummary(track: PropEditTrackConfigRow) {
  const parts = [
    track.currentLanguage || "und",
    track.currentName || "No name"
  ];
  if (track.currentDefault) parts.push("default");
  return parts.join(" | ");
}
