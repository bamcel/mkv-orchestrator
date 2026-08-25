import { createContext, type ReactNode, useContext, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getOperationJob, type OperationJobResponse } from "../api";

const storageKey = "mkvo.web.activeOperationJob";
const terminalStatuses = new Set(["Completed", "Failed", "Skipped", "Canceled"]);

type ActiveOperation = { id: string; label: string };

type OperationJobContextValue = {
  activeOperation: ActiveOperation | null;
  job: OperationJobResponse | undefined;
  isRunning: boolean;
  statusText: string | null;
  trackJob: (id: string, label: string) => void;
  clearJob: () => void;
};

const OperationJobContext = createContext<OperationJobContextValue | null>(null);

function readActiveOperation(): ActiveOperation | null {
  try {
    const stored = sessionStorage.getItem(storageKey);
    return stored ? JSON.parse(stored) as ActiveOperation : null;
  } catch {
    return null;
  }
}

export function OperationJobProvider({ children }: { children: ReactNode }) {
  const [activeOperation, setActiveOperation] = useState<ActiveOperation | null>(readActiveOperation);
  const jobQuery = useQuery({
    queryKey: ["operation-job", activeOperation?.id],
    queryFn: () => getOperationJob(activeOperation!.id),
    enabled: activeOperation !== null,
    refetchInterval: (query) => {
      const job = query.state.data;
      return job && terminalStatuses.has(job.status) ? false : 1000;
    }
  });
  const job = jobQuery.data;
  const isRunning = Boolean(activeOperation && (!job || !terminalStatuses.has(job.status)));
  const statusText = useMemo(() => {
    if (!activeOperation) return null;
    if (!job) return `${activeOperation.label}: checking job`;
    const finished = job.completed + job.failed + job.skipped;
    if (!terminalStatuses.has(job.status)) {
      const file = job.currentFile ? ` · ${job.currentFile} ${job.currentFilePercent}%` : "";
      return `${activeOperation.label}: ${finished}/${job.total}${file}`;
    }
    if (job.status === "Completed") return `${activeOperation.label}: completed ${finished}/${job.total}`;
    if (job.status === "Failed") return `${activeOperation.label}: failed`;
    return `${activeOperation.label}: ${job.status.toLowerCase()}`;
  }, [activeOperation, job]);

  const value = useMemo<OperationJobContextValue>(() => ({
    activeOperation,
    job,
    isRunning,
    statusText,
    trackJob: (id, label) => {
      const next = { id, label };
      setActiveOperation(next);
      try { sessionStorage.setItem(storageKey, JSON.stringify(next)); } catch { /* state still tracks it */ }
    },
    clearJob: () => {
      setActiveOperation(null);
      try { sessionStorage.removeItem(storageKey); } catch { /* state is already clear */ }
    }
  }), [activeOperation, job, isRunning, statusText]);

  return <OperationJobContext.Provider value={value}>{children}</OperationJobContext.Provider>;
}

export function useOperationJob() {
  const value = useContext(OperationJobContext);
  if (!value) throw new Error("useOperationJob must be used inside OperationJobProvider");
  return value;
}
