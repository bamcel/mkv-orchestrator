import { Fragment, type MouseEvent, type ReactNode, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronUp, Copy, FileCheck, FileVideo, Folder, FolderOpen, Plus, RefreshCw, Search, Trash2, X } from "lucide-react";
import { authorizeBrowsedRoot, cancelScan, FileSystemEntry, getBackendTransport, getCurrentScanFiles, getScanJob, getStatus, getWebSettings, MediaFileRow, startScan } from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { FileBrowser } from "../components/FileBrowser";
import { useMediaLibrary } from "../state/MediaLibraryContext";

const lastBrowsePathStorageKey = "mkvo.web.lastBrowsePath";

export function DashboardPage() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const { files, setFiles, templateFilePath, setTemplateFilePath, hydrateSelection } = useMediaLibrary();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [sources, setSources] = useState<string[]>([]);
  const [isBrowseOpen, setIsBrowseOpen] = useState(false);
  const [lastBrowsePath, setLastBrowsePath] = useState(() => {
    try {
      return window.localStorage.getItem(lastBrowsePathStorageKey) ?? "";
    } catch {
      return "";
    }
  });
  const [scanJobId, setScanJobId] = useState<string | null>(null);
  // The scan request carries this list, so seeding it from a local constant
  // would silently override the folders configured in Settings on every scan.
  const settings = useQuery({ queryKey: ["settings"], queryFn: getWebSettings });
  const [ignoredFolders, setIgnoredFolders] = useState("");
  const [ignoredFoldersEdited, setIgnoredFoldersEdited] = useState(false);
  const [skipped, setSkipped] = useState<string[]>([]);
  const [selectedFilePath, setSelectedFilePath] = useState<string>("");
  const [actionStatus, setActionStatus] = useState("");
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const scanStart = useMutation({
    mutationFn: startScan,
    onSuccess: (job) => {
      setScanJobId(job.id);
    }
  });
  const scanCancel = useMutation({ mutationFn: cancelScan });
  const scanJob = useQuery({
    queryKey: ["scan-job", scanJobId],
    queryFn: () => getScanJob(scanJobId!),
    enabled: scanJobId !== null,
    refetchInterval: (query) => {
      const job = query.state.data;
      return job && ["Completed", "Failed", "Skipped", "Canceled"].includes(job.status) ? false : 1000;
    }
  });

  const configuredIgnoredFolders = settings.data?.ignoredScanFolderNames;
  useEffect(() => {
    if (ignoredFoldersEdited || !configuredIgnoredFolders) return;
    setIgnoredFolders(configuredIgnoredFolders.join(", "));
  }, [configuredIgnoredFolders, ignoredFoldersEdited]);

  // A container has a mount to point at; a desktop has the whole machine, so
  // guidance that names /media is wrong there.
  const isDesktop = getBackendTransport() === "tauri";
  // An empty media root means the user has named no library folder yet, which
  // only the desktop can report. Browsing then opens at the volume list rather
  // than at a folder nobody chose.
  const defaultSourcePath = status.data?.mediaRoot ?? (isDesktop ? "" : "/media");
  const browseRootOptions = useMemo(() => {
    // The host reports its own roots and the user's library folders as one
    // list, each already named. Synthesising a "Media" entry here would win the
    // dedupe below and relabel whatever the user called their own folder.
    const roots = status.data?.sourceRoots ?? [];
    const seen = new Set<string>();
    return roots.filter((root) => {
      const path = root.path.trim();
      if (!path || seen.has(path.toLowerCase())) return false;
      seen.add(path.toLowerCase());
      return true;
    });
  }, [status.data?.sourceRoots]);
  const activeSources = sources;
  const hasSources = sources.length > 0;
  const sourceSummary = sources.length === 0
    ? "No source selected"
    : sources.length === 1
      ? sources[0]
      : `${sources.length} sources selected`;
  const currentScanJob = scanJob.data;
  const isScanning = scanStart.isPending || currentScanJob?.status === "Queued" || currentScanJob?.status === "WaitingForResources" || currentScanJob?.status === "Running" || currentScanJob?.status === "Canceling";
  const progressText = currentScanJob?.total
    ? `${currentScanJob.completed}/${currentScanJob.total} files`
    : isScanning ? "preparing scan" : "";
  const summary = useMemo(() => {
    const mkv = files.filter((file) => file.extension.toLowerCase() === ".mkv").length;
    const mp4 = files.filter((file) => file.extension.toLowerCase() === ".mp4").length;
    const failed = files.filter((file) => file.status.toLowerCase().includes("failed")).length;
    return { total: files.length, mkv, mp4, failed };
  }, [files]);
  const selectedFile = useMemo(() => {
    if (files.length === 0) return null;
    return files.find((file) => file.path === selectedFilePath) ?? files[0];
  }, [files, selectedFilePath]);
  const templateFile = useMemo(() => {
    if (files.length === 0) return null;
    return files.find((file) => file.path === templateFilePath) ?? files[0];
  }, [files, templateFilePath]);
  const selectedMismatchMessages = useMemo(
    () => selectedFile ? getTemplateMismatchMessages(selectedFile, templateFile) : [],
    [selectedFile, templateFile]
  );
  /**
   * Media Info rows, built either way so the panel keeps its shape.
   *
   * With nothing selected the labels still show, each against a dash: the
   * reader can see what the panel will tell them once a file is picked, rather
   * than an empty box.
   */
  const mediaInfoRows = useMemo(() => {
    if (!selectedFile) {
      return ["File", "Codec", "Resolution", "Bit Depth", "HDR", "Audio", "Subtitles", "Status"].map(
        (label) => ({ label, value: "—", className: "text-subtle", title: undefined as string | undefined })
      );
    }

    const statusClass = isTemplate(selectedFile)
      ? "text-accent"
      : hasTemplateMismatch(selectedFile, templateFile)
        ? "text-warning"
        : "text-success";
    const statusText = isTemplate(selectedFile)
      ? "Template"
      : hasTemplateMismatch(selectedFile, templateFile)
        ? "Warning"
        : selectedFile.status;

    return [
      { label: "File", value: selectedFile.fileName, className: "truncate text-text", title: selectedFile.path },
      { label: "Codec", value: selectedFile.codec || "Unknown", className: compareTextClass(selectedFile, (row) => row.codec), title: undefined },
      { label: "Resolution", value: selectedFile.resolution || "Unknown", className: compareTextClass(selectedFile, (row) => row.resolution), title: undefined },
      { label: "Bit Depth", value: selectedFile.bitDepth || "Unknown", className: compareTextClass(selectedFile, (row) => row.bitDepth), title: undefined },
      { label: "HDR", value: selectedFile.hdr || "None", className: compareTextClass(selectedFile, (row) => row.hdr), title: undefined },
      {
        label: "Audio",
        value: selectedFile.audioSummary || "None",
        className: ["truncate", compareTextClass(selectedFile, (row) => row.audioSummary)].join(" "),
        title: selectedFile.audioSummary
      },
      {
        label: "Subtitles",
        value: selectedFile.subtitleSummary || "None",
        className: ["truncate", compareTextClass(selectedFile, (row) => row.subtitleSummary)].join(" "),
        title: selectedFile.subtitleSummary
      },
      { label: "Status", value: statusText, className: statusClass, title: undefined }
    ];
  }, [selectedFile, templateFile]);

  const dashboardStatus = actionStatus
    || (isScanning ? `scan executing ${progressText}` : files.length > 0 ? `${files.length} file(s) scanned` : hasSources ? "ready" : "choose a source to scan");

  useEffect(() => {
    if (!contextMenu) return;

    const close = () => setContextMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [contextMenu]);

  useEffect(() => {
    if (!currentScanJob) return;

    setFiles(currentScanJob.files);
    setSkipped(currentScanJob.skipped);

    if (currentScanJob.status === "Completed") {
      setFiles(currentScanJob.files);
      setSkipped(currentScanJob.skipped);
      setSelectedFilePath(currentScanJob.files[0]?.path ?? "");
    } else if (currentScanJob.status === "Failed") {
      setSkipped(currentScanJob.error ? [currentScanJob.error] : ["Scan failed."]);
    }
  }, [currentScanJob]);

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
    // Rust owns the selection, so adopt what it reports rather than keeping
    // whatever this tab happened to have.
    hydrateSelection(currentScan.data.selectedPaths);
    setSelectedFilePath(currentScan.data.files[0]?.path ?? "");
  }, [currentScan.data, files.length, setFiles]);

  function runScan() {
    if (!hasSources) return;

    setFiles([]);
    setSkipped([]);
    setSelectedFilePath("");
    setActionStatus("");
    setScanJobId(null);
    scanStart.mutate({
      sources: activeSources,
      ignoredFolderNames: ignoredFolders.split(/[\n,]/).map((item) => item.trim()).filter(Boolean)
    });
  }

  function cancelCurrentScan() {
    if (!scanJobId) return;
    scanCancel.mutate(scanJobId);
  }

  function addSource(path: string) {
    const cleanPath = path.trim();
    if (!cleanPath) return;
    setSources((current) => current.some((item) => item.toLowerCase() === cleanPath.toLowerCase())
      ? current
      : [...current, cleanPath]);
  }

  function removeSource(path: string) {
    setSources((current) => current.filter((item) => item !== path));
  }

  function rememberBrowsePath(path: string | null | undefined) {
    const cleanPath = path?.trim();
    if (!cleanPath) return;

    setLastBrowsePath(cleanPath);
    try {
      window.localStorage.setItem(lastBrowsePathStorageKey, cleanPath);
    } catch {
      // Ignore storage failures; browsing should still work.
    }
  }

  function openBrowse() {
    setIsBrowseOpen(true);
  }

  async function addBrowsePath(path: string, kind: FileSystemEntry["kind"] = "folder") {
    // Browsing may range wider than the authorized roots, so the chosen folder
    // is not usable as a scan source until the backend grants it. Choosing it
    // is the user's explicit act; the backend still validates the path.
    const folder = kind === "file" ? getParentPath(path) : path;
    try {
      await authorizeBrowsedRoot(folder || path);
    } catch (error) {
      setActionStatus(error instanceof Error ? error.message : "That folder could not be authorized.");
      return;
    }
    addSource(path);
    rememberBrowsePath(folder || path);
    setIsBrowseOpen(false);
  }

  function useSelectedAsTemplate() {
    if (!selectedFile) return;
    setTemplateFilePath(selectedFile.path);
    setActionStatus(`Template file set: ${selectedFile.fileName}`);
  }

  function openFileContextMenu(event: MouseEvent<HTMLTableRowElement>, file: MediaFileRow) {
    event.preventDefault();
    setSelectedFilePath(file.path);
    setContextMenu({ x: event.clientX, y: event.clientY, path: file.path });
  }

  function setContextFileAsTemplate(file: MediaFileRow) {
    setTemplateFilePath(file.path);
    setActionStatus(`Template file set: ${file.fileName}`);
    setContextMenu(null);
  }

  async function copyContextFileText(file: MediaFileRow, value: string, label: string) {
    try {
      await navigator.clipboard.writeText(value);
      setActionStatus(`${label} copied: ${file.fileName}`);
    } catch {
      setActionStatus(`Unable to copy ${label.toLowerCase()}.`);
    }
    setContextMenu(null);
  }

  function removeContextFile(file: MediaFileRow) {
    const remaining = files.filter((item) => item.path !== file.path);
    setFiles(remaining);
    setSelectedFilePath((current) => current === file.path ? remaining[0]?.path ?? "" : current);
    setActionStatus(`Removed from list: ${file.fileName}`);
    setContextMenu(null);
  }

  function isTemplate(file: MediaFileRow) {
    return templateFile?.path === file.path;
  }

  function isDifferent(file: MediaFileRow, selector: (row: MediaFileRow) => string) {
    if (!templateFile || isTemplate(file)) return false;
    return normalizeCompareValue(selector(file)) !== normalizeCompareValue(selector(templateFile));
  }

  function compareTextClass(file: MediaFileRow, selector: (row: MediaFileRow) => string, normal = "text-text") {
    if (isTemplate(file)) return "text-accent";
    return isDifferent(file, selector) ? "text-warning" : normal;
  }

  function trackTextClass(index: number, selector: (track: MediaFileRow["tracks"][number]) => string) {
    if (!templateFile || !selectedFile || isTemplate(selectedFile)) return "text-text";
    const selectedTrack = selectedFile.tracks[index];
    const templateTrack = templateFile.tracks[index];
    if (!selectedTrack || !templateTrack) return "text-warning";
    return normalizeCompareValue(selector(selectedTrack)) === normalizeCompareValue(selector(templateTrack))
      ? "text-text"
      : "text-warning";
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Dashboard" description="Scan folders and review MKV or MP4 file metadata." />
      <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[18.75rem_minmax(0,1fr)] gap-5">
        <section className="min-h-0 overflow-auto rounded-xl border border-border bg-card p-5 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Scan Sources</h2>

          <label className="mt-4 block text-xs font-semibold text-muted">Sources</label>
          <div className="mt-2 rounded-lg border border-border bg-panel p-2">
            <div className="flex items-center gap-2">
              <Folder size={15} className="shrink-0 text-accent" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm text-text" title={sourceSummary}>{sourceSummary}</div>
                <div className="mt-0.5 text-xs text-subtle">
                  {sources.length === 0 ? "Browse to choose a folder or file" : `${sources.length} selected`}
                </div>
              </div>
              {sources.length > 0 ? (
                <button
                  type="button"
                  onClick={() => setSources([])}
                  className="rounded-md p-2 text-subtle transition hover:bg-button-hover hover:text-text"
                  aria-label="Clear selected sources"
                >
                  <X size={15} />
                </button>
              ) : null}
            </div>

            {sources.length > 1 ? (
              <div className="mt-2 max-h-24 space-y-1 overflow-auto border-t border-border pt-2">
                {sources.map((path) => (
                  <div key={path} className="flex items-center gap-2 rounded-md bg-input px-2 py-1.5 text-xs text-muted">
                    <span className="min-w-0 flex-1 truncate" title={path}>{path}</span>
                    <button
                      type="button"
                      onClick={() => removeSource(path)}
                      className="rounded p-1 text-subtle transition hover:bg-button-hover hover:text-text"
                      aria-label={`Remove ${path}`}
                    >
                      <X size={13} />
                    </button>
                  </div>
                ))}
              </div>
            ) : null}
          </div>

          <div className="mt-3 flex gap-2">
            <button
              type="button"
              onClick={openBrowse}
              className="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-border bg-button px-2 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              <FolderOpen size={15} />
              Browse
            </button>
            <button
              onClick={runScan}
              disabled={isScanning || !hasSources}
              className="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md bg-accent px-2 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:bg-button disabled:text-disabled"
            >
              {isScanning ? <RefreshCw size={15} className="animate-spin" /> : <Search size={15} />}
              Scan
            </button>
          </div>

          {isScanning ? (
            <button
              type="button"
              onClick={cancelCurrentScan}
              disabled={scanCancel.isPending || currentScanJob?.status === "Canceling"}
              className="mt-2 inline-flex h-9 w-full items-center justify-center rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              {currentScanJob?.status === "Canceling" ? "Canceling scan" : "Cancel Scan"}
            </button>
          ) : null}

          <label className="mt-4 block text-xs font-semibold text-muted" htmlFor="ignored-folders">Ignored Subfolders</label>
          <textarea
            id="ignored-folders"
            value={ignoredFolders}
            onChange={(event) => {
              setIgnoredFoldersEdited(true);
              setIgnoredFolders(event.target.value);
            }}
            rows={4}
            className="mt-2 w-full resize-none rounded-md border border-border bg-input px-3 py-2 text-sm text-text outline-none placeholder:text-subtle transition focus:border-accent"
          />

          <div className="mt-4 flex gap-2">
            <button
              onClick={() => status.refetch()}
              className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              <RefreshCw size={15} />
              Refresh
            </button>
          </div>

          <div className="mt-4 text-sm text-success">{dashboardStatus}</div>
          {selectedMismatchMessages.length > 0 ? (
            <div className="mt-3 rounded-md border border-warning bg-input p-3 text-xs text-warning">
              <div className="font-semibold">Selected file mismatches</div>
              <ul className="mt-2 list-disc space-y-1 pl-4 leading-5">
                {selectedMismatchMessages.map((message) => <li key={message}>{message}</li>)}
              </ul>
            </div>
          ) : null}
          {currentScanJob?.currentSource && isScanning ? (
            <div className="mt-2 truncate text-xs text-subtle" title={currentScanJob.currentSource}>
              {currentScanJob.currentSource}
            </div>
          ) : null}
          {scanStart.error ? <div className="mt-3 rounded-md border border-warning bg-input p-3 text-xs text-warning">{String(scanStart.error.message)}</div> : null}
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
          <div className="flex min-w-0 shrink-0 items-start justify-between gap-4">
            <div className="min-w-0 flex-1">
              <h2 className="text-base font-semibold">File Info</h2>
              <div className="mt-1 truncate text-xs text-muted" title={templateFile?.path}>
                Template: {templateFile?.fileName ?? "None selected"}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-3">
              <button
                type="button"
                onClick={useSelectedAsTemplate}
                disabled={!selectedFile}
                className="whitespace-nowrap rounded-md border border-border bg-button px-3 py-2 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
              >
                Use Selected as Template
              </button>
              <div className="whitespace-nowrap text-xs text-muted">
                {summary.total} total | {summary.mkv} MKV | {summary.mp4} MP4 | {summary.failed} failed
              </div>
            </div>
          </div>

          <div className="mt-4 min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-panel">
            {files.length === 0 ? (
              <div className="flex h-full min-h-[13.75rem] flex-col items-center justify-center text-center">
                <div className="text-xl font-semibold">No files scanned yet</div>
                {/* The desktop browses the whole machine, so telling it to
                    mount something is advice for a container the user is not
                    running. */}
                <div className="mt-2 text-sm text-subtle">
                  {isDesktop
                    ? "Browse for a folder or file, then scan."
                    : "Mount media to /media, then scan."}
                </div>
              </div>
            ) : (
              <div className="h-full overflow-auto">
                <table className="w-full min-w-[68.75rem] border-collapse text-left text-sm">
                  <thead className="sticky top-0 bg-panel text-xs uppercase tracking-wide text-subtle">
                    <tr>
                      <th className="border-b border-border px-3 py-2 font-semibold">File</th>
                      <th className="border-b border-border px-3 py-2 font-semibold">Reader</th>
                      <th className="border-b border-border px-3 py-2 font-semibold">Codec</th>
                      <th className="border-b border-border px-3 py-2 font-semibold">Resolution</th>
                      <th className="border-b border-border px-3 py-2 font-semibold">Audio</th>
                      <th className="border-b border-border px-3 py-2 font-semibold">Subtitles</th>
                      <th className="border-b border-border px-3 py-2 font-semibold">Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {files.map((file) => {
                      const templateRow = isTemplate(file);
                      const mismatchRow = !templateRow && hasTemplateMismatch(file, templateFile);

                      return (
                        <tr
                          key={file.path}
                          onClick={() => setSelectedFilePath(file.path)}
                          onContextMenu={(event) => openFileContextMenu(event, file)}
                          className={[
                            "cursor-pointer bg-card hover:bg-selected",
                            selectedFile?.path === file.path ? "bg-selected" : "",
                            templateRow ? "text-accent" : mismatchRow ? "text-warning" : "text-text"
                          ].join(" ")}
                        >
                          <td className="max-w-[21.25rem] truncate border-b border-border px-3 py-2" title={file.path}>{file.fileName}</td>
                          <td className="border-b border-border px-3 py-2">{file.reader}</td>
                          <td className="border-b border-border px-3 py-2">{file.codec || "Unknown"}</td>
                          <td className="border-b border-border px-3 py-2">{file.resolution || "Unknown"}</td>
                          <td className="max-w-[15.625rem] truncate border-b border-border px-3 py-2" title={file.audioSummary}>{file.audioSummary || "None"}</td>
                          <td className="max-w-[15.625rem] truncate border-b border-border px-3 py-2" title={file.subtitleSummary}>{file.subtitleSummary || "None"}</td>
                          <td className={["border-b border-border px-3 py-2", !templateRow && !mismatchRow ? "text-success" : ""].join(" ")}>
                            {templateRow ? "Template" : mismatchRow ? "Warning" : file.status}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>

          {skipped.length > 0 ? (
            <div className="mt-3 max-h-20 shrink-0 overflow-auto rounded-md border border-warning bg-input p-3 text-xs text-warning">
              {skipped.map((line) => <div key={line}>{line}</div>)}
            </div>
          ) : null}

          {/* Both panels hold their place whether or not a file is selected.
              Showing them only after a scan made the dashboard reflow as files
              came and went, and an empty frame says what will appear here. */}
          <div className="mt-4 grid h-[26vh] min-h-[11.875rem] max-h-[15.625rem] min-w-0 shrink-0 grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)] gap-4">
              <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-panel p-4">
                <div className="flex items-center justify-between gap-3">
                  <h3 className="text-sm font-semibold">Media Info</h3>
                  {selectedFile ? (
                    <span className="rounded-md bg-input px-2 py-1 text-[0.6875rem] font-semibold uppercase tracking-wide text-subtle">
                      {selectedFile.reader}
                    </span>
                  ) : null}
                </div>
                <dl className="mt-3 grid min-h-0 flex-1 grid-cols-[6.875rem_1fr] gap-x-3 gap-y-2 overflow-auto text-sm">
                  {mediaInfoRows.map((row) => (
                    <Fragment key={row.label}>
                      <dt className="text-subtle">{row.label}</dt>
                      <dd className={row.className} title={row.title}>{row.value}</dd>
                    </Fragment>
                  ))}
                </dl>
              </section>

              <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-panel p-4">
                <h3 className={["text-sm font-semibold", selectedFile && !isTemplate(selectedFile) && normalizeTrackSignature(selectedFile) !== normalizeTrackSignature(templateFile ?? selectedFile) ? "text-warning" : ""].join(" ")}>Track Info</h3>
                <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-border">
                  <table className="w-full table-fixed border-collapse text-left text-xs">
                    <thead className="sticky top-0 bg-panel text-subtle">
                      <tr>
                        <th className="w-10 border-b border-border px-2 py-2">ID</th>
                        <th className="w-16 border-b border-border px-2 py-2">Type</th>
                        <th className="w-24 border-b border-border px-2 py-2">Codec</th>
                        <th className="w-14 border-b border-border px-2 py-2">Lang</th>
                        <th className="border-b border-border px-2 py-2">Name</th>
                        <th className="w-20 border-b border-border px-2 py-2">Flags</th>
                      </tr>
                    </thead>
                    <tbody>
                      {!selectedFile ? (
                        <tr>
                          <td colSpan={6} className="px-2 py-8 text-center text-subtle">Scan a folder, then select a file.</td>
                        </tr>
                      ) : selectedFile.tracks.length === 0 ? (
                        <tr>
                          <td colSpan={6} className="px-2 py-8 text-center text-subtle">No track data available.</td>
                        </tr>
                      ) : selectedFile.tracks.map((track, index) => (
                        <tr key={`${track.type}-${track.id}-${track.trackNumber}`} className="bg-card">
                          <td className={["truncate border-b border-border px-2 py-2", trackTextClass(index, (item) => `${item.id}-${item.trackNumber}`)].join(" ")}>{track.id}</td>
                          <td className={["truncate border-b border-border px-2 py-2 capitalize", trackTextClass(index, (item) => item.type)].join(" ")}>{track.type}</td>
                          <td className={["truncate border-b border-border px-2 py-2", trackTextClass(index, (item) => item.codec)].join(" ")} title={track.codec}>{track.codec || "Unknown"}</td>
                          <td className={["truncate border-b border-border px-2 py-2", trackTextClass(index, (item) => item.language)].join(" ")}>{track.language || "und"}</td>
                          <td className={["max-w-[13.75rem] truncate border-b border-border px-2 py-2", normalizeCompareValue(track.type) === "video" ? "text-text" : trackTextClass(index, (item) => item.name)].join(" ")} title={track.name}>{track.name || "-"}</td>
                          <td className={["truncate border-b border-border px-2 py-2", trackTextClass(index, (item) => `${item.default}-${item.forced}`), trackTextClass(index, (item) => `${item.default}-${item.forced}`) === "text-text" ? "text-subtle" : ""].join(" ")}>
                            {[track.default ? "Default" : "", track.forced ? "Forced" : ""].filter(Boolean).join(", ") || "-"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>
          </div>
        </section>
      </div>

      {isBrowseOpen ? (
        <FileBrowser
          initialPath={lastBrowsePath || sources[0] || defaultSourcePath}
          roots={browseRootOptions}
          onCancel={() => setIsBrowseOpen(false)}
          onSelect={addBrowsePath}
        />
      ) : null}
      {contextMenu ? (
        <FileContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          file={files.find((file) => file.path === contextMenu.path) ?? null}
          onSetTemplate={setContextFileAsTemplate}
          onCopyName={(file) => copyContextFileText(file, file.fileName, "File name")}
          onCopyPath={(file) => copyContextFileText(file, file.path, "Full path")}
          onRemove={removeContextFile}
        />
      ) : null}
    </div>
  );
}

function FileContextMenu({ x, y, file, onSetTemplate, onCopyName, onCopyPath, onRemove }: {
  x: number;
  y: number;
  file: MediaFileRow | null;
  onSetTemplate: (file: MediaFileRow) => void;
  onCopyName: (file: MediaFileRow) => void;
  onCopyPath: (file: MediaFileRow) => void;
  onRemove: (file: MediaFileRow) => void;
}) {
  if (!file) return null;

  return (
    <div
      className="fixed z-[60] w-56 overflow-hidden rounded-lg border border-border-strong bg-card py-1 shadow-[0_1.125rem_3.4375rem_rgba(0,0,0,0.45)]"
      style={{ left: Math.min(x, window.innerWidth - 240), top: Math.min(y, window.innerHeight - 180) }}
      onClick={(event) => event.stopPropagation()}
    >
      <ContextMenuButton icon={<FileCheck size={15} />} label="Set as Template" onClick={() => onSetTemplate(file)} />
      <ContextMenuButton icon={<Copy size={15} />} label="Copy File Name" onClick={() => onCopyName(file)} />
      <ContextMenuButton icon={<Copy size={15} />} label="Copy Full Path" onClick={() => onCopyPath(file)} />
      <div className="my-1 border-t border-border" />
      <ContextMenuButton icon={<Trash2 size={15} />} label="Remove from List" onClick={() => onRemove(file)} warning />
    </div>
  );
}

function ContextMenuButton({ icon, label, onClick, warning = false }: { icon: ReactNode; label: string; onClick: () => void; warning?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={["flex w-full items-center gap-3 px-3 py-2 text-left text-sm transition hover:bg-selected", warning ? "text-warning" : "text-muted hover:text-text"].join(" ")}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}

function getParentPath(path: string) {
  const clean = path.trim().replace(/[\\/]+$/, "");
  if (!clean || clean === "/") return "";

  const slash = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  if (slash < 0) return "";
  if (slash === 0) return clean.startsWith("/") ? "/" : "";
  return clean.slice(0, slash);
}

function normalizeCompareValue(value: string | null | undefined) {
  const clean = (value ?? "").trim();
  return clean.length === 0 ? "none" : clean.toLowerCase();
}

function hasTemplateMismatch(file: MediaFileRow, templateFile: MediaFileRow | null) {
  return getTemplateMismatchMessages(file, templateFile).length > 0;
}

function getTemplateMismatchMessages(file: MediaFileRow, templateFile: MediaFileRow | null) {
  if (!templateFile || file.path === templateFile.path) return [];

  const messages: string[] = [];
  addValueMismatch(messages, "Codec", file.codec, templateFile.codec);
  addValueMismatch(messages, "Resolution", file.resolution, templateFile.resolution);
  addValueMismatch(messages, "Bit depth", file.bitDepth, templateFile.bitDepth);
  addValueMismatch(messages, "Audio summary", file.audioSummary, templateFile.audioSummary);
  addValueMismatch(messages, "Subtitle summary", file.subtitleSummary, templateFile.subtitleSummary);

  const trackCount = Math.max(file.tracks.length, templateFile.tracks.length);
  for (let index = 0; index < trackCount; index += 1) {
    const track = file.tracks[index];
    const templateTrack = templateFile.tracks[index];
    const label = `Track ${index + 1}`;

    if (!track && templateTrack) {
      messages.push(`${label} is missing (template: ${formatCompareValue(templateTrack.type)}).`);
      continue;
    }
    if (track && !templateTrack) {
      messages.push(`${label} is extra (${formatCompareValue(track.type)}).`);
      continue;
    }
    if (!track || !templateTrack) continue;

    addValueMismatch(messages, `${label} ID`, `${track.id}/${track.trackNumber}`, `${templateTrack.id}/${templateTrack.trackNumber}`);
    addValueMismatch(messages, `${label} type`, track.type, templateTrack.type);
    addValueMismatch(messages, `${label} codec`, track.codec, templateTrack.codec);
    addValueMismatch(messages, `${label} language`, track.language, templateTrack.language);
    const isVideo = normalizeCompareValue(track.type) === "video" || normalizeCompareValue(templateTrack.type) === "video";
    if (!isVideo) addValueMismatch(messages, `${label} name`, track.name, templateTrack.name);
    addValueMismatch(messages, `${label} default flag`, track.default ? "Yes" : "No", templateTrack.default ? "Yes" : "No");
    addValueMismatch(messages, `${label} forced flag`, track.forced ? "Yes" : "No", templateTrack.forced ? "Yes" : "No");
  }

  return messages;
}

function addValueMismatch(messages: string[], label: string, value: string, templateValue: string) {
  if (normalizeCompareValue(value) === normalizeCompareValue(templateValue)) return;
  messages.push(`${label}: ${formatCompareValue(value)} (template: ${formatCompareValue(templateValue)}).`);
}

function formatCompareValue(value: string | null | undefined) {
  const clean = (value ?? "").trim();
  return clean || "None";
}

function normalizeTrackSignature(file: MediaFileRow) {
  return file.tracks
    .map((track) => {
      const isVideo = normalizeCompareValue(track.type) === "video";
      return [
        String(track.id),
        String(track.trackNumber),
        track.type,
        track.codec,
        track.language,
        isVideo ? "" : track.name,
        track.default ? "default" : "",
        track.forced ? "forced" : ""
      ].map(normalizeCompareValue).join("|");
    })
    .join(";");
}
