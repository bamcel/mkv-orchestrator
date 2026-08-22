import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { AlertTriangle, CheckCircle2, Film, RefreshCw, Search, X } from "lucide-react";
import {
  buildLibraryAudit,
  cancelScan,
  getLibraryArtwork,
  getLibraryCatalog,
  getScanJob,
  getWebSettings,
  LibraryAuditResponse,
  LibraryAuditRow,
  LibraryCatalogItem,
  MediaFileRow,
  startScan,
  WebSettings
} from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

type LibraryFilter = "all" | "matching" | "mismatch";

export function LibraryPage() {
  const navigate = useNavigate();
  const { setFiles, setSelectedPaths, setTemplateFilePath } = useMediaLibrary();
  const webSettings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const [auditResult, setAuditResult] = useState<LibraryAuditResponse | null>(null);
  const [libraryFiles, setLibraryFiles] = useState<MediaFileRow[]>([]);
  const [selectedTitleId, setSelectedTitleId] = useState("");
  const [selectedSource, setSelectedSource] = useState("");
  const [scanJobId, setScanJobId] = useState<string | null>(null);
  const [pendingOverviewScan, setPendingOverviewScan] = useState(false);
  const [filter, setFilter] = useState<LibraryFilter>("all");
  const [searchText, setSearchText] = useState("");
  const [statusText, setStatusText] = useState("Choose a library and build its overview.");

  const sourceOptions = useMemo(
    () => librarySourceOptions(webSettings.data).filter((root) => root.paths.length > 0),
    [webSettings.data]
  );

  useEffect(() => {
    if (sourceOptions.length === 0) {
      setSelectedSource("");
      return;
    }
    if (!sourceOptions.some((source) => source.id === selectedSource)) {
      setSelectedSource(sourceOptions[0].id);
    }
  }, [selectedSource, sourceOptions]);

  const selectedSourceOption = sourceOptions.find((source) => source.id === selectedSource);
  const catalog = useQuery({
    queryKey: ["library-catalog", selectedSourceOption?.serverId, selectedSourceOption?.libraryName],
    queryFn: () => getLibraryCatalog({
      serverId: selectedSourceOption!.serverId!,
      libraryName: selectedSourceOption!.libraryName!
    }),
    enabled: Boolean(selectedSourceOption?.serverId && selectedSourceOption.libraryName),
    staleTime: 5 * 60 * 1000
  });

  const audit = useMutation({
    mutationFn: buildLibraryAudit,
    onSuccess: (response) => {
      setAuditResult(response);
      setStatusText(`Library ready: ${response.summary.groups} season/folder groups, ${response.summary.files} files, ${response.summary.issueGroups} warning groups.`);
    },
    onError: (error) => {
      setPendingOverviewScan(false);
      setStatusText(error instanceof Error ? error.message : "Library audit failed.");
    }
  });

  const scanStart = useMutation({
    mutationFn: startScan,
    onSuccess: (job) => {
      setScanJobId(job.id);
      setStatusText("Scanning the selected library...");
    },
    onError: (error) => {
      setPendingOverviewScan(false);
      setStatusText(error instanceof Error ? error.message : "Library scan failed to start.");
    }
  });
  const scanCancel = useMutation({
    mutationFn: cancelScan,
    onSuccess: () => setStatusText("Cancel requested for the library scan."),
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Cancel failed.")
  });
  const scanJob = useQuery({
    queryKey: ["library-cache-job", scanJobId],
    queryFn: () => getScanJob(scanJobId!),
    enabled: scanJobId !== null,
    refetchInterval: (query) => {
      const job = query.state.data;
      return job && ["Completed", "Failed", "Skipped", "Canceled"].includes(job.status) ? false : 1000;
    }
  });

  const currentScanJob = scanJob.data;
  const isBusy = scanStart.isPending
    || audit.isPending
    || currentScanJob?.status === "Queued"
    || currentScanJob?.status === "WaitingForResources"
    || currentScanJob?.status === "Running"
    || currentScanJob?.status === "Canceling";
  const progressText = currentScanJob?.total ? `${currentScanJob.completed}/${currentScanJob.total} files` : "preparing";

  useEffect(() => {
    if (!currentScanJob) return;
    if (["Queued", "WaitingForResources", "Running", "Canceling"].includes(currentScanJob.status)) {
      setStatusText(`Scanning library: ${progressText}`);
      return;
    }
    if (currentScanJob.status === "Completed" && pendingOverviewScan) {
      setPendingOverviewScan(false);
      setLibraryFiles(currentScanJob.files);
      audit.mutate(currentScanJob.files);
    } else if (currentScanJob.status === "Failed") {
      setPendingOverviewScan(false);
      setStatusText(currentScanJob.error || "Library scan failed.");
    } else if (currentScanJob.status === "Canceled") {
      setPendingOverviewScan(false);
      setStatusText("Library scan canceled.");
    }
  }, [currentScanJob, pendingOverviewScan]);

  const titles = useMemo(
    () => buildLibraryTitles(auditResult?.items ?? [], catalog.data?.items ?? []),
    [auditResult?.items, catalog.data?.items]
  );
  const displayedTitles = useMemo(() => {
    const query = searchText.trim().toLocaleLowerCase();
    return titles.filter((title) => {
      if (filter === "matching" && title.hasIssues) return false;
      if (filter === "mismatch" && !title.hasIssues) return false;
      return !query || title.title.toLocaleLowerCase().includes(query);
    });
  }, [filter, searchText, titles]);
  const selectedTitle = titles.find((title) => title.id === selectedTitleId) ?? null;

  function selectSource(id: string) {
    setSelectedSource(id);
    setAuditResult(null);
    setLibraryFiles([]);
    setSelectedTitleId("");
    setFilter("all");
    setSearchText("");
    setStatusText("Build the selected library to inspect its metadata health.");
  }

  function runBuildOverview(forceRefresh: boolean) {
    if (!selectedSourceOption) {
      setStatusText("Select a library first.");
      return;
    }
    setAuditResult(null);
    setLibraryFiles([]);
    setSelectedTitleId("");
    setPendingOverviewScan(true);
    scanStart.mutate({
      sources: selectedSourceOption.paths,
      ignoredFolderNames: webSettings.data?.ignoredScanFolderNames ?? [],
      forceRefresh
    });
  }

  function handoffToDashboard(title: LibraryTitle, mode: "mismatch" | "all") {
    const paths = mode === "all"
      ? title.allFilePaths
      : uniquePaths(title.seasons.flatMap((season) => season.hasIssues ? [season.templateFilePath, ...season.issueFilePaths] : []));
    const wanted = new Set(paths.map(normalizePath));
    const selectedFiles = libraryFiles.filter((file) => wanted.has(normalizePath(file.path)));
    if (selectedFiles.length === 0) {
      setStatusText("The selected title has no scanned files available for Dashboard.");
      return;
    }
    const templatePath = title.seasons.find((season) => season.hasIssues)?.templateFilePath
      || title.seasons[0]?.templateFilePath
      || selectedFiles[0].path;
    selectedFiles.sort((left, right) => {
      if (normalizePath(left.path) === normalizePath(templatePath)) return -1;
      if (normalizePath(right.path) === normalizePath(templatePath)) return 1;
      return left.path.localeCompare(right.path);
    });
    setFiles(selectedFiles);
    setSelectedPaths(selectedFiles.map((file) => file.path));
    setTemplateFilePath(templatePath);
    navigate("/dashboard");
  }

  const mismatchCount = titles.filter((title) => title.hasIssues).length;
  const matchingCount = titles.length - mismatchCount;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Library" description="Browse your media as posters, review metadata health, and send repair batches to Dashboard." />

      <section className="mt-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border bg-card shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
        <div className="shrink-0 border-b border-border px-5 pt-4">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0">
              <h2 className="text-lg font-semibold">Media Library</h2>
              <p className="mt-1 truncate text-xs text-subtle" title={selectedSourceOption?.paths.join(" · ")}>
                {selectedSourceOption ? `${selectedSourceOption.paths.length} source path${selectedSourceOption.paths.length === 1 ? "" : "s"}` : "No library sources configured"}
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <label className="relative block">
                <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-subtle" />
                <input value={searchText} onChange={(event) => setSearchText(event.target.value)} placeholder="Filter titles..." aria-label="Filter library titles" className="h-9 w-56 rounded-md border border-border bg-input pl-9 pr-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent" />
              </label>
              <button type="button" onClick={() => runBuildOverview(false)} disabled={isBusy || !selectedSourceOption} className="h-9 rounded-md bg-accent px-4 text-sm font-semibold text-window disabled:bg-button disabled:text-disabled">{auditResult ? "Rebuild" : "Build Library"}</button>
              <button type="button" onClick={() => runBuildOverview(true)} disabled={isBusy || !selectedSourceOption} className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled"><RefreshCw size={14} className={isBusy ? "animate-spin" : ""} /> Refresh</button>
              {isBusy ? <button type="button" onClick={() => scanJobId && scanCancel.mutate(scanJobId)} disabled={scanCancel.isPending} className="h-9 rounded-md border border-warning px-3 text-sm font-semibold text-warning">Cancel</button> : null}
            </div>
          </div>

          <div className="mt-4 flex items-end justify-between gap-4">
            <div className="flex min-w-0 gap-1 overflow-x-auto">
              {sourceOptions.map((source) => (
                <button key={source.id} type="button" onClick={() => selectSource(source.id)} className={`whitespace-nowrap border-b-2 px-3 py-2 text-sm font-semibold transition ${selectedSource === source.id ? "border-accent text-text" : "border-transparent text-muted hover:text-text"}`}>{source.name}</button>
              ))}
            </div>
            <div className="flex shrink-0 gap-2 pb-2 text-xs">
              <StatusCount label="Titles" value={titles.length} tone="text-text" />
              <StatusCount label="Match" value={matchingCount} tone="text-success" />
              <StatusCount label="Mismatch" value={mismatchCount} tone="text-warning" />
            </div>
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-5 py-3">
          <div className="flex gap-2">
            <FilterButton active={filter === "all"} onClick={() => setFilter("all")}>All</FilterButton>
            <FilterButton active={filter === "matching"} onClick={() => setFilter("matching")}>Matching</FilterButton>
            <FilterButton active={filter === "mismatch"} onClick={() => setFilter("mismatch")}>Mismatches</FilterButton>
          </div>
          <div className={`truncate text-xs ${isBusy ? "text-accent" : "text-subtle"}`} title={statusText}>{statusText}</div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          {isBusy && !auditResult ? (
            <div className="grid h-full min-h-72 place-items-center text-sm text-muted"><div className="text-center"><RefreshCw size={28} className="mx-auto mb-3 animate-spin text-accent" />Scanning {progressText}</div></div>
          ) : displayedTitles.length > 0 ? (
            <div className="grid gap-x-5 gap-y-6 [grid-template-columns:repeat(auto-fill,minmax(9.375rem,1fr))]">
              {displayedTitles.map((title) => <LibraryPosterCard key={title.id} title={title} serverId={selectedSourceOption?.serverId} onOpen={() => setSelectedTitleId(title.id)} />)}
            </div>
          ) : (
            <div className="grid h-full min-h-72 place-items-center rounded-lg border border-dashed border-border bg-panel/40 px-6 text-center text-sm text-subtle">
              {auditResult ? "No titles match the current filter." : selectedSourceOption ? "Build this library to create its poster overview." : "Configure a watch folder or media-server library in Settings."}
            </div>
          )}
        </div>
      </section>

      {selectedTitle ? <LibraryTitleDialog title={selectedTitle} serverId={selectedSourceOption?.serverId} onClose={() => setSelectedTitleId("")} onSendMismatches={() => handoffToDashboard(selectedTitle, "mismatch")} onSendAll={() => handoffToDashboard(selectedTitle, "all")} /> : null}
    </div>
  );
}

