import { Activity, Database, FileCog, FolderOpen, ListVideo, Logs, Settings, Shuffle } from "lucide-react";
import { NavLink, Outlet } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { getStatus } from "../api";
import mkvoIcon from "../assets/mkvo-icon-purple.png";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useOperationJob } from "../state/OperationJobContext";

const navItems = [
  { to: "/dashboard", label: "Dashboard", icon: Activity },
  { to: "/rename", label: "Rename Files", icon: ListVideo },
  { to: "/mux-remux", label: "MKV Operations", icon: Shuffle },
  { to: "/track-properties", label: "Track Properties", icon: FileCog },
  { to: "/library", label: "Library", icon: Database },
  { to: "/settings", label: "Settings", icon: Settings },
  { to: "/logs", label: "Logs", icon: Logs }
];

export function Layout() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const missingTools = status.data?.tools.filter((tool) => !tool.available).length ?? 0;
  const { selectionError } = useMediaLibrary();
  const operation = useOperationJob();

  return (
    <div className="h-screen overflow-hidden bg-window text-text">
      <div className="grid h-screen grid-cols-[14.75rem_1fr]">
        <aside className="flex h-screen min-h-0 flex-col border-r border-border bg-sidebar px-3 py-5">
          <div className="mb-8 flex items-center gap-3 px-1">
            <div className="flex h-9 w-9 items-center justify-center">
              <span
                className="h-9 w-9"
                style={{
                  backgroundColor: "var(--color-app-title)",
                  maskImage: `url(${mkvoIcon})`,
                  maskPosition: "center",
                  maskRepeat: "no-repeat",
                  maskSize: "contain",
                  WebkitMaskImage: `url(${mkvoIcon})`,
                  WebkitMaskPosition: "center",
                  WebkitMaskRepeat: "no-repeat",
                  WebkitMaskSize: "contain",
                  filter: "drop-shadow(0 0 0.875rem color-mix(in srgb, var(--color-app-title) 32%, transparent))"
                }}
                aria-hidden="true"
              />
            </div>
            <div>
              <div className="text-base font-bold text-app-title">MKV Orchestrator</div>
              <div className="mt-0.5 text-xs text-subtle">Media operations console</div>
            </div>
          </div>

          <nav className="space-y-1.5">
            {navItems.map((item) => {
              const Icon = item.icon;
              return (
                <NavLink
                  key={item.to}
                  to={item.to}
                  className={({ isActive }) =>
                    [
                      "flex h-9 items-center gap-3 rounded-md px-3 text-sm font-medium transition",
                      isActive
                        ? "bg-selected text-text shadow-[inset_3px_0_0_var(--color-accent)]"
                        : "text-muted hover:bg-input-hover hover:text-text"
                    ].join(" ")
                  }
                >
                  <Icon size={16} />
                  <span>{item.label}</span>
                </NavLink>
              );
            })}
          </nav>

          <div className="mt-auto min-w-0 overflow-hidden rounded-lg border border-border bg-panel p-3">
            <div className="text-[0.6875rem] font-semibold uppercase tracking-wide text-subtle">Status</div>
            <div className={["mt-2 min-w-0 break-words text-sm font-medium [overflow-wrap:anywhere]", operation.isRunning ? "text-accent" : "text-success"].join(" ")} title={operation.statusText ?? undefined}>
              {operation.statusText ?? (status.isLoading ? "checking tools" : missingTools === 0 ? "ready" : `${missingTools} tool issue(s)`)}
            </div>
            <div className="mt-3 flex items-center gap-2 truncate text-xs text-muted">
              <FolderOpen size={14} />
              <span className="truncate">{status.data?.mediaRoot ?? "/media"}</span>
            </div>
          </div>
        </aside>

        <main className="flex min-h-0 min-w-0 flex-col overflow-hidden px-8 py-8">
          {selectionError ? (
            <div role="alert" className="mb-3 shrink-0 rounded-md border border-warning bg-panel px-4 py-2 text-sm text-warning">
              Selection sync failed: {selectionError}
            </div>
          ) : null}
          <div className="min-h-0 min-w-0 flex-1 overflow-auto">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
