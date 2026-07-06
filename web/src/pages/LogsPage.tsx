import { useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Copy, Download, RefreshCw, Trash2 } from "lucide-react";
import { clearOperationLogs, getOperationLogs, OperationLogEntry } from "../api";
import { SectionHeader } from "../components/SectionHeader";

export function LogsPage() {
  const [autoRefresh, setAutoRefresh] = useState(true);
  const logs = useQuery({ queryKey: ["operation-logs"], queryFn: getOperationLogs, refetchInterval: autoRefresh ? 5000 : false });
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [areaFilter, setAreaFilter] = useState("All");
  const [statusText, setStatusText] = useState("");
  const clear = useMutation({
    mutationFn: clearOperationLogs,
    onSuccess: () => {
      setSelectedIndex(0);
      setStatusText("Logs cleared.");
      logs.refetch();
    }
  });
  const entries = logs.data?.entries ?? [];
  const areas = useMemo(() => ["All", ...Array.from(new Set(entries.map((entry) => entry.area))).sort()], [entries]);
  const visibleEntries = useMemo(() => {
    return areaFilter === "All" ? entries : entries.filter((entry) => entry.area === areaFilter);
  }, [areaFilter, entries]);
  const selected = useMemo<OperationLogEntry | null>(() => visibleEntries[selectedIndex] ?? visibleEntries[0] ?? null, [visibleEntries, selectedIndex]);

  async function copySelectedOutput() {
    if (!selected) return;

    await navigator.clipboard.writeText(formatLogOutput(selected));
    setStatusText("Selected log output copied.");
  }

  function downloadSelectedOutput() {
    if (!selected) return;

    const blob = new Blob([formatLogOutput(selected)], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `mkvo-${selected.area.toLowerCase()}-${safeDateStamp(selected.timestampUtc)}.log`;
    anchor.click();
    URL.revokeObjectURL(url);
    setStatusText("Selected log output downloaded.");
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Logs" description="Review recent scan and operation output." />
      <div className="grid min-h-0 flex-1 grid-cols-[430px_1fr] gap-5">
        <section className="flex min-h-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex items-center justify-between">
            <h2 className="text-base font-semibold">Recent Operations</h2>
            <div className="flex gap-2">
              <button onClick={() => logs.refetch()} className="rounded-md border border-border bg-button p-2 text-muted hover:bg-button-hover hover:text-text" title="Refresh">
                <RefreshCw size={15} />
              </button>
              <button onClick={() => clear.mutate()} className="rounded-md border border-border bg-button p-2 text-muted hover:bg-button-hover hover:text-text" title="Clear">
                <Trash2 size={15} />
              </button>
            </div>
          </div>

          <div className="mt-4 grid grid-cols-[1fr_auto] gap-2">
            <label className="block">
              <span className="text-xs font-semibold text-muted">Area</span>
              <select
                value={areaFilter}
                onChange={(event) => {
                  setAreaFilter(event.target.value);
                  setSelectedIndex(0);
                }}
                className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
              >
                {areas.map((area) => <option key={area} value={area}>{area}</option>)}
              </select>
            </label>
            <label className="mt-6 flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-sm text-muted">
              <input type="checkbox" checked={autoRefresh} onChange={(event) => setAutoRefresh(event.target.checked)} />
              Auto
            </label>
          </div>

          <div className="mt-4 min-h-0 flex-1 overflow-auto rounded-lg border border-border bg-panel">
            {visibleEntries.length === 0 ? (
              <div className="flex h-full min-h-[300px] items-center justify-center text-sm text-subtle">No operation logs yet.</div>
            ) : visibleEntries.map((entry, index) => (
              <button
                key={`${entry.timestampUtc}-${entry.area}-${index}`}
                onClick={() => setSelectedIndex(index)}
                className={["block w-full border-b border-border px-3 py-3 text-left hover:bg-selected", selected === entry ? "bg-selected" : "bg-card"].join(" ")}
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="font-semibold text-sm">{entry.area}</span>
                  <span className="shrink-0 text-xs text-subtle">{formatDate(entry.timestampUtc)}</span>
                </div>
                <div className="mt-1 truncate text-sm text-muted">{entry.message}</div>
              </button>
            ))}
          </div>
          <div className="mt-3 text-xs text-muted">
            {visibleEntries.length} shown | {entries.length} total {statusText ? <span className="text-success">| {statusText}</span> : null}
          </div>
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <div className="flex items-center justify-between gap-3">
            <h2 className="text-base font-semibold">Output</h2>
            <div className="flex gap-2">
              <button onClick={copySelectedOutput} disabled={!selected} className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled" title="Copy output">
                <Copy size={14} />
                Copy
              </button>
              <button onClick={downloadSelectedOutput} disabled={!selected} className="inline-flex h-8 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted hover:bg-button-hover hover:text-text disabled:text-disabled" title="Download output">
                <Download size={14} />
                Download
              </button>
            </div>
          </div>
          {selected ? (
            <>
              <div className="mt-4 rounded-lg border border-border bg-panel p-4">
                <div className="text-xs font-semibold uppercase tracking-wide text-subtle">{selected.area}</div>
                <div className="mt-1 text-sm text-success">{selected.message}</div>
                <div className="mt-1 text-xs text-muted">{formatDate(selected.timestampUtc)}</div>
              </div>
              <pre className="mt-4 min-h-0 flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-border bg-input p-4 font-mono text-xs leading-6 text-muted">
                {selected.detail || "No detailed output was captured for this operation."}
              </pre>
            </>
          ) : (
            <div className="mt-4 flex min-h-[300px] items-center justify-center rounded-lg border border-border bg-panel text-sm text-subtle">
              Select an operation to inspect output.
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function safeDateStamp(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "unknown";
  return date.toISOString().replace(/[:.]/g, "-");
}

function formatLogOutput(entry: OperationLogEntry) {
  return [
    `Area: ${entry.area}`,
    `Time: ${formatDate(entry.timestampUtc)}`,
    `Message: ${entry.message}`,
    "",
    entry.detail || "No detailed output was captured for this operation."
  ].join("\n");
}
