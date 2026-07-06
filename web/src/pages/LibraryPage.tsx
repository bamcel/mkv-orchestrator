import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Database, RefreshCw, SearchCheck, Send, Square, Trash2 } from "lucide-react";
import {
  buildLibraryAudit,
  cancelScan,
  clearCurrentScanFiles,
  getCurrentScanFiles,
  getScanJob,
  getStatus,
  LibraryAuditResponse,
  LibraryAuditRow,
  startScan
} from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

export function LibraryPage() {
  const { files, setFiles, setTemplateFilePath } = useMediaLibrary();
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [auditResult, setAuditResult] = useState<LibraryAuditResponse | null>(null);
  const [selectedFolder, setSelectedFolder] = useState("");
  const [scanJobId, setScanJobId] = useState<string | null>(null);
  const [showWarningsOnly, setShowWarningsOnly] = useState(false);
  const [statusText, setStatusText] = useState("Load scanned files from Dashboard, then build the library overview.");

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
  }, [currentScan.data, files.length, setFiles]);

  const audit = useMutation({
    mutationFn: buildLibraryAudit,
    onSuccess: (response) => {
      setAuditResult(response);
      setSelectedFolder((current) => current || response.items[0]?.folderPath || "");
      setStatusText(`Build Overview: ${response.summary.groups} group(s), ${response.summary.issueGroups} warning group(s), ${response.summary.files} file(s).`);
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Library audit failed.")
  });
  const scanStart = useMutation({
    mutationFn: startScan,
    onSuccess: (job) => {
      setScanJobId(job.id);
      setStatusText("Library cache build queued.");
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Library cache build failed to start.")
  });
  const scanCancel = useMutation({
    mutationFn: cancelScan,
    onSuccess: () => setStatusText("Cancel requested for the library cache build."),
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Cancel build failed.")
  });
  const clearScan = useMutation({
    mutationFn: clearCurrentScanFiles,
    onSuccess: () => {
      setFiles([]);
      setAuditResult(null);
      setSelectedFolder("");
      setScanJobId(null);
      setStatusText("Library scan cache cleared.");
      currentScan.refetch();
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Clear library cache failed.")
  });
  const scanJob = useQuery({
    queryKey: ["library-cache-job", scanJobId],
    queryFn: () => getScanJob(scanJobId!),
    enabled: scanJobId !== null,
    refetchInterval: (query) => {
      const job = query.state.data;
      return job && ["Completed", "Failed", "Canceled"].includes(job.status) ? false : 1000;
    }
  });

  const selected = useMemo(() => {
    return auditResult?.items.find((item) => item.folderPath === selectedFolder) ?? auditResult?.items[0] ?? null;
  }, [auditResult, selectedFolder]);
  const displayedItems = useMemo(() => {
    const items = auditResult?.items ?? [];
    return showWarningsOnly ? items.filter((item) => item.hasIssues) : items;
  }, [auditResult, showWarningsOnly]);
  const configuredSources = useMemo(() => {
    const roots = status.data?.sourceRoots.map((root) => root.path).filter(Boolean) ?? [];
    return roots.length > 0 ? roots : [status.data?.mediaRoot ?? "/media"];
  }, [status.data]);
  const currentScanJob = scanJob.data;
  const isCacheBuilding = scanStart.isPending || currentScanJob?.status === "Queued" || currentScanJob?.status === "Running" || currentScanJob?.status === "Canceling";
  const cacheProgressText = currentScanJob?.total
    ? `${currentScanJob.completed}/${currentScanJob.total} files`
    : isCacheBuilding ? "preparing cache build" : "";

  useEffect(() => {
    if (!currentScanJob) return;

    setFiles(currentScanJob.files);
    if (currentScanJob.status === "Completed") {
      setAuditResult(null);
      setSelectedFolder("");
      setStatusText(`Library cache build complete: ${currentScanJob.summary.total} file(s), ${currentScanJob.summary.mkv} MKV, ${currentScanJob.summary.mp4} MP4.`);
      currentScan.refetch();
    } else if (currentScanJob.status === "Failed") {
      setStatusText(currentScanJob.error || "Library cache build failed.");
    } else if (currentScanJob.status === "Canceled") {
      setStatusText("Library cache build canceled.");
    } else if (currentScanJob.status === "Running") {
      setStatusText(`Building library cache: ${cacheProgressText}`);
    }
  }, [currentScanJob, setFiles, currentScan, cacheProgressText]);

  async function refreshFiles() {
    const result = await currentScan.refetch();
    if (result.data?.files.length) {
      setFiles(result.data.files);
      setStatusText(`Loaded ${result.data.files.length} scanned file(s).`);
    } else {
      setStatusText("No Dashboard scan is available yet.");
    }
  }

  function runAudit() {
    audit.mutate(files);
  }

  function buildLibraryCache() {
    setAuditResult(null);
    setSelectedFolder("");
    scanStart.mutate({ sources: configuredSources });
  }

  function cancelLibraryCacheBuild() {
    if (!scanJobId) return;
    scanCancel.mutate(scanJobId);
  }

  function useTemplate(row: LibraryAuditRow) {
    setTemplateFilePath(row.templateFilePath);
    setStatusText(`Template file set: ${row.templateFileName}`);
  }

  function sendSelectionToDashboard(row: LibraryAuditRow | null) {
    if (!row) {
      setStatusText("Select a folder before sending files to Dashboard.");
      return;
    }

    const paths = [
      row.templateFilePath,
      ...(row.issueFilePaths.length > 0 ? row.issueFilePaths : row.hasIssues ? row.allFilePaths : [])
    ];
    const selectedPathSet = new Set(paths.map((path) => path.toLowerCase()));
    const selectedFiles = files
      .filter((file) => selectedPathSet.has(file.path.toLowerCase()))
      .sort((left, right) => {
        if (left.path === row.templateFilePath) return -1;
        if (right.path === row.templateFilePath) return 1;
        return left.path.localeCompare(right.path);
      })
      .map((file) => ({
        ...file,
        status: file.path === row.templateFilePath ? "Template" : row.hasIssues ? "Library Warning" : file.status
      }));

    if (selectedFiles.length === 0) {
      setStatusText("No matching scanned files were found for the selected folder.");
      return;
    }

    setFiles(selectedFiles);
    setTemplateFilePath(row.templateFilePath);
    setStatusText(`Sent ${selectedFiles.length} file(s) to Dashboard. Template file set: ${row.templateFileName}`);
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Library" description="Build an overview of scanned folders and highlight metadata mismatches." />
      <div className="grid min-h-0 flex-1 grid-cols-[370px_1fr] gap-5">
        <section className="min-h-0 overflow-auto rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Library Build Overview</h2>
          <div className="mt-4 grid grid-cols-2 gap-2">
            <button onClick={buildLibraryCache} disabled={isCacheBuilding || configuredSources.length === 0} className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-accent text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
              {isCacheBuilding ? <RefreshCw size={15} className="animate-spin" /> : <Database size={15} />}
              Build Cache
            </button>
            <button onClick={cancelLibraryCacheBuild} disabled={!isCacheBuilding || scanCancel.isPending} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              <Square size={13} />
              Cancel Build
            </button>
            <button onClick={refreshFiles} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text">
              <RefreshCw size={15} />
              Refresh Files
            </button>
            <button onClick={() => clearScan.mutate()} disabled={clearScan.isPending || files.length === 0} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              <Trash2 size={15} />
              Clear Cache
            </button>
            <button onClick={runAudit} disabled={audit.isPending || files.length === 0} className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-accent text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
              {audit.isPending ? <RefreshCw size={15} className="animate-spin" /> : <SearchCheck size={15} />}
              Build Overview
            </button>
            <button onClick={() => setShowWarningsOnly((current) => !current)} disabled={!auditResult?.items.length} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              {showWarningsOnly ? "Show All" : "Show Warnings"}
            </button>
            <button onClick={() => sendSelectionToDashboard(selected)} disabled={!selected} className="col-span-2 inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled">
              <Send size={15} />
              Send Selection to Dashboard
            </button>
          </div>

          <div className="mt-5 grid grid-cols-2 gap-3">
            <Metric label="Files" value={auditResult?.summary.files ?? files.length} />
            <Metric label="Groups" value={auditResult?.summary.groups ?? 0} />
            <Metric label="Standard" value={auditResult?.summary.standardGroups ?? 0} />
            <Metric label="Warnings" value={auditResult?.summary.issueGroups ?? 0} warning />
          </div>

          <div className="mt-5 text-sm text-success">{statusText}</div>
          {currentScanJob?.currentSource && isCacheBuilding ? (
            <div className="mt-2 truncate text-xs text-subtle" title={currentScanJob.currentSource}>
              {currentScanJob.currentSource}
            </div>
          ) : null}
          <div className="mt-3 rounded-lg border border-border bg-panel p-3 text-xs leading-5 text-muted">
            Build Cache scans configured container roots ({configuredSources.join(", ")}). Build Overview groups the current scan and highlights track-layout differences.
          </div>
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Folder Audit</h2>
          <div className="mt-4 grid min-h-0 flex-1 grid-cols-[minmax(430px,1fr)_minmax(360px,0.8fr)] gap-4">
            <div className="min-h-0 overflow-auto rounded-lg border border-border bg-panel">
              {displayedItems.length ? (
                <table className="w-full min-w-[900px] border-collapse text-left text-sm">
                  <thead className="sticky top-0 bg-panel text-xs uppercase tracking-wide text-subtle">
                    <tr>
                      <th className="border-b border-border px-3 py-2">Folder</th>
                      <th className="border-b border-border px-3 py-2">Files</th>
                      <th className="border-b border-border px-3 py-2">Video Standard</th>
                      <th className="border-b border-border px-3 py-2">Audio Standard</th>
                      <th className="border-b border-border px-3 py-2">Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {displayedItems.map((item) => (
                      <tr key={item.folderPath} onClick={() => setSelectedFolder(item.folderPath)} className={["cursor-pointer bg-card hover:bg-selected", selected?.folderPath === item.folderPath ? "bg-selected" : ""].join(" ")}>
                        <td className="max-w-[300px] truncate border-b border-border px-3 py-2" title={item.folderPath}>{item.folderName}</td>
                        <td className="border-b border-border px-3 py-2">{item.fileCount}</td>
                        <td className="max-w-[220px] truncate border-b border-border px-3 py-2 text-muted" title={item.standardVideo}>{item.standardVideo}</td>
                        <td className="max-w-[260px] truncate border-b border-border px-3 py-2 text-muted" title={item.standardAudio}>{item.standardAudio}</td>
                        <td className={["border-b border-border px-3 py-2", item.hasIssues ? "text-warning" : "text-success"].join(" ")}>{item.hasIssues ? "Warning" : "Standard"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : (
                <div className="flex h-full min-h-[320px] items-center justify-center text-sm text-subtle">
                  {auditResult ? "No folders match the current filter." : "Build an overview to inspect scanned folders."}
                </div>
              )}
            </div>

            <div className="min-h-0 overflow-auto rounded-lg border border-border bg-panel p-4">
              <h3 className="text-sm font-semibold">Selection Details</h3>
              {selected ? (
                <div className="mt-4 space-y-4 text-sm">
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-wide text-subtle">Folder</div>
                    <div className="mt-1 break-all text-text">{selected.folderPath}</div>
                  </div>
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-wide text-subtle">Template File</div>
                    <div className="mt-1 text-accent">{selected.templateFileName}</div>
                    <button onClick={() => useTemplate(selected)} className="mt-2 rounded-md border border-border bg-button px-3 py-1.5 text-xs font-semibold text-muted hover:bg-button-hover hover:text-text">
                      Use Template
                    </button>
                  </div>
                  <Detail label="Video" value={selected.standardVideo} />
                  <Detail label="Audio" value={selected.standardAudio} />
                  <Detail label="Subtitles" value={selected.standardSubtitles} />
                  <div>
                    <div className="text-xs font-semibold uppercase tracking-wide text-subtle">Issues</div>
                    <div className="mt-2 space-y-2">
                      {selected.issues.length === 0 ? (
                        <div className="text-success">No issues found.</div>
                      ) : selected.issues.map((issue) => (
                        <div key={issue} className="rounded-md border border-border bg-input p-2 text-warning">{issue}</div>
                      ))}
                    </div>
                  </div>
                </div>
              ) : (
                <div className="mt-4 text-sm text-subtle">Select a folder to review standards and warnings.</div>
              )}
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

function Metric({ label, value, warning = false }: { label: string; value: number; warning?: boolean }) {
  return (
    <div className="rounded-lg border border-border bg-panel p-3">
      <div className="text-xs font-semibold uppercase tracking-wide text-subtle">{label}</div>
      <div className={["mt-1 text-2xl font-semibold", warning ? "text-warning" : "text-text"].join(" ")}>{value}</div>
    </div>
  );
}

function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs font-semibold uppercase tracking-wide text-subtle">{label}</div>
      <div className="mt-1 break-words text-muted">{value || "unknown"}</div>
    </div>
  );
}
