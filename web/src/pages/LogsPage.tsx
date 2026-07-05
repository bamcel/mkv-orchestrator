import { useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { RefreshCw, Trash2 } from "lucide-react";
import { clearOperationLogs, getOperationLogs, OperationLogEntry } from "../api";
import { SectionHeader } from "../components/SectionHeader";

export function LogsPage() {
  const logs = useQuery({ queryKey: ["operation-logs"], queryFn: getOperationLogs, refetchInterval: 5000 });
  const [selectedIndex, setSelectedIndex] = useState(0);
  const clear = useMutation({
    mutationFn: clearOperationLogs,
    onSuccess: () => {
      setSelectedIndex(0);
      logs.refetch();
    }
  });
  const entries = logs.data?.entries ?? [];
  const selected = useMemo<OperationLogEntry | null>(() => entries[selectedIndex] ?? entries[0] ?? null, [entries, selectedIndex]);

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

          <div className="mt-4 min-h-0 flex-1 overflow-auto rounded-lg border border-border bg-panel">
            {entries.length === 0 ? (
              <div className="flex h-full min-h-[300px] items-center justify-center text-sm text-subtle">No operation logs yet.</div>
            ) : entries.map((entry, index) => (
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
        </section>

        <section className="flex min-h-0 min-w-0 flex-col rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
          <h2 className="text-base font-semibold">Output</h2>
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
