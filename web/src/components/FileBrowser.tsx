import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Check,
  ChevronRight,
  Folder,
  FolderOpen,
  FileVideo,
  HardDrive,
  House,
  Monitor,
  Network,
  Plus,
  Pin,
  RefreshCw,
  Search,
  X
} from "lucide-react";

import { browseFileSystem, FileSystemEntry } from "../api";

export type FileBrowserRoot = {
  name: string;
  path: string;
};

type FileBrowserProps = {
  /** Where to start. Empty string opens the volume list. */
  initialPath: string;
  /** Configured libraries, shown under Quick access. */
  roots: FileBrowserRoot[];
  /** The Settings default directory, shown separately above shortcuts. */
  homeRoot?: FileBrowserRoot;
  onCancel: () => void;
  /** Called with the chosen folder or media file. */
  onSelect: (path: string, kind: "folder" | "file", browsePath?: string) => void;
  /** Enables desktop-style multi-selection and submits all chosen entries. */
  onSelectMany?: (entries: Array<{ path: string; kind: "folder" | "file" }>, browsePath?: string) => void | Promise<void>;
  /** Persist a folder as a named Quick Access shortcut. */
  onPinToQuickAccess?: (path: string, name: string) => void | Promise<void>;
  /** Remove a user-pinned Quick Access shortcut. */
  onUnpinFromQuickAccess?: (path: string, name: string) => void | Promise<void>;
  /** Roots owned by the user; host-provided roots remain non-removable. */
  removableRootPaths?: string[];
};

type SortColumn = "name" | "modified" | "type" | "size";
type SortDirection = "asc" | "desc";

/** Compare paths the way the host does: separators and case do not distinguish. */
function samePath(left: string, right: string): boolean {
  return left.replace(/\\/g, "/").toLowerCase() === right.replace(/\\/g, "/").toLowerCase();
}

function parentOf(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  // Above a drive root is the volume list, which is addressed as "".
  if (cut <= 0) return "";
  const parent = trimmed.slice(0, cut);
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

/** Path split into clickable segments, each carrying the path it navigates to. */
function breadcrumbs(path: string): Array<{ label: string; path: string }> {
  if (!path) return [];
  const separator = path.includes("\\") ? "\\" : "/";
  const isUnc = path.startsWith("\\\\");
  const parts = path.split(/[\\/]+/).filter(Boolean);

  const crumbs: Array<{ label: string; path: string }> = [];
  let accumulated = "";
  parts.forEach((part, index) => {
    if (index === 0) {
      accumulated = isUnc ? `\\\\${part}` : /^[A-Za-z]:$/.test(part) ? `${part}\\` : `${separator}${part}`;
    } else {
      accumulated = accumulated.endsWith(separator)
        ? `${accumulated}${part}`
        : `${accumulated}${separator}${part}`;
    }
    crumbs.push({ label: part, path: accumulated });
  });
  return crumbs;
}

// Optional on the wire: a folder has no size, and the field may be
// absent rather than null.
function formatSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

function formatModified(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  // A network share has no timestamp of its own and is reported as the epoch.
  // Rendering that as "12/31/1969" would be worse than showing nothing.
  if (date.getTime() === 0) return "";
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit"
  });
}

const networkStorageKey = "mkvo.web.networkLocations";

/**
 * The `\\server` root of a UNC path, or null when the path is local.
 *
 * `\\?\` and `\\.\` also begin with two separators but address a local device,
 * so a leading `?` or `.` is not a server name.
 */
function networkRoot(path: string): string | null {
  const match = /^[\\/]{2}([^\\/?.][^\\/]*)/.exec(path.trim());
  return match ? `\\\\${match[1]}` : null;
}

