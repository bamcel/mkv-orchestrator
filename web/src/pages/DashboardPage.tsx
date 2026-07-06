import { type MouseEvent, type ReactNode, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronUp, Copy, FileCheck, FileVideo, Folder, FolderOpen, Plus, RefreshCw, Search, Trash2, X } from "lucide-react";
import { browseFileSystem, cancelScan, FileSystemEntry, getCurrentScanFiles, getScanJob, getStatus, MediaFileRow, startScan } from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

const defaultIgnored = "Extras, OVAs, backdrops, Specials";
const lastBrowsePathStorageKey = "mkvo.web.lastBrowsePath";

export function DashboardPage() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const { files, setFiles, templateFilePath, setTemplateFilePath } = useMediaLibrary();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [sources, setSources] = useState<string[]>([]);
  const [isBrowseOpen, setIsBrowseOpen] = useState(false);
  const [browsePath, setBrowsePath] = useState("");
  const [browsePathInput, setBrowsePathInput] = useState("");
  const [lastBrowsePath, setLastBrowsePath] = useState(() => {
    try {
      return window.localStorage.getItem(lastBrowsePathStorageKey) ?? "";
    } catch {
      return "";
    }
  });
  const [scanJobId, setScanJobId] = useState<string | null>(null);
  const [ignoredFolders, setIgnoredFolders] = useState(defaultIgnored);
  const [skipped, setSkipped] = useState<string[]>([]);
  const [selectedFilePath, setSelectedFilePath] = useState<string>("");
  const [actionStatus, setActionStatus] = useState("");
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; path: string } | null>(null);
  const browser = useQuery({
    queryKey: ["filesystem", browsePath],
    queryFn: () => browseFileSystem(browsePath || status.data?.mediaRoot || "/media"),
    enabled: isBrowseOpen
  });
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
      return job && ["Completed", "Failed", "Canceled"].includes(job.status) ? false : 1000;
    }
  });

  const defaultSourcePath = status.data?.mediaRoot || "/media";
  const browseRootOptions = useMemo(() => {
    const roots = [
      { name: "Media", path: defaultSourcePath },
      ...(status.data?.sourceRoots ?? [])
    ];
    const seen = new Set<string>();
    return roots.filter((root) => {
      const path = root.path.trim();
      if (!path || seen.has(path.toLowerCase())) return false;
      seen.add(path.toLowerCase());
      return true;
    });
  }, [defaultSourcePath, status.data?.sourceRoots]);
  const activeSources = sources;
  const hasSources = sources.length > 0;
  const sourceSummary = sources.length === 0
    ? "No source selected"
    : sources.length === 1
      ? sources[0]
      : `${sources.length} sources selected`;
  const currentScanJob = scanJob.data;
  const isScanning = scanStart.isPending || currentScanJob?.status === "Queued" || currentScanJob?.status === "Running" || currentScanJob?.status === "Canceling";
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
    if (!isBrowseOpen) return;
    setBrowsePathInput(browser.data?.path ?? browsePath);
  }, [isBrowseOpen, browser.data?.path, browsePath]);

  useEffect(() => {
    if (files.length > 0 || !currentScan.data?.files.length) return;
    setFiles(currentScan.data.files);
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
    const nextPath = lastBrowsePath || sources[0] || defaultSourcePath;
    setBrowsePath(nextPath);
    setBrowsePathInput(nextPath);
    setIsBrowseOpen(true);
  }

  function navigateBrowsePath(path: string, remember = true) {
    const cleanPath = path.trim();
    if (!cleanPath) return;

    setBrowsePath(cleanPath);
    setBrowsePathInput(cleanPath);
    if (remember) rememberBrowsePath(cleanPath);
  }

  function goBrowseParent() {
    const parent = browser.data?.parentPath || getParentPath(browsePath);
    if (!parent) return;
    navigateBrowsePath(parent);
  }

  function addBrowsePath(path: string, kind: FileSystemEntry["kind"] = "folder") {
    addSource(path);
    rememberBrowsePath(kind === "file" ? browser.data?.path ?? browsePath : path);
    setIsBrowseOpen(false);
  }

  function openEntry(entry: FileSystemEntry) {
    if (entry.kind === "folder") {
      setBrowsePath(entry.path);
      rememberBrowsePath(entry.path);
      return;
    }

    addBrowsePath(entry.path, "file");
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
      <div className="grid min-h-0 flex-1 grid-cols-[370px_1fr] gap-5">
        <section className="min-h-0 overflow-auto rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
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
              className="inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              <FolderOpen size={15} />
              Browse
            </button>
            <button
              onClick={runScan}
              disabled={isScanning || !hasSources}
              className="inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:bg-button disabled:text-disabled"
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
            onChange={(event) => setIgnoredFolders(event.target.value)}
            rows={4}
            className="mt-2 w-full resize-none rounded-md border border-border bg-input px-3 py-2 text-sm text-text outline-none placeholder:text-subtle transition focus:border-accent"
          />

          <div className="mt-4 flex gap-2">
            <button
              onClick={() => status.refetch()}
              className="inline-flex h-10 items-center gap-2 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              <RefreshCw size={15} />
              Refresh
            </button>
          </div>

          <div className="mt-4 text-sm text-success">{dashboardStatus}</div>
          {currentScanJob?.currentSource && isScanning ? (
            <div className="mt-2 truncate text-xs text-subtle" title={currentScanJob.currentSource}>
              {currentScanJob.currentSource}
            </div>
          ) : null}
          {scanStart.error ? <div className="mt-3 rounded-md border border-warning bg-input p-3 text-xs text-warning">{String(scanStart.error.message)}</div> : null}
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex shrink-0 items-center justify-between">
            <div>
              <h2 className="text-base font-semibold">File Info</h2>
              <div className="mt-1 max-w-[720px] truncate text-xs text-muted" title={templateFile?.path}>
                Template: {templateFile?.fileName ?? "None selected"}
              </div>
            </div>
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={useSelectedAsTemplate}
                disabled={!selectedFile}
                className="rounded-md border border-border bg-button px-3 py-2 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
              >
                Use Selected as Template
              </button>
              <div className="text-xs text-muted">
                {summary.total} total | {summary.mkv} MKV | {summary.mp4} MP4 | {summary.failed} failed
              </div>
            </div>
          </div>

          <div className="mt-4 min-h-0 flex-1 overflow-hidden rounded-lg border border-border bg-panel">
            {files.length === 0 ? (
              <div className="flex h-full min-h-[220px] flex-col items-center justify-center text-center">
                <div className="text-xl font-semibold">No files scanned yet</div>
                <div className="mt-2 text-sm text-subtle">Mount media to /media, then scan.</div>
              </div>
            ) : (
              <div className="h-full overflow-auto">
                <table className="w-full min-w-[1100px] border-collapse text-left text-sm">
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
                    {files.map((file) => (
                      <tr
                        key={file.path}
                        onClick={() => setSelectedFilePath(file.path)}
                        onContextMenu={(event) => openFileContextMenu(event, file)}
                        className={[
                          "cursor-pointer bg-card hover:bg-selected",
                          selectedFile?.path === file.path ? "bg-selected" : ""
                        ].join(" ")}
                      >
                        <td className={[
                          "max-w-[340px] truncate border-b border-border px-3 py-2",
                          isTemplate(file) ? "text-accent" : "text-text"
                        ].join(" ")} title={file.path}>
                          {file.fileName}
                        </td>
                        <td className="border-b border-border px-3 py-2 text-muted">{file.reader}</td>
                        <td className={["border-b border-border px-3 py-2", compareTextClass(file, (row) => row.codec)].join(" ")}>{file.codec || "Unknown"}</td>
                        <td className={["border-b border-border px-3 py-2", compareTextClass(file, (row) => row.resolution)].join(" ")}>{file.resolution || "Unknown"}</td>
                        <td className={["max-w-[250px] truncate border-b border-border px-3 py-2", compareTextClass(file, (row) => row.audioSummary)].join(" ")} title={file.audioSummary}>{file.audioSummary || "None"}</td>
                        <td className={["max-w-[250px] truncate border-b border-border px-3 py-2", compareTextClass(file, (row) => row.subtitleSummary)].join(" ")} title={file.subtitleSummary}>{file.subtitleSummary || "None"}</td>
                        <td className={["border-b border-border px-3 py-2", isTemplate(file) ? "text-accent" : hasTemplateMismatch(file, templateFile) ? "text-warning" : "text-success"].join(" ")}>
                          {isTemplate(file) ? "Template" : hasTemplateMismatch(file, templateFile) ? "Warning" : file.status}
                        </td>
                      </tr>
                    ))}
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

          {selectedFile ? (
            <div className="mt-4 grid h-[26vh] min-h-[190px] max-h-[250px] shrink-0 grid-cols-2 gap-4">
              <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-panel p-4">
                <div className="flex items-center justify-between gap-3">
                  <h3 className="text-sm font-semibold">Media Info</h3>
                  <span className="rounded-md bg-input px-2 py-1 text-[11px] font-semibold uppercase tracking-wide text-subtle">
                    {selectedFile.reader}
                  </span>
                </div>
                <dl className="mt-3 grid min-h-0 flex-1 grid-cols-[110px_1fr] gap-x-3 gap-y-2 overflow-auto text-sm">
                  <dt className="text-subtle">File</dt>
                  <dd className="truncate text-text" title={selectedFile.path}>{selectedFile.fileName}</dd>
                  <dt className="text-subtle">Codec</dt>
                  <dd className={compareTextClass(selectedFile, (row) => row.codec)}>{selectedFile.codec || "Unknown"}</dd>
                  <dt className="text-subtle">Resolution</dt>
                  <dd className={compareTextClass(selectedFile, (row) => row.resolution)}>{selectedFile.resolution || "Unknown"}</dd>
                  <dt className="text-subtle">Bit Depth</dt>
                  <dd className={compareTextClass(selectedFile, (row) => row.bitDepth)}>{selectedFile.bitDepth || "Unknown"}</dd>
                  <dt className="text-subtle">HDR</dt>
                  <dd className={compareTextClass(selectedFile, (row) => row.hdr)}>{selectedFile.hdr || "None"}</dd>
                  <dt className="text-subtle">Audio</dt>
                  <dd className={["truncate", compareTextClass(selectedFile, (row) => row.audioSummary)].join(" ")} title={selectedFile.audioSummary}>{selectedFile.audioSummary || "None"}</dd>
                  <dt className="text-subtle">Subtitles</dt>
                  <dd className={["truncate", compareTextClass(selectedFile, (row) => row.subtitleSummary)].join(" ")} title={selectedFile.subtitleSummary}>{selectedFile.subtitleSummary || "None"}</dd>
                  <dt className="text-subtle">Status</dt>
                  <dd className={isTemplate(selectedFile) ? "text-accent" : hasTemplateMismatch(selectedFile, templateFile) ? "text-warning" : "text-success"}>
                    {isTemplate(selectedFile) ? "Template" : hasTemplateMismatch(selectedFile, templateFile) ? "Warning" : selectedFile.status}
                  </dd>
                </dl>
              </section>

              <section className="flex min-h-0 min-w-0 flex-col rounded-lg border border-border bg-panel p-4">
                <h3 className={["text-sm font-semibold", selectedFile && !isTemplate(selectedFile) && normalizeTrackSignature(selectedFile) !== normalizeTrackSignature(templateFile ?? selectedFile) ? "text-warning" : ""].join(" ")}>Track Info</h3>
                <div className="mt-3 min-h-0 flex-1 overflow-auto rounded-md border border-border">
                  <table className="w-full border-collapse text-left text-xs">
                    <thead className="sticky top-0 bg-panel text-subtle">
                      <tr>
                        <th className="border-b border-border px-2 py-2">ID</th>
                        <th className="border-b border-border px-2 py-2">Type</th>
                        <th className="border-b border-border px-2 py-2">Codec</th>
                        <th className="border-b border-border px-2 py-2">Lang</th>
                        <th className="border-b border-border px-2 py-2">Name</th>
                        <th className="border-b border-border px-2 py-2">Flags</th>
                      </tr>
                    </thead>
                    <tbody>
                      {selectedFile.tracks.length === 0 ? (
                        <tr>
                          <td colSpan={6} className="px-2 py-8 text-center text-subtle">No track data available.</td>
                        </tr>
                      ) : selectedFile.tracks.map((track, index) => (
                        <tr key={`${track.type}-${track.id}-${track.trackNumber}`} className="bg-card">
                          <td className="border-b border-border px-2 py-2">{track.id}</td>
                          <td className={["border-b border-border px-2 py-2 capitalize", trackTextClass(index, (item) => item.type)].join(" ")}>{track.type}</td>
                          <td className={["border-b border-border px-2 py-2", trackTextClass(index, (item) => item.codec)].join(" ")}>{track.codec || "Unknown"}</td>
                          <td className={["border-b border-border px-2 py-2", trackTextClass(index, (item) => item.language)].join(" ")}>{track.language || "und"}</td>
                          <td className={["max-w-[220px] truncate border-b border-border px-2 py-2", trackTextClass(index, (item) => item.name)].join(" ")} title={track.name}>{track.name || "-"}</td>
                          <td className={["border-b border-border px-2 py-2", trackTextClass(index, (item) => `${item.default}-${item.forced}`), trackTextClass(index, (item) => `${item.default}-${item.forced}`) === "text-text" ? "text-subtle" : ""].join(" ")}>
                            {[track.default ? "Default" : "", track.forced ? "Forced" : ""].filter(Boolean).join(", ") || "-"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>
            </div>
          ) : null}
        </section>
      </div>

      {isBrowseOpen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
          <section className="flex h-[82vh] min-h-[620px] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-[0_24px_80px_rgba(0,0,0,0.45)]">
            <div className="flex h-[82px] shrink-0 items-center justify-between border-b border-border px-5 py-4">
              <div className="min-w-0">
                <h2 className="text-base font-semibold">Browse Media Sources</h2>
                <div className="mt-1 max-w-[620px] truncate font-mono text-xs text-subtle" title={browser.data?.path ?? browsePath}>
                  {browser.data?.path ?? browsePath}
                </div>
              </div>
              <button
                type="button"
                onClick={() => setIsBrowseOpen(false)}
                className="rounded-md p-2 text-muted transition hover:bg-button-hover hover:text-text"
                aria-label="Close browser"
              >
                <X size={18} />
              </button>
            </div>

            <div className="shrink-0 space-y-2 border-b border-border px-5 py-3">
              <div className="flex items-center gap-2">
                <input
                  value={browsePathInput}
                  onChange={(event) => setBrowsePathInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") navigateBrowsePath(browsePathInput);
                  }}
                  className="h-9 min-w-0 flex-1 rounded-md border border-border bg-input px-3 font-mono text-xs text-text outline-none transition focus:border-accent"
                />
                <button
                  type="button"
                  onClick={() => navigateBrowsePath(browsePathInput)}
                  className="h-9 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                >
                  Go
                </button>
                <button
                  type="button"
                  onClick={goBrowseParent}
                  disabled={!browser.data?.parentPath && !getParentPath(browsePath)}
                  className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
                >
                  <ChevronUp size={15} />
                  Up
                </button>
              </div>

              <div className="flex items-center gap-2 overflow-x-auto">
                {browseRootOptions.map((root) => (
                  <button
                    key={root.path}
                    type="button"
                    onClick={() => navigateBrowsePath(root.path)}
                    className="h-8 shrink-0 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                    title={root.path}
                  >
                    {root.name}
                  </button>
                ))}
                <div className="min-w-2 flex-1" />
                <button
                  type="button"
                  onClick={() => addBrowsePath(browser.data?.path ?? browsePath, "folder")}
                  className="inline-flex h-8 shrink-0 items-center gap-2 rounded-md bg-accent px-3 text-xs font-semibold text-window transition hover:bg-accent-hover"
                >
                  <Plus size={14} />
                  Add This Folder
                </button>
                <button
                  type="button"
                  onClick={() => browser.refetch()}
                  className="inline-flex h-8 shrink-0 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                >
                  <RefreshCw size={14} />
                  Refresh
                </button>
              </div>
            </div>

            <div className="min-h-0 flex-1 overflow-auto p-4">
              {browser.isLoading ? (
                <div className="flex h-full items-center justify-center text-sm text-muted">loading directory</div>
              ) : browser.error ? (
                <div className="rounded-md border border-warning bg-input p-3 text-sm text-warning">{String(browser.error.message)}</div>
              ) : (browser.data?.entries.length ?? 0) === 0 ? (
                <div className="flex h-full items-center justify-center text-sm text-muted">No folders or media files found.</div>
              ) : (
                <div className="overflow-hidden rounded-lg border border-border">
                  <table className="w-full table-fixed border-collapse text-left text-sm">
                    <tbody>
                      {browser.data?.entries.map((entry) => (
                        <tr key={entry.path} className="bg-card hover:bg-selected">
                          <td className="min-w-0 border-b border-border px-3 py-2">
                            <button
                              type="button"
                              onClick={() => openEntry(entry)}
                              className="flex w-full items-center gap-3 text-left text-text"
                            >
                              {entry.kind === "folder"
                                ? <Folder size={16} className="shrink-0 text-accent" />
                                : <FileVideo size={16} className="shrink-0 text-muted" />}
                              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
                            </button>
                          </td>
                          <td className="w-24 border-b border-border px-3 py-2 text-xs uppercase tracking-wide text-subtle">
                            {entry.kind}
                          </td>
                          <td className="w-24 border-b border-border px-3 py-2 text-right">
                            <button
                              type="button"
                              onClick={() => addBrowsePath(entry.path, entry.kind)}
                              className="rounded-md border border-border bg-button px-2 py-1 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                            >
                              Add
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          </section>
        </div>
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
      className="fixed z-[60] w-56 overflow-hidden rounded-lg border border-border-strong bg-card py-1 shadow-[0_18px_55px_rgba(0,0,0,0.45)]"
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
  if (!templateFile || file.path === templateFile.path) return false;

  return normalizeCompareValue(file.codec) !== normalizeCompareValue(templateFile.codec)
    || normalizeCompareValue(file.resolution) !== normalizeCompareValue(templateFile.resolution)
    || normalizeCompareValue(file.bitDepth) !== normalizeCompareValue(templateFile.bitDepth)
    || normalizeCompareValue(file.audioSummary) !== normalizeCompareValue(templateFile.audioSummary)
    || normalizeCompareValue(file.subtitleSummary) !== normalizeCompareValue(templateFile.subtitleSummary)
    || normalizeTrackSignature(file) !== normalizeTrackSignature(templateFile);
}

function normalizeTrackSignature(file: MediaFileRow) {
  return file.tracks
    .filter((track) => track.type === "audio" || track.type === "subtitles")
    .map((track) => [
      track.type,
      track.codec,
      track.language,
      track.name,
      track.default ? "default" : "",
      track.forced ? "forced" : ""
    ].map(normalizeCompareValue).join("|"))
    .join(";");
}