function LibraryPosterCard({ title, serverId, onOpen }: { title: LibraryTitle; serverId?: string; onOpen: () => void }) {
  const artwork = useQuery({
    queryKey: ["library-artwork", serverId, title.catalogItem?.id],
    queryFn: () => getLibraryArtwork({ serverId: serverId!, itemId: title.catalogItem!.id }),
    enabled: Boolean(serverId && title.catalogItem?.hasPoster),
    staleTime: Number.POSITIVE_INFINITY
  });
  const imageUrl = artwork.data ? `data:${artwork.data.contentType};base64,${artwork.data.dataBase64}` : "";
  return (
    <button type="button" onClick={onOpen} className="group min-w-0 select-none text-left" title={title.title} aria-label={`Open ${title.title} library details`}>
      <div className="relative aspect-[2/3] overflow-hidden rounded-xl bg-[#64748B] ring-1 ring-border transition duration-150 group-hover:-translate-y-1 group-hover:ring-2 group-hover:ring-accent group-focus-visible:ring-2 group-focus-visible:ring-accent">
        {imageUrl ? <img src={imageUrl} alt={`${title.title} poster`} loading="lazy" className="h-full w-full object-cover" /> : <div className="grid h-full place-items-center bg-[#64748B] px-4 text-center text-xs font-bold uppercase tracking-wider text-white"><div><Film size={34} className="mx-auto mb-3 opacity-80" />No poster found</div></div>}
        <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 via-black/40 to-transparent px-2 pb-2 pt-10">
          <span className={`inline-flex rounded-md px-2 py-1 text-[0.6875rem] font-semibold uppercase tracking-wide ${title.hasIssues ? "bg-warning/20 text-warning" : "bg-success/20 text-success"}`}>{title.hasIssues ? `Mismatch · ${title.mismatchFileCount}` : "Match"}</span>
        </div>
      </div>
      <div className="mt-2 px-0.5"><div className="truncate text-sm font-semibold text-text">{title.title}</div><div className="mt-0.5 truncate text-xs text-subtle">{title.catalogItem?.year ? `${title.catalogItem.year} · ` : ""}{title.seasons.length} season{title.seasons.length === 1 ? "" : "s"} · {title.fileCount} files</div></div>
    </button>
  );
}