function readNetworkLocations(): string[] {
  try {
    const stored: unknown = JSON.parse(window.localStorage.getItem(networkStorageKey) ?? "[]");
    return Array.isArray(stored) ? stored.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
}

function describeType(entry: FileSystemEntry): string {
  if (entry.kind === "folder") return "File folder";
  const extension = entry.name.includes(".") ? entry.name.split(".").pop() ?? "" : "";
  return extension ? `${extension.toUpperCase()} file` : "File";
}

export function FileBrowser({
  initialPath,
  roots,
  homeRoot,
  onCancel,
  onSelect,
  onSelectMany,
  onPinToQuickAccess,
  onUnpinFromQuickAccess,
  removableRootPaths = []
}: FileBrowserProps) {
  // A single history array with a cursor gives Back and Forward the same
  // meaning they have in a file manager, including forward being discarded
  // once you navigate somewhere new.
  const [history, setHistory] = useState<string[]>([initialPath]);
  const [cursor, setCursor] = useState(0);
  const path = history[cursor] ?? "";

  const [editingPath, setEditingPath] = useState(false);
  const [pathDraft, setPathDraft] = useState(path);
  const [filter, setFilter] = useState("");
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  const [activePath, setActivePath] = useState("");
  const [selectionAnchorPath, setSelectionAnchorPath] = useState("");
  const [folderMenu, setFolderMenu] = useState<{ x: number; y: number; entry: FileSystemEntry } | null>(null);
  const [sortColumn, setSortColumn] = useState<SortColumn>("name");
  const [sortDirection, setSortDirection] = useState<SortDirection>("asc");
  // Servers are remembered rather than discovered: enumerating hosts on the
  // network is slow and unreliable, and a NAS the user has reached once is the
  // one they want again.
  const [networkLocations, setNetworkLocations] = useState<string[]>(readNetworkLocations);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!folderMenu) return;
    const close = () => setFolderMenu(null);
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [folderMenu]);

  const listing = useQuery({
    queryKey: ["file-browser", path],
    queryFn: () => browseFileSystem(path)
  });

  const volumes = useQuery({
    queryKey: ["file-browser", "volumes"],
    queryFn: () => browseFileSystem(""),
    staleTime: 60_000
  });
  // Only a host that allows unrestricted browsing answers the empty path with
  // the volume list; a confined one falls back to its configured root. The
  // empty response path is what distinguishes them, so "This PC" is shown only
  // where it is real rather than mislabelling a fallback listing.
  const hasVolumeList = volumes.data?.path === "";

  const navigate = useCallback((next: string) => {
    setHistory((current) => [...current.slice(0, cursor + 1), next]);
    setCursor((current) => current + 1);
    setSelectedPaths([]);
    setActivePath("");
    setSelectionAnchorPath("");
    setFilter("");
    setEditingPath(false);
  }, [cursor]);

  const goUp = useCallback(() => {
    const parent = listing.data?.parentPath ?? parentOf(path);
    if (parent !== path) navigate(parent);
  }, [listing.data?.parentPath, navigate, path]);

  useEffect(() => setPathDraft(listing.data?.path ?? path), [listing.data?.path, path]);

  // Remembering only on a successful listing keeps typos and unreachable hosts
  // out of the sidebar.
  const reachedPath = listing.data?.path;
  useEffect(() => {
    const server = reachedPath ? networkRoot(reachedPath) : null;
    if (!server) return;
    setNetworkLocations((current) => {
      if (current.some((item) => samePath(item, server))) return current;
      const next = [...current, server].sort((left, right) => left.localeCompare(right));
      try {
        window.localStorage.setItem(networkStorageKey, JSON.stringify(next));
      } catch {
        // Ignore storage failures; browsing should still work.
      }
      return next;
    });
  }, [reachedPath]);

  function forgetNetworkLocation(server: string) {
    setNetworkLocations((current) => {
      const next = current.filter((item) => !samePath(item, server));
      try {
        window.localStorage.setItem(networkStorageKey, JSON.stringify(next));
      } catch {
        // Ignore storage failures; browsing should still work.
      }
      return next;
    });
  }

  /** Opens the address bar ready for a UNC path, so adding a server needs no separate dialog. */
  function addNetworkLocation() {
    setPathDraft("\\\\");
    setEditingPath(true);
  }

  const entries = useMemo(() => {
    const all = listing.data?.entries ?? [];
    const needle = filter.trim().toLowerCase();
    const visible = needle ? all.filter((entry) => entry.name.toLowerCase().includes(needle)) : all;

    const direction = sortDirection === "asc" ? 1 : -1;
    return [...visible].sort((left, right) => {
      // Folders lead regardless of sort, as they do in a file manager.
      if (left.kind !== right.kind) return left.kind === "folder" ? -1 : 1;
      switch (sortColumn) {
        case "modified":
          return direction * (Date.parse(left.modifiedUtc) - Date.parse(right.modifiedUtc));
        case "size":
          return direction * ((left.sizeBytes ?? -1) - (right.sizeBytes ?? -1));
        case "type":
          return direction * describeType(left).localeCompare(describeType(right));
        default:
          return direction * left.name.localeCompare(right.name, undefined, { numeric: true });
      }
    });
  }, [filter, listing.data?.entries, sortColumn, sortDirection]);

  function toggleSort(column: SortColumn) {
    if (column === sortColumn) {
      setSortDirection((current) => (current === "asc" ? "desc" : "asc"));
    } else {
      setSortColumn(column);
      setSortDirection("asc");
    }
  }

  function open(entry: FileSystemEntry) {
    if (entry.kind === "folder") navigate(entry.path);
    else onSelect(entry.path, "file");
  }

  function selectEntry(entry: FileSystemEntry, toggle: boolean, range = false) {
    listRef.current?.focus();
    setActivePath(entry.path);
    if (range && onSelectMany && selectionAnchorPath) {
      const anchorIndex = entries.findIndex((item) => samePath(item.path, selectionAnchorPath));
      const entryIndex = entries.findIndex((item) => samePath(item.path, entry.path));
      if (anchorIndex >= 0 && entryIndex >= 0) {
        const start = Math.min(anchorIndex, entryIndex);
        const end = Math.max(anchorIndex, entryIndex);
        setSelectedPaths(entries.slice(start, end + 1).map((item) => item.path));
        return;
      }
    }
    setSelectionAnchorPath(entry.path);
    setSelectedPaths((current) => {
      if (!toggle || !onSelectMany) return [entry.path];
      return current.some((path) => samePath(path, entry.path))
        ? current.filter((path) => !samePath(path, entry.path))
        : [...current, entry.path];
    });
  }

  function submitSelection(selectedEntries: FileSystemEntry[]) {
    if (onSelectMany && selectedEntries.length > 1) {
      void onSelectMany(selectedEntries.map((entry) => ({ path: entry.path, kind: entry.kind })), currentPath);
      return;
    }
    const entry = selectedEntries[0];
    if (entry) onSelect(entry.path, entry.kind, currentPath);
    else if (currentPath) onSelect(currentPath, "folder", currentPath);
  }

  function onListKeyDown(event: React.KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a" && onSelectMany) {
      event.preventDefault();
      setSelectedPaths(entries.map((entry) => entry.path));
      setActivePath(entries[0]?.path ?? "");
      setSelectionAnchorPath(entries[0]?.path ?? "");
      return;
    }
    if (event.key === "Backspace") {
      event.preventDefault();
      goUp();
      return;
    }
    if (!entries.length) return;
    const index = activePath ? entries.findIndex((entry) => samePath(entry.path, activePath)) : -1;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      selectEntry(entries[Math.min(index + 1, entries.length - 1)], false, event.shiftKey);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      selectEntry(entries[Math.max(index - 1, 0)], false, event.shiftKey);
    } else if (event.key === "Enter" && activePath) {
      event.preventDefault();
      const active = entries.find((entry) => samePath(entry.path, activePath));
      if (active) open(active);
    }
  }

  const currentPath = listing.data?.path ?? path;
  const crumbs = breadcrumbs(currentPath);
  const selectedEntries = entries.filter((entry) => selectedPaths.some((path) => samePath(path, entry.path)));

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6">
      <section
        className="flex h-[82vh] min-h-[35rem] w-full max-w-5xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-[0_1.5rem_5rem_rgba(0,0,0,0.45)]"
        role="dialog"
        aria-label="Select media source"
      >
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-border px-4">
          <h2 className="text-sm font-semibold">Select Media Source</h2>
          <button
            type="button"
            onClick={onCancel}
            className="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text"
            aria-label="Close browser"
          >
            <X size={16} />
          </button>
        </header>

        <div className="flex h-12 shrink-0 items-center gap-1.5 border-b border-border bg-panel/40 px-3">
          <button
            type="button"
            onClick={() => setCursor((current) => Math.max(0, current - 1))}
            disabled={cursor === 0}
            className="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text disabled:text-disabled disabled:hover:bg-transparent"
            aria-label="Back"
            title="Back"
          >
            <ArrowLeft size={16} />
          </button>
          <button
            type="button"
            onClick={() => setCursor((current) => Math.min(history.length - 1, current + 1))}
            disabled={cursor >= history.length - 1}
            className="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text disabled:text-disabled disabled:hover:bg-transparent"
            aria-label="Forward"
            title="Forward"
          >
            <ArrowRight size={16} />
          </button>
          <button
            type="button"
            onClick={goUp}
            disabled={!currentPath || (!hasVolumeList && !listing.data?.parentPath)}
            className="inline-flex h-8 w-8 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text disabled:text-disabled disabled:hover:bg-transparent"
            aria-label="Up one level"
            title="Up one level"
          >
            <ArrowUp size={16} />
          </button>

          {/* Clicking the breadcrumb's empty space swaps to a typable path, the
              way an address bar behaves in a file manager. */}
          {editingPath ? (
            <input
              autoFocus
              value={pathDraft}
              onChange={(event) => setPathDraft(event.target.value)}
              onBlur={() => setEditingPath(false)}
              onKeyDown={(event) => {
                if (event.key === "Enter") navigate(pathDraft.trim());
                if (event.key === "Escape") setEditingPath(false);
              }}
              className="h-8 min-w-0 flex-1 rounded-md border border-accent bg-input px-2 font-mono text-xs text-text outline-none"
              aria-label="Path"
            />
          ) : (
            <div
              role="button"
              tabIndex={0}
              onClick={() => setEditingPath(true)}
              onKeyDown={(event) => {
                if (event.key === "Enter") setEditingPath(true);
              }}
              className="flex h-8 min-w-0 flex-1 cursor-text items-center gap-0.5 overflow-x-auto rounded-md border border-border bg-input px-2 text-xs"
              // Labelled explicitly because the crumbs inside are buttons too,
              // which would otherwise make this element's name theirs.
              aria-label="Address bar"
              title={`${currentPath || "This PC"} — click to type a path`}
            >
              {hasVolumeList ? (
                <button
                  type="button"
                  onClick={(event) => {
                    event.stopPropagation();
                    navigate("");
                  }}
                  className="inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-1 text-muted transition hover:bg-button-hover hover:text-text"
                >
                  <Monitor size={13} />
                  This PC
                </button>
              ) : null}
              {crumbs.map((crumb) => (
                <span key={crumb.path} className="flex shrink-0 items-center">
                  <ChevronRight size={12} className="text-subtle" />
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      navigate(crumb.path);
                    }}
                    className="rounded px-1.5 py-1 text-muted transition hover:bg-button-hover hover:text-text"
                  >
                    {crumb.label}
                  </button>
                </span>
              ))}
            </div>
          )}

          <div className="relative shrink-0">
            <Search size={13} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-subtle" />
            <input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="Filter"
              className="h-8 w-40 rounded-md border border-border bg-input pl-7 pr-2 text-xs text-text outline-none transition placeholder:text-subtle focus:border-accent"
              aria-label="Filter this folder"
            />
          </div>
          <button
            type="button"
            onClick={() => listing.refetch()}
            className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text"
            aria-label="Refresh"
            title="Refresh"
          >
            <RefreshCw size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1">
          <nav className="w-52 shrink-0 overflow-y-auto border-r border-border bg-sidebar/60 py-3">
            {homeRoot ? (
              <>
                <div className="px-4 pb-1.5 text-[0.625rem] font-semibold uppercase tracking-wider text-subtle">
                  Home
                </div>
                <button
                  type="button"
                  onClick={() => navigate(homeRoot.path)}
                  className={[
                    "flex w-full items-center gap-2 px-4 py-1.5 text-left text-xs transition",
                    samePath(homeRoot.path, currentPath)
                      ? "bg-selected text-text"
                      : "text-muted hover:bg-button-hover hover:text-text"
                  ].join(" ")}
                  title={homeRoot.path || "This PC"}
                >
                  <House size={13} className="shrink-0 text-accent" />
                  <span className="truncate">{homeRoot.name}</span>
                </button>
              </>
            ) : null}
            {roots.length > 0 ? (
              <>
                <div className="px-4 pb-1.5 pt-4 text-[0.625rem] font-semibold uppercase tracking-wider text-subtle">
                  Quick access
                </div>
                {roots.map((root) => {
                  const removable = removableRootPaths.some((path) => samePath(path, root.path));
                  return (
                    <div key={root.path} className="group/quick relative flex items-center">
                      <button
                        type="button"
                        onClick={() => navigate(root.path)}
                        className={[
                          "flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-4 pr-7 text-left text-xs transition",
                          samePath(root.path, currentPath)
                            ? "bg-selected text-text"
                            : "text-muted hover:bg-button-hover hover:text-text"
                        ].join(" ")}
                        title={root.path}
                      >
                        <Pin size={13} className="shrink-0 text-accent" />
                        <span className="truncate">{root.name}</span>
                      </button>
                      {removable ? (
                        <button
                          type="button"
                          onClick={() => void onUnpinFromQuickAccess?.(root.path, root.name)}
                          className="absolute right-1.5 hidden h-5 w-5 items-center justify-center rounded text-subtle transition hover:bg-button-hover hover:text-text group-hover/quick:flex"
                          aria-label={`Remove ${root.name} from Quick Access`}
                          title="Remove from Quick Access"
                        >
                          <X size={11} />
                        </button>
                      ) : null}
                    </div>
                  );
                })}
              </>
            ) : null}

            {hasVolumeList ? (
              <div className="px-4 pb-1.5 pt-4 text-[0.625rem] font-semibold uppercase tracking-wider text-subtle">
                This PC
              </div>
            ) : null}
            {(hasVolumeList ? volumes.data?.entries ?? [] : []).map((volume) => (
              <button
                key={volume.path}
                type="button"
                onClick={() => navigate(volume.path)}
                className={[
                  "flex w-full items-center gap-2 px-4 py-1.5 text-left text-xs transition",
                  samePath(volume.path, currentPath)
                    ? "bg-selected text-text"
                    : "text-muted hover:bg-button-hover hover:text-text"
                ].join(" ")}
                title={volume.path}
              >
                <HardDrive size={13} className="shrink-0 text-subtle" />
                <span className="truncate">{volume.name}</span>
              </button>
            ))}

            {hasVolumeList ? (
              <>
                <div className="px-4 pb-1.5 pt-4 text-[0.625rem] font-semibold uppercase tracking-wider text-subtle">
                  Network
                </div>
                {networkLocations.map((server) => (
                  <div key={server} className="group/net relative flex items-center">
                    <button
                      type="button"
                      onClick={() => navigate(server)}
                      className={[
                        "flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-4 pr-7 text-left text-xs transition",
                        samePath(server, currentPath)
                          ? "bg-selected text-text"
                          : "text-muted hover:bg-button-hover hover:text-text"
                      ].join(" ")}
                      title={server}
                    >
                      <Network size={13} className="shrink-0 text-subtle" />
                      <span className="truncate">{server.replace(/^\\\\/, "")}</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => forgetNetworkLocation(server)}
                      className="absolute right-1.5 hidden h-5 w-5 items-center justify-center rounded text-subtle transition hover:bg-button-hover hover:text-text group-hover/net:flex"
                      aria-label={`Forget ${server}`}
                      title="Remove from this list"
                    >
                      <X size={11} />
                    </button>
                  </div>
                ))}
                <button
                  type="button"
                  onClick={addNetworkLocation}
                  className="flex w-full items-center gap-2 px-4 py-1.5 text-left text-xs text-subtle transition hover:bg-button-hover hover:text-text"
                >
                  <Plus size={13} className="shrink-0" />
                  <span className="truncate">Add network location</span>
                </button>
              </>
            ) : null}
          </nav>

          <div
            ref={listRef}
            tabIndex={0}
            onKeyDown={onListKeyDown}
            className="min-w-0 flex-1 overflow-auto outline-none"
          >
            <table className="w-full table-fixed border-collapse text-left text-xs">
              <thead className="sticky top-0 z-10 bg-panel">
                <tr className="text-subtle">
                  {([
                    ["name", "Name", ""],
                    ["modified", "Date modified", "w-40"],
                    ["type", "Type", "w-28"],
                    ["size", "Size", "w-20"]
                  ] as Array<[SortColumn, string, string]>).map(([column, label, width]) => (
                    <th key={column} className={`border-b border-border font-medium ${width}`}>
                      <button
                        type="button"
                        onClick={() => toggleSort(column)}
                        className="flex w-full items-center gap-1 px-3 py-2 text-left transition hover:text-text"
                      >
                        <span className="flex-1">{label}</span>
                        {sortColumn === column ? (
                          <span aria-hidden className="text-accent">{sortDirection === "asc" ? "▲" : "▼"}</span>
                        ) : null}
                      </button>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {entries.map((entry) => {
                  const isSelected = selectedPaths.some((path) => samePath(path, entry.path));
                  return (
                    <tr
                      key={entry.path}
                      onClick={(event) => selectEntry(
                        entry,
                        event.ctrlKey || event.metaKey,
                        event.shiftKey
                      )}
                      onDoubleClick={() => open(entry)}
                      onContextMenu={(event) => {
                        if (entry.kind !== "folder") return;
                        event.preventDefault();
                        if (!isSelected) selectEntry(entry, false);
                        setFolderMenu({ x: event.clientX, y: event.clientY, entry });
                      }}
                      className={[
                        "cursor-default select-none",
                        isSelected ? "bg-selected text-text" : "text-muted hover:bg-input-hover"
                      ].join(" ")}
                    >
                      <td className="min-w-0 px-3 py-1.5">
                        <span className="flex items-center gap-2">
                          {entry.kind === "folder" ? (
                            <Folder size={15} className="shrink-0 text-accent" />
                          ) : (
                            <FileVideo size={15} className="shrink-0 text-subtle" />
                          )}
                          <span className="truncate">{entry.name}</span>
                        </span>
                      </td>
                      {/* Truncating rather than merely not wrapping keeps a
                          long value from overflowing its fixed column and
                          scrolling the whole list sideways. */}
                      <td className="truncate px-3 py-1.5 text-subtle">{formatModified(entry.modifiedUtc)}</td>
                      <td className="truncate px-3 py-1.5 text-subtle">{describeType(entry)}</td>
                      <td className="truncate px-3 py-1.5 text-right text-subtle">{formatSize(entry.sizeBytes)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>

            {listing.isLoading ? (
              <div className="p-6 text-center text-xs text-muted">Loading…</div>
            ) : listing.error ? (
              <div className="m-3 rounded-md border border-warning bg-input p-3 text-xs text-warning">
                {listing.error instanceof Error ? listing.error.message : "This folder could not be read."}
              </div>
            ) : entries.length === 0 ? (
              <div className="p-6 text-center text-xs text-muted">
                {filter ? "Nothing matches that filter." : "This folder is empty."}
              </div>
            ) : null}
          </div>
        </div>

        {folderMenu ? (
          <div
            role="menu"
            aria-label={`${folderMenu.entry.name} folder options`}
            className="fixed z-[60] min-w-44 overflow-hidden rounded-lg border border-border bg-card p-1 shadow-[0_0.75rem_2.5rem_rgba(0,0,0,0.45)]"
            style={{ left: Math.min(folderMenu.x, window.innerWidth - 200), top: Math.min(folderMenu.y, window.innerHeight - 132) }}
            onPointerDown={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              role="menuitem"
              disabled={!onPinToQuickAccess || roots.some((root) => samePath(root.path, folderMenu.entry.path))}
              onClick={() => {
                void onPinToQuickAccess?.(folderMenu.entry.path, folderMenu.entry.name);
                setFolderMenu(null);
              }}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-xs text-muted transition hover:bg-selected hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              <Pin size={14} className="text-accent" />
              Pin to Quick Access
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                navigate(folderMenu.entry.path);
                setFolderMenu(null);
              }}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-xs text-muted transition hover:bg-selected hover:text-text"
            >
              <FolderOpen size={14} className="text-accent" />
              Open Folder
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                onSelect(folderMenu.entry.path, "folder", currentPath);
                setFolderMenu(null);
              }}
              className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-xs text-muted transition hover:bg-selected hover:text-text"
            >
              <Check size={14} className="text-accent" />
              Select Folder
            </button>
          </div>
        ) : null}

        <footer className="flex h-14 shrink-0 items-center justify-between gap-4 border-t border-border bg-panel/40 px-4">
          <div className="min-w-0 truncate text-xs text-subtle">
            {entries.length} item{entries.length === 1 ? "" : "s"}
            {selectedEntries.length > 0 ? ` · ${selectedEntries.length} selected` : ""}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              type="button"
              onClick={onCancel}
              className="h-8 rounded-md border border-border bg-button px-4 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => submitSelection(selectedEntries)}
              disabled={selectedEntries.length === 0 && !currentPath}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-accent px-4 text-xs font-semibold text-window transition hover:bg-accent-hover disabled:bg-button disabled:text-disabled"
            >
              <Check size={14} />
              {selectedEntries.length > 1
                ? `Select ${selectedEntries.length} Items`
                : selectedEntries[0]
                  ? selectedEntries[0].kind === "folder" ? "Select Folder" : "Select File"
                  : "Select This Folder"}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
