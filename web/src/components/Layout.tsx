import { useEffect, useState } from "react";
import { Activity, Database, FileCog, FolderOpen, LayoutGrid, ListVideo, Logs, Menu, Settings, Shuffle, X } from "lucide-react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { getStatus } from "../api";
import mkvoIcon from "../assets/mkvo-icon-purple.png";
import { useMediaLibrary } from "../state/MediaLibraryContext";
import { useOperationJob } from "../state/OperationJobContext";

const navItems = [
  { to: "/dashboard", label: "Dashboard", shortLabel: "Home", icon: Activity },
  { to: "/rename", label: "Rename Files", shortLabel: "Rename", icon: ListVideo },
  { to: "/mux-remux", label: "MKV Operations", shortLabel: "Operate", icon: Shuffle },
  { to: "/track-properties", label: "Track Properties", shortLabel: "Tracks", icon: FileCog },
  { to: "/library", label: "Library", shortLabel: "Library", icon: Database },
  { to: "/settings", label: "Settings", shortLabel: "Settings", icon: Settings },
  { to: "/logs", label: "Logs", shortLabel: "Logs", icon: Logs }
];

const primaryMobileItems = navItems.filter((item) => ["/dashboard", "/library", "/mux-remux"].includes(item.to));
const mobileQuery = "(max-width: 767px) and (hover: none) and (pointer: coarse)";
const layoutPreferenceKey = "mkvo.web.layoutPreference";

type LayoutPreference = "auto" | "desktop" | "mobile";

function readLayoutPreference(): LayoutPreference {
  try {
    const value = window.localStorage.getItem(layoutPreferenceKey);
    return value === "desktop" || value === "mobile" ? value : "auto";
  } catch {
    return "auto";
  }
}