function LibraryTitleDialog({ title, serverId, onClose, onSendMismatches, onSendAll }: { title: LibraryTitle; serverId?: string; onClose: () => void; onSendMismatches: () => void; onSendAll: () => void }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-6" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section role="dialog" aria-modal="true" aria-label={`${title.title} library details`} className="flex max-h-[88vh] w-full max-w-5xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl">
        <header className="flex shrink-0 items-start justify-between gap-4 border-b border-border px-5 py-4">
          <div className="min-w-0"><div className="flex items-center gap-2"><h2 className="truncate text-xl font-semibold">{title.title}</h2><span className={`shrink-0 rounded-md px-2 py-1 text-[0.6875rem] font-semibold uppercase tracking-wide ${title.hasIssues ? "bg-warning/20 text-warning" : "bg-success/20 text-success"}`}>{title.hasIssues ? "Mismatch" : "Match"}</span></div><p className="mt-1 text-sm text-muted">{title.seasons.length} season{title.seasons.length === 1 ? "" : "s"} · {title.fileCount} files · {title.mismatchFileCount} mismatched{serverId && title.catalogItem?.hasPoster ? " · Media-server artwork" : ""}</p></div>
          <button type="button" onClick={onClose} aria-label="Close library details" className="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted hover:bg-button-hover hover:text-text"><X size={17} /></button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          <div className="grid grid-cols-3 gap-3"><SummaryCard label="Seasons" value={title.seasons.length} /><SummaryCard label="Files" value={title.fileCount} /><SummaryCard label="Mismatch files" value={title.mismatchFileCount} tone={title.hasIssues ? "text-warning" : "text-success"} /></div>
          <div className="mt-5 space-y-3">
            {title.seasons.map((season) => (
              <section key={season.folderPath} className="rounded-lg border border-border bg-panel p-4">
                <div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2 font-semibold">{season.hasIssues ? <AlertTriangle size={15} className="text-warning" /> : <CheckCircle2 size={15} className="text-success" />}{seasonLabel(season)}</div><span className={`rounded-md px-2 py-1 text-[0.6875rem] font-semibold uppercase tracking-wide ${season.hasIssues ? "bg-warning/20 text-warning" : "bg-success/20 text-success"}`}>{season.hasIssues ? `${season.issueFilePaths.length} mismatch` : "Match"}</span></div>
                <div className="mt-3 grid gap-2 text-xs md:grid-cols-3"><ProfileValue label="Video" value={season.standardVideo} /><ProfileValue label="Audio" value={season.standardAudio} /><ProfileValue label="Subtitles" value={season.standardSubtitles} /></div>
                <div className="mt-3 text-xs text-subtle">Template: <span className="text-template">{season.templateFileName}</span></div>
                {season.issues.length ? <ul className="mt-3 space-y-1.5 text-sm text-warning">{season.issues.map((issue, index) => <li key={`${issue}-${index}`}>• {issue}</li>)}</ul> : <div className="mt-3 text-sm text-success">All scanned files match this season’s metadata profile.</div>}
              </section>
            ))}
          </div>
        </div>
        <footer className="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t border-border px-5 py-4"><button type="button" onClick={onClose} className="h-9 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted hover:bg-button-hover hover:text-text">Close</button><button type="button" onClick={onSendMismatches} disabled={!title.hasIssues} className="h-9 rounded-md border border-warning bg-button px-4 text-sm font-semibold text-warning hover:bg-button-hover disabled:border-border disabled:text-disabled">Send mismatches + template to Dashboard</button><button type="button" onClick={onSendAll} className="h-9 rounded-md bg-accent px-4 text-sm font-semibold text-window">Send all files to Dashboard</button></footer>
      </section>
    </div>
  );
}

function FilterButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return <button type="button" onClick={onClick} className={`h-8 rounded-md border px-3 text-xs font-semibold ${active ? "border-accent bg-selected text-text" : "border-border bg-button text-muted hover:text-text"}`}>{children}</button>;
}
function StatusCount({ label, value, tone }: { label: string; value: number; tone: string }) {
  return <span className="rounded-md border border-border bg-panel px-2 py-1 text-subtle">{label} <strong className={tone}>{value}</strong></span>;
}
function SummaryCard({ label, value, tone = "text-text" }: { label: string; value: number; tone?: string }) {
  return <div className="rounded-lg border border-border bg-panel p-3"><div className="text-xs text-subtle">{label}</div><div className={`mt-1 text-2xl font-semibold ${tone}`}>{value}</div></div>;
}
function ProfileValue({ label, value }: { label: string; value: string }) {
  return <div className="rounded-md bg-input p-2"><div className="font-semibold uppercase tracking-wide text-subtle">{label}</div><div className="mt-1 break-words text-muted">{value || "Unknown"}</div></div>;
}

export type LibrarySourceOption = { id: string; name: string; paths: string[]; serverId?: string; libraryName?: string; mediaType?: string };

export function librarySourceOptions(settings: WebSettings | undefined): LibrarySourceOption[] {
  if (!settings) return [];
  const roots: LibrarySourceOption[] = settings.watchFolders.map((path) => ({ id: `watch:${normalizePath(path)}`, name: folderName(path) || "Watch Folder", paths: [path] })).filter((root) => root.paths[0].trim());
  const grouped = new Map<string, LibrarySourceOption>();
  for (const server of settings.mediaServers) {
    for (const library of server.libraries) {
      if (!library.isEnabled || !library.containerPath.trim()) continue;
      const groupKey = `${server.id}:${library.name.trim().toLowerCase()}`;
      const current = grouped.get(groupKey);
      if (current) {
        if (!current.paths.some((path) => normalizePath(path) === normalizePath(library.containerPath))) current.paths.push(library.containerPath);
      } else {
        grouped.set(groupKey, { id: `media:${groupKey}`, name: `${server.name} — ${library.name}`, paths: [library.containerPath], serverId: server.id, libraryName: library.name, mediaType: library.type });
      }
    }
  }
  return [...roots, ...grouped.values()];
}

