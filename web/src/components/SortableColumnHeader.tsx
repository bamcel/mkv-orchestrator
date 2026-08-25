import { ArrowUpDown, ChevronDown, ChevronUp } from "lucide-react";
import type { ReactNode } from "react";

export type SortDirection = "asc" | "desc";

export function SortableColumnHeader({ active, direction, label, onSort, className = "" }: {
  active: boolean;
  direction: SortDirection;
  label: ReactNode;
  onSort: () => void;
  className?: string;
}) {
  const Icon = active ? (direction === "asc" ? ChevronUp : ChevronDown) : ArrowUpDown;
  const text = typeof label === "string" ? label : "column";
  return (
    <th className={["border-b border-border px-3 py-2 font-semibold", className].join(" ")} aria-sort={active ? (direction === "asc" ? "ascending" : "descending") : "none"}>
      <button type="button" onClick={onSort} className="inline-flex w-full items-center gap-1.5 text-left hover:text-text focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent" aria-label={`Sort by ${text}`}>
        <span>{label}</span><Icon size={13} aria-hidden="true" />
      </button>
    </th>
  );
}