function useMobileLayout(preference: LayoutPreference) {
  const [matchesMobileDevice, setMatchesMobileDevice] = useState(() =>
    typeof window.matchMedia === "function" && window.matchMedia(mobileQuery).matches);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(mobileQuery);
    const update = () => setMatchesMobileDevice(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  return preference === "mobile" || (preference === "auto" && matchesMobileDevice);
}

export function Layout() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const missingTools = status.data?.tools.filter((tool) => !tool.available).length ?? 0;
  const { selectionError } = useMediaLibrary();
  const operation = useOperationJob();
  const location = useLocation();
  const [layoutPreference, setLayoutPreferenceState] = useState<LayoutPreference>(readLayoutPreference);
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const mobileLayout = useMobileLayout(layoutPreference);
  const currentItem = navItems.find((item) => location.pathname.startsWith(item.to)) ?? navItems[0];

  useEffect(() => setMobileMenuOpen(false), [location.pathname]);

  useEffect(() => {
    document.documentElement.classList.toggle("mkvo-mobile-ui", mobileLayout);
    return () => document.documentElement.classList.remove("mkvo-mobile-ui");
  }, [mobileLayout]);

  function setLayoutPreference(preference: LayoutPreference) {
    setLayoutPreferenceState(preference);
    try {
      window.localStorage.setItem(layoutPreferenceKey, preference);
    } catch {
      // The selected layout still applies for this session when storage is unavailable.
    }
  }

  const statusText = operation.statusText ?? (status.isLoading ? "checking tools" : missingTools === 0 ? "ready" : `${missingTools} tool issue(s)`);

  return (
    <div className={["h-screen overflow-hidden bg-window text-text", mobileLayout ? "mkvo-mobile-shell" : ""].join(" ")}>
      <div className={mobileLayout ? "flex h-screen flex-col" : "grid h-screen grid-cols-[14.75rem_1fr]"}>
        {mobileLayout ? (
          <header className="mkvo-mobile-header">
            <div className="flex min-w-0 items-center gap-3">
              <AppIcon />
              <div className="min-w-0">
                <div className="truncate text-sm font-bold text-app-title">MKV Orchestrator</div>
                <div className="truncate text-xs text-subtle">{currentItem.label}</div>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <span className={["max-w-28 truncate text-xs font-semibold", operation.isRunning ? "text-accent" : "text-success"].join(" ")} title={statusText}>{statusText}</span>
              <button type="button" className="mkvo-mobile-icon-button" onClick={() => setMobileMenuOpen(true)} aria-label="Open navigation menu"><Menu size={20} /></button>
            </div>
          </header>
        ) : (
          <DesktopSidebar statusText={statusText} operationRunning={operation.isRunning} mediaRoot={status.data?.mediaRoot} layoutPreference={layoutPreference} onLayoutPreferenceChange={setLayoutPreference} />
        )}

        <main className={mobileLayout ? "mkvo-mobile-main" : "flex min-h-0 min-w-0 flex-col overflow-hidden px-8 py-8"}>
          {selectionError ? (
            <div role="alert" className="mb-3 shrink-0 rounded-md border border-warning bg-panel px-4 py-2 text-sm text-warning">
              Selection sync failed: {selectionError}
            </div>
          ) : null}
          <div className="min-h-0 min-w-0 flex-1 overflow-auto"><Outlet /></div>
        </main>

        {mobileLayout ? (
          <>
            <nav className="mkvo-mobile-bottom-nav" aria-label="Primary navigation">
              {primaryMobileItems.map((item) => <MobileNavItem key={item.to} item={item} />)}
              <button type="button" className="mkvo-mobile-nav-item text-subtle" onClick={() => setMobileMenuOpen(true)} aria-label="Open all navigation"><LayoutGrid size={20} /><span>More</span></button>
            </nav>
            {mobileMenuOpen ? (
              <div className="mkvo-mobile-drawer-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && setMobileMenuOpen(false)}>
                <section className="mkvo-mobile-drawer" role="dialog" aria-modal="true" aria-label="MKV Orchestrator navigation">
                  <header className="flex items-center justify-between border-b border-border px-4 py-3">
                    <div><div className="font-semibold text-text">All tools</div><div className="mt-0.5 text-xs text-subtle">Every desktop feature remains available</div></div>
                    <button type="button" className="mkvo-mobile-icon-button" onClick={() => setMobileMenuOpen(false)} aria-label="Close navigation menu"><X size={20} /></button>
                  </header>
                  <nav className="grid grid-cols-2 gap-2 p-4" aria-label="All navigation">
                    {navItems.map((item) => {
                      const Icon = item.icon;
                      return <NavLink key={item.to} to={item.to} className={({ isActive }) => ["flex min-h-16 items-center gap-3 rounded-lg border px-3 py-2 text-sm font-semibold", isActive ? "border-accent bg-selected text-text" : "border-border bg-panel text-muted"].join(" ")}><Icon size={19} /><span>{item.label}</span></NavLink>;
                    })}
                  </nav>
                  <div className="mt-auto border-t border-border p-4">
                    <label className="block text-xs font-semibold uppercase tracking-wide text-subtle" htmlFor="mobile-layout-mode">Interface layout</label>
                    <select id="mobile-layout-mode" value={layoutPreference} onChange={(event) => setLayoutPreference(event.target.value as LayoutPreference)} className="mt-2 h-11 w-full rounded-md border border-border bg-input px-3 text-sm text-text">
                      <option value="auto">Automatic</option><option value="desktop">Desktop workspace</option><option value="mobile">Mobile touch layout</option>
                    </select>
                  </div>
                </section>
              </div>
            ) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}

function AppIcon() {
  return <div className="flex h-9 w-9 shrink-0 items-center justify-center"><span className="h-9 w-9" style={{ backgroundColor: "var(--color-app-title)", maskImage: `url(${mkvoIcon})`, maskPosition: "center", maskRepeat: "no-repeat", maskSize: "contain", WebkitMaskImage: `url(${mkvoIcon})`, WebkitMaskPosition: "center", WebkitMaskRepeat: "no-repeat", WebkitMaskSize: "contain", filter: "drop-shadow(0 0 0.875rem color-mix(in srgb, var(--color-app-title) 32%, transparent))" }} aria-hidden="true" /></div>;
}

function DesktopSidebar({ statusText, operationRunning, mediaRoot, layoutPreference, onLayoutPreferenceChange }: { statusText: string; operationRunning: boolean; mediaRoot?: string; layoutPreference: LayoutPreference; onLayoutPreferenceChange: (preference: LayoutPreference) => void }) {
  return (
    <aside className="flex h-screen min-h-0 flex-col border-r border-border bg-sidebar px-3 py-5">
      <div className="mb-8 flex items-center gap-3 px-1"><AppIcon /><div><div className="text-base font-bold text-app-title">MKV Orchestrator</div><div className="mt-0.5 text-xs text-subtle">Media operations console</div></div></div>
      <nav className="space-y-1.5">
        {navItems.map((item) => { const Icon = item.icon; return <NavLink key={item.to} to={item.to} className={({ isActive }) => ["flex h-9 items-center gap-3 rounded-md px-3 text-sm font-medium transition", isActive ? "bg-selected text-text shadow-[inset_3px_0_0_var(--color-accent)]" : "text-muted hover:bg-input-hover hover:text-text"].join(" ")}><Icon size={16} /><span>{item.label}</span></NavLink>; })}
      </nav>
      <div className="mt-auto min-w-0 overflow-hidden rounded-lg border border-border bg-panel p-3">
        <div className="text-[0.6875rem] font-semibold uppercase tracking-wide text-subtle">Status</div>
        <div className={["mt-2 min-w-0 break-words text-sm font-medium [overflow-wrap:anywhere]", operationRunning ? "text-accent" : "text-success"].join(" ")} title={statusText}>{statusText}</div>
        <div className="mt-3 flex items-center gap-2 truncate text-xs text-muted"><FolderOpen size={14} /><span className="truncate">{mediaRoot ?? "/media"}</span></div>
        <label className="mt-4 block text-[0.6875rem] font-semibold uppercase tracking-wide text-subtle" htmlFor="desktop-layout-mode">Interface layout</label>
        <select id="desktop-layout-mode" value={layoutPreference} onChange={(event) => onLayoutPreferenceChange(event.target.value as LayoutPreference)} className="mt-2 h-8 w-full rounded-md border border-border bg-input px-2 text-xs text-text"><option value="auto">Automatic</option><option value="desktop">Desktop workspace</option><option value="mobile">Mobile touch layout</option></select>
      </div>
    </aside>
  );
}

function MobileNavItem({ item }: { item: (typeof navItems)[number] }) {
  const Icon = item.icon;
  return <NavLink to={item.to} className={({ isActive }) => ["mkvo-mobile-nav-item", isActive ? "text-accent" : "text-subtle"].join(" ")}><Icon size={20} /><span>{item.shortLabel}</span></NavLink>;
}
