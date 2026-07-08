import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import {
  buildLibraryAudit,
  cancelScan,
  getCurrentScanFiles,
  getScanJob,
  getStatus,
  LibraryAuditResponse,
  LibraryAuditRow,
  startScan
} from "../api";
import { OutputModal } from "../components/OutputModal";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

export function LibraryPage() {
  const { files, setFiles, setTemplateFilePath } = useMediaLibrary();
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [auditResult, setAuditResult] = useState<LibraryAuditResponse | null>(null);
  const [selectedFolder, setSelectedFolder] = useState("");
  const [selectedSource, setSelectedSource] = useState("");
  const [scanJobId, setScanJobId] = useState<string | null>(null);
  const [showWarningsOnly, setShowWarningsOnly] = useState(false);
  const [pendingOverviewScan, setPendingOverviewScan] = useState(false);
  const [isDetailsExpanded, setIsDetailsExpanded] = useState(false);
  const [statusText, setStatusText] = useState("Select a watch folder, then build the library overview.");

  const sourceOptions = useMemo(() => {
    const roots = status.data?.sourceRoots?.length
      ? status.data.sourceRoots
      : [{ name: "media", path: status.data?.mediaRoot ?? "/media" }];
    const seen = new Set<string>();
    return roots.filter((root) => {
      const path = root.path.trim();
      if (!path || seen.has(path.toLowerCase())) return false;
      seen.add(path.toLowerCase());
      return true;
    });
  }, [status.data]);

  useEffect(() => {
    if (selectedSource || sourceOptions.length === 0) return;
    setSelectedSource(sourceOptions[0].path);
  }, [selectedSource, sourceOptions]);

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
  }, [currentScan.data, files.length, setFiles]);

  const audit = useMutation({
    mutationFn: buildLibraryAudit,
    onSuccess: (response) => {
      setAuditResult(response);
      setSelectedFolder((current) => current || response.items[0]?.folderPath || "");
      setStatusText(`Library overview ready: ${response.summary.groups} folders, ${response.summary.files} files, ${response.summary.issueGroups} warning groups.`);
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
      setStatusText("Building library overview...");
    },
    onError: (error) => {
      setPendingOverviewScan(false);
      setStatusText(error instanceof Error ? error.message : "Library overview failed to start.");
    }
  });

  const scanCancel = useMutation({
    mutationFn: cancelScan,
    onSuccess: () => {
      setPendingOverviewScan(false);
      setStatusText("Cancel requested for the library overview build.");
    },
    onError: (error) => setStatusText(error instanceof Error ? error.message : "Cancel failed.")
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

  const currentScanJob = scanJob.data;
  const isBusy = scanStart.isPending
    || audit.isPending
    || currentScanJob?.status === "Queued"
    || currentScanJob?.status === "Running"
    || currentScanJob?.status === "Canceling";
  const cacheProgressText = currentScanJob?.total
    ? `${currentScanJob.completed}/${currentScanJob.total} files`
    : isBusy ? "preparing" : "";

  useEffect(() => {
    if (!currentScanJob) return;

    if (currentScanJob.status === "Running") {
      setFiles(currentScanJob.files);
      setStatusText(`Building library overview: ${cacheProgressText}`);
      return;
    }

    if (currentScanJob.status === "Completed") {
      setFiles(currentScanJob.files);
      if (pendingOverviewScan) {
        setPendingOverviewScan(false);
        audit.mutate(currentScanJob.files);
      } else {
        setStatusText(`Loaded ${currentScanJob.summary.total} file(s) from ${selectedSource || "selected source"}.`);
      }
      currentScan.refetch();
    } else if (currentScanJob.status === "Failed") {
      setPendingOverviewScan(false);
      setStatusText(currentScanJob.error || "Library overview build failed.");
    } else if (currentScanJob.status === "Canceled") {
      setPendingOverviewScan(false);
      setStatusText("Library overview build canceled.");
    }
  }, [audit, cacheProgressText, currentScan, currentScanJob, pendingOverviewScan, selectedSource, setFiles]);

  const selected = useMemo(() => {
    return auditResult?.items.find((item) => item.folderPath === selectedFolder) ?? auditResult?.items[0] ?? null;
  }, [auditResult, selectedFolder]);

  const displayedItems = useMemo(() => {
    const items = auditResult?.items ?? [];
    return showWarningsOnly ? items.filter((item) => item.hasIssues) : items;
  }, [auditResult, showWarningsOnly]);

  const detailSummary = selected
    ? selected.hasIssues
      ? `Will send ${selected.issueFilePaths.length} mismatched file(s) plus template: ${selected.templateFileName}`
      : `No warnings found. Template: ${selected.templateFileName}`
    : "Select a folder to review standards and warnings.";

  async function refreshLibrarySource() {
    status.refetch();
    const result = await currentScan.refetch();
    if (result.data?.files.length) {
      setFiles(result.data.files);
      setStatusText(`Loaded ${result.data.files.length} scanned file(s) from Dashboard.`);
    } else {
      setStatusText("No Dashboard scan is available yet. Build Overview can scan the selected watch folder.");
    }
  }

  function runBuildOverview() {
    if (!selectedSource.trim()) {
      setStatusText("Select a watch folder first.");
      return;
    }

    setAuditResult(null);
    setSelectedFolder("");
    setPendingOverviewScan(true);
    scanStart.mutate({ sources: [selectedSource] });
  }

  function cancelLibraryBuild() {
    if (!scanJobId) return;
    scanCancel.mutate(scanJobId);
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
      <SectionHeader title="Library" description="Browse cached watch-folder coverage, season groups, and items that may need attention." />

      <div className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)_190px] gap-3">
        <section className="rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Library Source</h2>
          <div className="mt-3 grid grid-cols-[120px_minmax(260px,420px)_1fr] items-center gap-3">
            <label className="text-sm text-muted" htmlFor="library-source">Source</label>
            <select
              id="library-source"
              value={selectedSource}
              onChange={(event) => setSelectedSource(event.target.value)}
              className="h-9 min-w-0 rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
            >
              {sourceOptions.map((source) => (
                <option key={source.path} value={source.path}>{source.name}: {source.path}</option>
              ))}
            </select>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <button type="button" onClick={refreshLibrarySource} className="h-9 whitespace-nowrap rounded-md border border-border bg-button px-5 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text">
              Refresh
            </button>
            <button type="button" onClick={runBuildOverview} disabled={isBusy || !selectedSource} className="h-9 whitespace-nowrap rounded-md bg-accent px-5 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:bg-button disabled:text-disabled">
              {isBusy ? "Building..." : "Build Overview"}
            </button>
            <button type="button" onClick={() => sendSelectionToDashboard(selected)} disabled={!selected || isBusy} className="h-9 whitespace-nowrap rounded-md border border-border bg-button px-5 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled">
              Send Selection to Dashboard
            </button>
            <button type="button" onClick={cancelLibraryBuild} disabled={!isBusy || scanCancel.isPending} className="h-9 whitespace-nowrap rounded-md border border-border bg-button px-5 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled">
              Cancel
            </button>
          </div>
          <p className="mt-3 text-sm leading-5 text-muted">
            Builds a cached overview from the selected manual, container, or synced media-server source. Select a season or folder group to review its media profile, warnings, and files that can be sent into Dashboard for rename, merge, or track-property work.
          </p>
          {currentScanJob?.currentSource && isBusy ? (
            <div className="mt-2 truncate font-mono text-xs text-subtle" title={currentScanJob.currentSource}>
              {currentScanJob.currentSource}
            </div>
          ) : null}
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex shrink-0 items-center justify-between">
            <h2 className="text-base font-semibold">Library Overview</h2>
            <button
              type="button"
              onClick={() => setShowWarningsOnly((current) => !current)}
              disabled={!auditResult?.items.length}
              className="h-8 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              {showWarningsOnly ? "Show All" : "Show Warnings"}
            </button>
          </div>

          <div className="mt-3 min-h-0 flex-1 overflow-auto">
            {displayedItems.length ? (
              <table className="w-full min-w-[1220px] table-fixed border-collapse text-left text-sm">
                <thead className="sticky top-0 bg-card text-xs text-text">
                  <tr>
                    <th className="w-24 border-b border-border px-1 py-2 font-semibold">status</th>
                    <th className="w-[220px] border-b border-border px-1 py-2 font-semibold">title</th>
                    <th className="w-[150px] border-b border-border px-1 py-2 font-semibold">season/folder</th>
                    <th className="w-16 border-b border-border px-1 py-2 font-semibold">files</th>
                    <th className="w-[220px] border-b border-border px-1 py-2 font-semibold">standard video</th>
                    <th className="w-[260px] border-b border-border px-1 py-2 font-semibold">standard audio</th>
                    <th className="w-[260px] border-b border-border px-1 py-2 font-semibold">standard subtitles</th>
                    <th className="border-b border-border px-1 py-2 font-semibold">warnings</th>
                  </tr>
                </thead>
                <tbody>
                  {displayedItems.map((item) => {
                    const visualClass = item.hasIssues ? "text-warning" : "text-text";
                    const rowClass = selected?.folderPath === item.folderPath ? "bg-selected" : "bg-card";
                    return (
                      <tr key={item.folderPath} onClick={() => setSelectedFolder(item.folderPath)} className={`${rowClass} cursor-pointer hover:bg-selected`}>
                        <TableCell className={visualClass} title={item.issueSummary}>{item.hasIssues ? "warning" : "standard"}</TableCell>
                        <TableCell className={visualClass} title={getAuditTitle(item)}>{getAuditTitle(item)}</TableCell>
                        <TableCell className={visualClass} title={getSeasonFolder(item)}>{getSeasonFolder(item)}</TableCell>
                        <TableCell className={visualClass}>{item.fileCount}</TableCell>
                        <TableCell className={visualClass} title={item.standardVideo}>{item.standardVideo}</TableCell>
                        <TableCell className={visualClass} title={item.standardAudio}>{item.standardAudio}</TableCell>
                        <TableCell className={visualClass} title={item.standardSubtitles}>{item.standardSubtitles}</TableCell>
                        <TableCell className={visualClass} title={item.issueSummary}>{item.hasIssues ? item.issueSummary : ""}</TableCell>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            ) : (
              <div className="flex h-full min-h-[320px] items-center justify-center text-sm text-subtle">
                {auditResult ? "No folders match the current filter." : "Build an overview to inspect scanned folders."}
              </div>
            )}
          </div>
        </section>

        <section className="flex min-h-0 flex-col rounded-lg border border-border bg-card p-4 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex shrink-0 items-start justify-between gap-3">
            <div className="min-w-0">
              <h2 className="text-base font-semibold">Selection Details</h2>
              <div className="mt-1 truncate text-sm text-muted" title={detailSummary}>{detailSummary}</div>
            </div>
            <button
              type="button"
              onClick={() => setIsDetailsExpanded(true)}
              disabled={!selected}
              className="h-8 rounded-md border border-border bg-button px-4 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              Expand
            </button>
          </div>

          <pre className="mt-3 min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words rounded-md bg-input p-3 font-mono text-xs leading-5 text-muted">
            {selected ? buildIssueText(selected) : "Select a folder to review standards and warnings."}
          </pre>
          <div className="mt-2 shrink-0 text-sm text-success">{statusText}</div>
        </section>
      </div>

      {isDetailsExpanded && selected ? (
        <OutputModal
          title="Library Selection Details"
          content={`${detailSummary}\n\n${buildIssueText(selected)}\n\n${statusText}`}
          onClose={() => setIsDetailsExpanded(false)}
        />
      ) : null}
    </div>
  );
}

function TableCell({ children, className, title }: { children: React.ReactNode; className?: string; title?: string }) {
  return (
    <td className={`truncate border-b border-border px-1 py-2 ${className ?? ""}`} title={title}>
      {children}
    </td>
  );
}

function buildIssueText(row: LibraryAuditRow) {
  if (row.issues.length === 0) return "No issues found.";
  return row.issues.join("\n\n");
}

function getAuditTitle(row: LibraryAuditRow) {
  const folderName = row.folderName || getBaseName(row.folderPath);
  if (isSeasonFolder(folderName)) {
    return getBaseName(getParentPath(row.folderPath)) || folderName;
  }

  return folderName || "root";
}

function getSeasonFolder(row: LibraryAuditRow) {
  const folderName = row.folderName || getBaseName(row.folderPath);
  if (isSeasonFolder(folderName)) return folderName;
  return row.fileCount === 1 ? "movie/single folder" : folderName || "root";
}

function isSeasonFolder(value: string) {
  return /^season\s*\d+/i.test(value.trim());
}

function getParentPath(path: string) {
  const clean = path.trim().replace(/[\\/]+$/, "");
  const slash = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  if (slash <= 0) return "";
  return clean.slice(0, slash);
}

function getBaseName(path: string) {
  const clean = path.trim().replace(/[\\/]+$/, "");
  if (!clean) return "";
  const slash = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  return slash >= 0 ? clean.slice(slash + 1) : clean;
}
