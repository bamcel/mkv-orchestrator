import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, SearchCheck } from "lucide-react";
import { buildLibraryAudit, getCurrentScanFiles, LibraryAuditResponse, LibraryAuditRow } from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { useMediaLibrary } from "../state/MediaLibraryContext";

export function LibraryPage() {
  const { files, setFiles, setTemplateFilePath } = useMediaLibrary();
  const currentScan = useQuery({ queryKey: ["current-scan-files"], queryFn: getCurrentScanFiles });
  const [auditResult, setAuditResult] = useState<LibraryAuditResponse | null>(null);
  const [selectedFolder, setSelectedFolder] = useState("");
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

  const selected = useMemo(() => {
    return auditResult?.items.find((item) => item.folderPath === selectedFolder) ?? auditResult?.items[0] ?? null;
  }, [auditResult, selectedFolder]);

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

  function useTemplate(row: LibraryAuditRow) {
    setTemplateFilePath(row.templateFilePath);
    setStatusText(`Template file set: ${row.templateFileName}`);
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Library" description="Build an overview of scanned folders and highlight metadata mismatches." />
      <div className="grid min-h-0 flex-1 grid-cols-[370px_1fr] gap-5">
        <section className="min-h-0 overflow-auto rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Library Build Overview</h2>
          <div className="mt-4 grid grid-cols-2 gap-2">
            <button onClick={refreshFiles} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-border bg-button text-sm font-semibold text-muted hover:bg-button-hover hover:text-text">
              <RefreshCw size={15} />
              Refresh Files
            </button>
            <button onClick={runAudit} disabled={audit.isPending || files.length === 0} className="inline-flex h-10 items-center justify-center gap-2 rounded-md bg-accent text-sm font-semibold text-window hover:bg-accent-hover disabled:bg-button disabled:text-disabled">
              {audit.isPending ? <RefreshCw size={15} className="animate-spin" /> : <SearchCheck size={15} />}
              Build Overview
            </button>
          </div>

          <div className="mt-5 grid grid-cols-2 gap-3">
            <Metric label="Files" value={auditResult?.summary.files ?? files.length} />
            <Metric label="Groups" value={auditResult?.summary.groups ?? 0} />
            <Metric label="Standard" value={auditResult?.summary.standardGroups ?? 0} />
            <Metric label="Warnings" value={auditResult?.summary.issueGroups ?? 0} warning />
          </div>

          <div className="mt-5 text-sm text-success">{statusText}</div>
          <div className="mt-3 rounded-lg border border-border bg-panel p-3 text-xs leading-5 text-muted">
            Library beta uses the current Dashboard scan as the source of truth. Scan the folder you want to review, then build the overview here.
          </div>
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Folder Audit</h2>
          <div className="mt-4 grid min-h-0 flex-1 grid-cols-[minmax(430px,1fr)_minmax(360px,0.8fr)] gap-4">
            <div className="min-h-0 overflow-auto rounded-lg border border-border bg-panel">
              {auditResult?.items.length ? (
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
                    {auditResult.items.map((item) => (
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
                <div className="flex h-full min-h-[320px] items-center justify-center text-sm text-subtle">Build an overview to inspect scanned folders.</div>
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