export type LibraryTitle = { id: string; title: string; catalogItem?: LibraryCatalogItem; seasons: LibraryAuditRow[]; fileCount: number; mismatchFileCount: number; hasIssues: boolean; allFilePaths: string[] };

export function buildLibraryTitles(rows: LibraryAuditRow[], catalogItems: LibraryCatalogItem[]): LibraryTitle[] {
  const catalogByTitle = new Map(catalogItems.map((item) => [titleKey(item.title), item]));
  const grouped = new Map<string, LibraryTitle>();
  for (const row of rows) {
    const title = auditTitle(row);
    const key = titleKey(title);
    const current = grouped.get(key) ?? { id: key || normalizePath(row.folderPath), title, catalogItem: catalogByTitle.get(key), seasons: [], fileCount: 0, mismatchFileCount: 0, hasIssues: false, allFilePaths: [] };
    current.seasons.push(row);
    current.fileCount += row.fileCount;
    current.mismatchFileCount += row.issueFilePaths.length;
    current.hasIssues ||= row.hasIssues;
    current.allFilePaths = uniquePaths([...current.allFilePaths, ...row.allFilePaths]);
    grouped.set(key, current);
  }
  return [...grouped.values()].map((title) => ({ ...title, seasons: [...title.seasons].sort((left, right) => seasonLabel(left).localeCompare(seasonLabel(right), undefined, { numeric: true })) })).sort((left, right) => left.title.localeCompare(right.title, undefined, { numeric: true }));
}

function auditTitle(row: LibraryAuditRow) {
  const label = row.folderName.split(" / ")[0]?.trim();
  if (label) return label;
  const folder = folderName(row.folderPath);
  return /^season\s*\d+/i.test(folder) ? folderName(parentPath(row.folderPath)) : folder || "Library item";
}
function seasonLabel(row: LibraryAuditRow) {
  const parts = row.folderName.split(" / ");
  return parts[1]?.trim() || folderName(row.folderPath) || "Movie";
}
function titleKey(value: string) {
  return value.normalize("NFKD").toLocaleLowerCase().replace(/\s*\(\d{4}\)\s*$/, "").replace(/[^\p{L}\p{N}]+/gu, "").trim();
}
function normalizePath(path: string) {
  return path.trim().replace(/\\/g, "/").replace(/\/+$/, "").toLocaleLowerCase();
}
function uniquePaths(paths: string[]) {
  const seen = new Set<string>();
  return paths.filter((path) => {
    if (!path.trim()) return false;
    const key = normalizePath(path);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
function folderName(path: string) {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
}
function parentPath(path: string) {
  const clean = path.replace(/[\\/]+$/, "");
  const index = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  return index > 0 ? clean.slice(0, index) : "";
}
