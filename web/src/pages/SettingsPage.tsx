import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BookOpen,
  CheckCircle2,
  CircleAlert,
  Database,
  KeyRound,
  Palette,
  Plus,
  RefreshCw,
  SlidersHorizontal,
  Trash2,
  Wrench
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  getStatus,
  getWebSettings,
  saveWebSettings,
  syncMediaServerLibraries,
  testMediaServerConnection
} from "../api";
import type { WebMediaServer, WebMediaServerPathMapping, WebSettings } from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { applyWebTheme, getAllWebThemes, getStoredWebThemeName, getWebTheme, removeCustomWebTheme, saveCustomWebTheme } from "../theme";

const settingsTabStorageKey = "mkvo.web.settingsTab";

const settingsTabs = [
  { id: "general", label: "General", Icon: Wrench },
  { id: "rename", label: "Rename", Icon: KeyRound },
  { id: "presets", label: "Presets", Icon: SlidersHorizontal },
  { id: "library", label: "Library", Icon: Database },
  { id: "appearance", label: "Appearance", Icon: Palette },
  { id: "about", label: "About", Icon: BookOpen }
] as const;

type SettingsTabId = typeof settingsTabs[number]["id"];
type SettingsTabDefinition = {
  id: SettingsTabId;
  label: string;
  Icon: LucideIcon;
};

type EditableMediaServer = WebMediaServer & {
  apiKey?: string;
};

export function SettingsPage() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const webSettings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const [activeTab, setActiveTab] = useState<SettingsTabId>(() => readStoredSettingsTab());
  const [tvdbApiKey, setTvdbApiKey] = useState("");
  const [tvdbPin, setTvdbPin] = useState("");
  const [tmdbApiKey, setTmdbApiKey] = useState("");
  const [language, setLanguage] = useState("eng");
  const [provider, setProvider] = useState("TVDB");
  const [template, setTemplate] = useState("{series} - S{season:00}E{episode:00} - {episodeTitle}");
  const [renameTemplatesText, setRenameTemplatesText] = useState("");
  const [audioNamePresetsText, setAudioNamePresetsText] = useState("");
  const [subtitleNamePresetsText, setSubtitleNamePresetsText] = useState("");
  const [languagePresetsText, setLanguagePresetsText] = useState("");
  const [muxAudioDefaults, setMuxAudioDefaults] = useState("eng,jpn");
  const [muxSubtitleDefaults, setMuxSubtitleDefaults] = useState("eng");
  const [watchFoldersText, setWatchFoldersText] = useState("");
  const [liveWatcherEnabled, setLiveWatcherEnabled] = useState(false);
  const [mediaServers, setMediaServers] = useState<EditableMediaServer[]>([]);
  const [mediaServerMappingsText, setMediaServerMappingsText] = useState("");
  const [newServerName, setNewServerName] = useState("Media Server");
  const [newServerType, setNewServerType] = useState("Emby");
  const [newServerUrl, setNewServerUrl] = useState("");
  const [newServerApiKey, setNewServerApiKey] = useState("");
  const [makeNewServerDefault, setMakeNewServerDefault] = useState(false);
  const [availableThemes, setAvailableThemes] = useState(() => getAllWebThemes());
  const [themeName, setThemeName] = useState(() => getStoredWebThemeName());
  const [themeJson, setThemeJson] = useState(() => JSON.stringify(getWebTheme(getStoredWebThemeName()), null, 2));
  const [customThemeName, setCustomThemeName] = useState("My Theme");
  const [settingsStatus, setSettingsStatus] = useState("");

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(settingsTabStorageKey, activeTab);
    }
  }, [activeTab]);

  useEffect(() => {
    if (!webSettings.data) return;
    setLanguage(webSettings.data.tvdbLanguage || "eng");
    setProvider(webSettings.data.renameLookupProvider || "TVDB");
    setTemplate(webSettings.data.renameTemplate || "{series} - S{season:00}E{episode:00} - {episodeTitle}");
    setRenameTemplatesText((webSettings.data.renameTemplates ?? []).join("\n"));
    setAudioNamePresetsText((webSettings.data.audioNamePresets ?? []).join("\n"));
    setSubtitleNamePresetsText((webSettings.data.subtitleNamePresets ?? []).join("\n"));
    setLanguagePresetsText((webSettings.data.languagePresets ?? []).join("\n"));
    setMuxAudioDefaults(webSettings.data.mkvMergeDefaultAudioLanguages || "eng,jpn");
    setMuxSubtitleDefaults(webSettings.data.mkvMergeDefaultSubtitleLanguages || "eng");
    setWatchFoldersText((webSettings.data.watchFolders ?? []).join("\n"));
    setLiveWatcherEnabled(Boolean(webSettings.data.enableLiveWatchFolderMonitoring));
    setMediaServers((webSettings.data.mediaServers ?? []).map((server) => ({ ...server, apiKey: "" })));
    setMediaServerMappingsText(formatPathMappings(webSettings.data.mediaServerPathMappings ?? []));
  }, [webSettings.data]);

  async function saveSettings(): Promise<WebSettings | null> {
    try {
      const saved = await saveWebSettings({
        tvdbApiKey: tvdbApiKey || undefined,
        tvdbPin: tvdbPin || undefined,
        tmdbApiKey: tmdbApiKey || undefined,
        tvdbLanguage: language,
        renameLookupProvider: provider,
        renameTemplate: template,
        renameTemplates: normalizeRenameTemplates(renameTemplatesText, template),
        audioNamePresets: normalizeLineList(audioNamePresetsText),
        subtitleNamePresets: normalizeLineList(subtitleNamePresetsText),
        languagePresets: normalizeLineList(languagePresetsText),
        mkvMergeDefaultAudioLanguages: muxAudioDefaults,
        mkvMergeDefaultSubtitleLanguages: muxSubtitleDefaults,
        watchFolders: normalizeLineList(watchFoldersText),
        enableLiveWatchFolderMonitoring: liveWatcherEnabled,
        mediaServers: mediaServers.map((server) => ({
          id: server.id,
          name: server.name,
          type: server.type,
          serverUrl: server.serverUrl,
          apiKey: server.apiKey || undefined,
          isDefault: server.isDefault,
          libraries: server.libraries
        })),
        mediaServerPathMappings: parsePathMappings(mediaServerMappingsText)
      });

      setTvdbApiKey("");
      setTvdbPin("");
      setTmdbApiKey("");
      setSettingsStatus("Settings saved.");
      webSettings.refetch();
      setLanguage(saved.tvdbLanguage);
      setProvider(saved.renameLookupProvider);
      setTemplate(saved.renameTemplate);
      setRenameTemplatesText(saved.renameTemplates.join("\n"));
      setAudioNamePresetsText(saved.audioNamePresets.join("\n"));
      setSubtitleNamePresetsText(saved.subtitleNamePresets.join("\n"));
      setLanguagePresetsText(saved.languagePresets.join("\n"));
      setMuxAudioDefaults(saved.mkvMergeDefaultAudioLanguages);
      setMuxSubtitleDefaults(saved.mkvMergeDefaultSubtitleLanguages);
      setWatchFoldersText(saved.watchFolders.join("\n"));
      setLiveWatcherEnabled(saved.enableLiveWatchFolderMonitoring);
      setMediaServers(saved.mediaServers.map((server) => ({ ...server, apiKey: "" })));
      setMediaServerMappingsText(formatPathMappings(saved.mediaServerPathMappings));
      status.refetch();
      return saved;
    } catch (error) {
      setSettingsStatus(error instanceof Error ? error.message : "Settings could not be saved.");
      return null;
    }
  }

  function addMediaServer() {
    if (!newServerUrl.trim()) {
      setSettingsStatus("Enter a media server URL before adding it.");
      return;
    }

    const nextServer: EditableMediaServer = {
      id: createLocalId(),
      name: newServerName.trim() || newServerType,
      type: newServerType,
      serverUrl: newServerUrl.trim(),
      apiKey: newServerApiKey,
      hasApiKey: Boolean(newServerApiKey.trim()),
      isDefault: makeNewServerDefault || mediaServers.length === 0,
      lastSyncedUtc: null,
      libraries: []
    };

    setMediaServers((current) => {
      const normalized = nextServer.isDefault
        ? current.map((server) => ({ ...server, isDefault: false }))
        : current;
      return [...normalized, nextServer];
    });
    setNewServerName("Media Server");
    setNewServerUrl("");
    setNewServerApiKey("");
    setMakeNewServerDefault(false);
    setSettingsStatus("Media server added. Save settings, then sync libraries.");
  }

  function updateMediaServer(id: string, patch: Partial<EditableMediaServer>) {
    setMediaServers((current) => current.map((server) => {
      if (server.id !== id) return patch.isDefault ? { ...server, isDefault: false } : server;
      return { ...server, ...patch };
    }));
  }

  function removeMediaServer(id: string) {
    setMediaServers((current) => current.filter((server) => server.id !== id));
    setSettingsStatus("Media server removed. Save settings to apply.");
  }

  async function testServer(server: EditableMediaServer) {
    setSettingsStatus(`Testing ${server.name}...`);
    try {
      const result = await testMediaServerConnection({
        id: server.id,
        name: server.name,
        type: server.type,
        serverUrl: server.serverUrl,
        apiKey: server.apiKey
      });
      setSettingsStatus(result.status);
    } catch (error) {
      setSettingsStatus(error instanceof Error ? error.message : "Media server test failed.");
    }
  }

  async function syncServer(server: EditableMediaServer) {
    setSettingsStatus(`Saving and syncing ${server.name}...`);
    const saved = await saveSettings();
    const savedServer = saved?.mediaServers.find((item) => item.id === server.id);
    if (!savedServer) {
      setSettingsStatus("Save the media server before syncing.");
      return;
    }

    try {
      const result = await syncMediaServerLibraries(savedServer.id);
      setMediaServers((current) => current.map((item) => item.id === savedServer.id ? { ...result.server, apiKey: "" } : item));
      setSettingsStatus(result.status);
      webSettings.refetch();
      status.refetch();
    } catch (error) {
      setSettingsStatus(error instanceof Error ? error.message : "Media server sync failed.");
    }
  }

  function reloadTheme() {
    const theme = applyWebTheme(themeName);
    setThemeJson(JSON.stringify(theme, null, 2));
    setSettingsStatus(`Theme reloaded: ${theme.name}`);
  }

  function saveCustomTheme() {
    try {
      const parsed = JSON.parse(themeJson);
      const name = customThemeName.trim() || parsed.name || "Custom Theme";
      const nextThemes = saveCustomWebTheme({
        name,
        colors: parsed.colors ?? parsed.Colors ?? {}
      });
      setAvailableThemes(nextThemes);
      setThemeName(name);
      applyWebTheme(name);
      setThemeJson(JSON.stringify(getWebTheme(name), null, 2));
      setSettingsStatus(`Custom theme saved: ${name}`);
    } catch (error) {
      setSettingsStatus(error instanceof Error ? error.message : "Theme JSON is not valid.");
    }
  }

  function removeSelectedCustomTheme() {
    const nextThemes = removeCustomWebTheme(themeName);
    setAvailableThemes(nextThemes);
    const nextName = getStoredWebThemeName();
    setThemeName(nextName);
    applyWebTheme(nextName);
    setThemeJson(JSON.stringify(getWebTheme(nextName), null, 2));
    setSettingsStatus(`Custom theme removed: ${themeName}`);
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <SectionHeader title="Settings" description="Configure web GUI behavior, provider keys, presets, library paths, themes, and bundled media tools." />

      <section className="flex min-h-0 flex-1 flex-col rounded-xl border border-border bg-card shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-border p-3">
          <div className="flex flex-wrap items-center gap-2">
            {settingsTabs.map((tab) => (
              <SettingsTabButton key={tab.id} tab={tab} active={activeTab === tab.id} onSelect={setActiveTab} />
            ))}
          </div>
          <div className="flex items-center gap-3">
            <span className="max-w-[360px] truncate text-sm text-success" title={settingsStatus}>{settingsStatus}</span>
            <button
              type="button"
              onClick={saveSettings}
              className="h-9 rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover"
            >
              Save Settings
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto p-5">
          {activeTab === "general" ? (
            <div className="grid gap-5 xl:grid-cols-[minmax(360px,520px)_1fr]">
              <SettingsCard title="Container" description="Paths are resolved inside the Docker container, not from the Windows host.">
                <dl className="grid grid-cols-[140px_1fr] gap-x-4 gap-y-3 text-sm">
                  <dt className="text-muted">Media Root</dt>
                  <dd className="font-mono text-text">{status.data?.mediaRoot ?? "/media"}</dd>
                  <dt className="text-muted">Config Root</dt>
                  <dd className="font-mono text-text">{status.data?.configRoot ?? "/config"}</dd>
                </dl>
              </SettingsCard>

              <SettingsCard title="Media Tools" description="The container bundles MKVToolNix and FFmpeg for scan, remux, and property workflows.">
                <div className="overflow-auto rounded-lg border border-border bg-panel">
                  <table className="w-full min-w-[620px] border-collapse text-left text-sm">
                    <thead className="bg-panel text-xs uppercase tracking-wide text-subtle">
                      <tr>
                        <th className="border-b border-border px-3 py-2">Tool</th>
                        <th className="border-b border-border px-3 py-2">Status</th>
                        <th className="border-b border-border px-3 py-2">Path</th>
                        <th className="border-b border-border px-3 py-2">Version</th>
                      </tr>
                    </thead>
                    <tbody>
                      {(status.data?.tools ?? []).map((tool) => (
                        <tr key={tool.name} className="bg-card hover:bg-selected">
                          <td className="border-b border-border px-3 py-2 font-semibold">{tool.name}</td>
                          <td className="border-b border-border px-3 py-2">
                            <span className={tool.available ? "inline-flex items-center gap-2 text-success" : "inline-flex items-center gap-2 text-warning"}>
                              {tool.available ? <CheckCircle2 size={15} /> : <CircleAlert size={15} />}
                              {tool.available ? "Available" : "Missing"}
                            </span>
                          </td>
                          <td className="border-b border-border px-3 py-2 font-mono text-xs text-muted">{tool.resolvedPath}</td>
                          <td className="border-b border-border px-3 py-2 text-muted">{tool.version || "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "rename" ? (
            <div className="grid gap-5 xl:grid-cols-[minmax(360px,520px)_1fr]">
              <SettingsCard title="API Providers" description="TVDB and TMDB lookup requires your own API keys. Leave saved key fields blank to keep existing values.">
                <div className="grid gap-4 md:grid-cols-2">
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">TVDB API Key</span>
                    <input
                      value={tvdbApiKey}
                      onChange={(event) => setTvdbApiKey(event.target.value)}
                      placeholder={webSettings.data?.hasTvdbApiKey ? "Saved - leave blank to keep" : "User-provided TVDB API key"}
                      type="password"
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">TVDB PIN</span>
                    <input
                      value={tvdbPin}
                      onChange={(event) => setTvdbPin(event.target.value)}
                      placeholder={webSettings.data?.hasTvdbPin ? "Saved - leave blank to keep" : "Optional TVDB subscriber PIN"}
                      type="password"
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">TMDB API Key</span>
                    <input
                      value={tmdbApiKey}
                      onChange={(event) => setTmdbApiKey(event.target.value)}
                      placeholder={webSettings.data?.hasTmdbApiKey ? "Saved - leave blank to keep" : "User-provided TMDB API key"}
                      type="password"
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">Metadata Language</span>
                    <input
                      value={language}
                      onChange={(event) => setLanguage(event.target.value)}
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">Default Rename Provider</span>
                    <select
                      value={provider}
                      onChange={(event) => setProvider(event.target.value)}
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    >
                      <option value="TVDB">TVDB</option>
                      <option value="TMDB">TMDB</option>
                    </select>
                  </label>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">Default Rename Template</span>
                    <input
                      value={template}
                      onChange={(event) => setTemplate(event.target.value)}
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    />
                  </label>
                </div>
              </SettingsCard>

              <SettingsCard title="Rename Templates" description="One template per line. The selected default template is always preserved when settings are saved.">
                <p className="text-xs leading-5 text-muted">
                  Series templates can use {"{series}"}, {"{season:00}"}, {"{episode:00}"}, {"{episodeTitle}"}, and {"{absolute:000}"}. Movie templates can use {"{title}"} and {"{year}"}.
                </p>
                <textarea
                  value={renameTemplatesText}
                  onChange={(event) => setRenameTemplatesText(event.target.value)}
                  rows={12}
                  className="mt-3 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
                />
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "presets" ? (
            <div className="grid gap-5 xl:grid-cols-[2fr_1fr]">
              <SettingsCard title="Track Presets" description="These lists feed Rename language choices and Track Properties name/language selectors.">
                <div className="grid gap-4 lg:grid-cols-3">
                  <PresetEditor label="Audio Name Presets" value={audioNamePresetsText} onChange={setAudioNamePresetsText} />
                  <PresetEditor label="Subtitle Name Presets" value={subtitleNamePresetsText} onChange={setSubtitleNamePresetsText} />
                  <PresetEditor label="Language Presets" value={languagePresetsText} onChange={setLanguagePresetsText} />
                </div>
              </SettingsCard>

              <SettingsCard title="Mux / Remux Defaults" description="Default keep-language values for remux track removal workflows.">
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Default audio languages to keep</span>
                  <input
                    value={muxAudioDefaults}
                    onChange={(event) => setMuxAudioDefaults(event.target.value)}
                    placeholder="eng,jpn"
                    className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
                <label className="mt-4 block">
                  <span className="text-xs font-semibold text-muted">Default subtitle languages to keep</span>
                  <input
                    value={muxSubtitleDefaults}
                    onChange={(event) => setMuxSubtitleDefaults(event.target.value)}
                    placeholder="eng"
                    className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "library" ? (
            <div className="grid gap-5 xl:grid-cols-[minmax(420px,600px)_1fr]">
              <SettingsCard title="Manual Watch Folders" description="Default fallback paths. These are always available even when no media server is configured.">
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Watch folders</span>
                  <textarea
                    value={watchFoldersText}
                    onChange={(event) => setWatchFoldersText(event.target.value)}
                    rows={10}
                    placeholder={"/media/anime\n/media/movies"}
                    className="mt-2 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
                <label className="flex items-center gap-2 text-sm text-muted">
                  <input
                    type="checkbox"
                    checked={liveWatcherEnabled}
                    onChange={(event) => setLiveWatcherEnabled(event.target.checked)}
                  />
                  Enable live watch-folder monitoring
                </label>
                <div className="mt-4 rounded-lg border border-border bg-panel p-3 text-xs leading-5 text-subtle">
                  Use container paths here. For Docker bind mounts, that usually means paths under <span className="font-mono text-text">/media</span> or <span className="font-mono text-text">/downloads</span>.
                </div>
              </SettingsCard>

              <SettingsCard title="Media Servers" description="Optional discovery for Emby, Jellyfin, or Plex library paths. Add path mappings here when server paths differ from Docker container paths.">
                <div className="space-y-3">
                  {mediaServers.length === 0 ? (
                    <div className="rounded-lg border border-border bg-input p-3 text-sm text-subtle">
                      No media servers configured. Manual watch folders remain the fallback.
                    </div>
                  ) : mediaServers.map((server) => (
                    <div key={server.id} className="rounded-lg border border-border bg-input p-3">
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-[240px] flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <input
                              value={server.name}
                              onChange={(event) => updateMediaServer(server.id, { name: event.target.value })}
                              className="h-9 min-w-[180px] flex-1 rounded-md border border-border bg-card px-3 text-sm font-semibold text-text outline-none focus:border-accent"
                            />
                            <select
                              value={server.type}
                              onChange={(event) => updateMediaServer(server.id, { type: event.target.value })}
                              className="h-9 rounded-md border border-border bg-card px-3 text-sm text-text outline-none focus:border-accent"
                            >
                              <option value="Emby">Emby</option>
                              <option value="Jellyfin">Jellyfin</option>
                              <option value="Plex">Plex</option>
                            </select>
                            {server.isDefault ? <span className="rounded bg-accent/20 px-2 py-1 text-[11px] font-semibold text-accent">default</span> : null}
                          </div>
                          <input
                            value={server.serverUrl}
                            onChange={(event) => updateMediaServer(server.id, { serverUrl: event.target.value })}
                            placeholder="http://localhost:8096"
                            className="mt-2 h-9 w-full rounded-md border border-border bg-card px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                          />
                          <input
                            value={server.apiKey ?? ""}
                            onChange={(event) => updateMediaServer(server.id, { apiKey: event.target.value })}
                            placeholder={server.hasApiKey ? "Saved API key - leave blank to keep" : "API key or token"}
                            type="password"
                            className="mt-2 h-9 w-full rounded-md border border-border bg-card px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                          />
                          <label className="mt-2 flex items-center gap-2 text-xs text-muted">
                            <input
                              type="checkbox"
                              checked={server.isDefault}
                              onChange={(event) => updateMediaServer(server.id, { isDefault: event.target.checked })}
                            />
                            Use as default media server
                          </label>
                        </div>
                        <div className="flex shrink-0 flex-wrap gap-2">
                          <button
                            type="button"
                            onClick={() => testServer(server)}
                            className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                          >
                            <CheckCircle2 size={14} />
                            Test
                          </button>
                          <button
                            type="button"
                            onClick={() => syncServer(server)}
                            className="inline-flex h-9 items-center gap-2 rounded-md bg-accent px-3 text-xs font-semibold text-window transition hover:bg-accent-hover"
                          >
                            <RefreshCw size={14} />
                            Sync
                          </button>
                          <button
                            type="button"
                            onClick={() => removeMediaServer(server.id)}
                            className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                          >
                            <Trash2 size={14} />
                          </button>
                        </div>
                      </div>
                      {server.libraries.length > 0 ? (
                        <div className="mt-3 max-h-36 overflow-auto rounded-md border border-border bg-card">
                          {server.libraries.map((library) => (
                            <label key={library.id} className="grid grid-cols-[24px_minmax(120px,180px)_1fr] gap-2 border-b border-border px-3 py-2 text-xs last:border-b-0">
                              <input
                                type="checkbox"
                                checked={library.isEnabled}
                                onChange={(event) => updateMediaServer(server.id, {
                                  libraries: server.libraries.map((item) => item.id === library.id ? { ...item, isEnabled: event.target.checked } : item)
                                })}
                              />
                              <span className="truncate font-semibold text-text" title={library.name}>{library.name}</span>
                              <span className="truncate font-mono text-subtle" title={`${library.serverPath} -> ${library.containerPath}`}>
                                {library.containerPath}
                              </span>
                            </label>
                          ))}
                        </div>
                      ) : (
                        <div className="mt-3 rounded-md border border-border bg-card p-2 text-xs text-subtle">
                          No synced libraries yet. Save settings, then click Sync.
                        </div>
                      )}
                      {server.lastSyncedUtc ? (
                        <div className="mt-2 text-[11px] text-subtle">Last synced: {formatDateTime(server.lastSyncedUtc)}</div>
                      ) : null}
                    </div>
                  ))}
                </div>

                <div className="mt-4 rounded-lg border border-border bg-panel p-3">
                  <h3 className="text-sm font-semibold">Add a server</h3>
                  <div className="mt-3 grid gap-3 md:grid-cols-2">
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">Name</span>
                      <input
                        value={newServerName}
                        onChange={(event) => setNewServerName(event.target.value)}
                        className="mt-2 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                      />
                    </label>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">Type</span>
                      <select
                        value={newServerType}
                        onChange={(event) => setNewServerType(event.target.value)}
                        className="mt-2 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                      >
                        <option value="Emby">Emby</option>
                        <option value="Jellyfin">Jellyfin</option>
                        <option value="Plex">Plex</option>
                      </select>
                    </label>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">Server URL</span>
                      <input
                        value={newServerUrl}
                        onChange={(event) => setNewServerUrl(event.target.value)}
                        placeholder="http://localhost:8096"
                        className="mt-2 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                      />
                    </label>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">API key / token</span>
                      <input
                        value={newServerApiKey}
                        onChange={(event) => setNewServerApiKey(event.target.value)}
                        type="password"
                        className="mt-2 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                      />
                    </label>
                  </div>
                  <div className="mt-3 flex flex-wrap items-center gap-3">
                    <label className="flex items-center gap-2 text-xs text-muted">
                      <input
                        type="checkbox"
                        checked={makeNewServerDefault}
                        onChange={(event) => setMakeNewServerDefault(event.target.checked)}
                      />
                      Use as default server
                    </label>
                    <button
                      type="button"
                      onClick={addMediaServer}
                      className="inline-flex h-9 items-center gap-2 rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover"
                    >
                      <Plus size={16} />
                      Add Server
                    </button>
                  </div>
                </div>

                <div className="mt-4 rounded-lg border border-border bg-panel p-3">
                  <h3 className="text-sm font-semibold">Path Mapping</h3>
                  <p className="mt-2 text-xs leading-5 text-muted">
                    Translate paths reported by the media server into paths available inside this MKVO container. One mapping per line.
                  </p>
                  <textarea
                    value={mediaServerMappingsText}
                    onChange={(event) => setMediaServerMappingsText(event.target.value)}
                    rows={5}
                    placeholder={"\\\\server\\media => /media\n/mnt/media => /media"}
                    className="mt-3 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                  <div className="mt-3 rounded-md border border-border bg-input p-3 text-xs leading-5 text-subtle">
                    If Emby reports <span className="font-mono text-text">\\server\media\Anime</span> and Docker mounts that share at <span className="font-mono text-text">/media/Anime</span>, add <span className="font-mono text-text">\\server\media =&gt; /media</span>.
                  </div>
                </div>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "appearance" ? (
            <div className="grid gap-5 xl:grid-cols-[minmax(420px,1fr)_280px]">
              <SettingsCard title="Theme" description="Themes apply to this browser session and are remembered locally for the web GUI.">
                <div className="flex max-w-xl items-end gap-3">
                  <label className="block flex-1">
                    <span className="text-xs font-semibold text-muted">Theme</span>
                    <select
                      value={themeName}
                      onChange={(event) => {
                        const nextName = event.target.value;
                        setThemeName(nextName);
                        setThemeJson(JSON.stringify(getWebTheme(nextName), null, 2));
                      }}
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    >
                      {availableThemes.map((theme) => (
                        <option key={theme.name} value={theme.name}>{theme.name}</option>
                      ))}
                    </select>
                  </label>
                  <button
                    type="button"
                    onClick={reloadTheme}
                    className="h-10 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                  >
                    Reload Theme
                  </button>
                </div>
                <label className="mt-4 block">
                  <span className="text-xs font-semibold text-muted">Theme JSON</span>
                  <textarea
                    value={themeJson}
                    onChange={(event) => setThemeJson(event.target.value)}
                    rows={16}
                    className="mt-2 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
              </SettingsCard>

              <SettingsCard title="Custom Theme" description="Save edited JSON as a named custom theme or remove a selected custom theme.">
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Custom Theme Name</span>
                  <input
                    value={customThemeName}
                    onChange={(event) => setCustomThemeName(event.target.value)}
                    className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                  />
                </label>
                <button
                  type="button"
                  onClick={saveCustomTheme}
                  className="mt-4 h-10 w-full rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover"
                >
                  Save Custom Theme
                </button>
                <button
                  type="button"
                  onClick={removeSelectedCustomTheme}
                  disabled={["Dark", "Midnight", "Light"].includes(themeName)}
                  className="mt-2 h-10 w-full rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
                >
                  Remove Custom Theme
                </button>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "about" ? (
            <div className="grid gap-5 xl:grid-cols-[minmax(440px,720px)_1fr]">
              <SettingsCard title="About MKV Orchestrator Web" description="A single-container companion for server or NAS-style access.">
                <div className="space-y-3 text-sm leading-6 text-muted">
                  <p>
                    This web build shares the desktop app's core media logic where possible and exposes it through a React web interface.
                  </p>
                  <p>
                    Current supported workflows include MKV and MP4 scanning, metadata inspection, provider-based rename previews, rename apply and undo batches, MKV mux/remux operations, MKV track property edits, library audit views, logs, and container tool checks.
                  </p>
                  <p>
                    TVDB and TMDB lookup requires your own API keys. Saved keys are written to the container config volume under <span className="font-mono text-text">/config</span>, so do not commit or publish that volume.
                  </p>
                </div>
              </SettingsCard>

              <SettingsCard title="Configuration Safety" description="Deployment-specific data should live in mounted volumes or environment variables, not source control.">
                <div className="space-y-3 text-sm leading-6 text-muted">
                  <p>
                    The repository should contain application code and default examples only. Personal paths, API keys, cache databases, and runtime logs belong outside Git.
                  </p>
                  <p>
                    Docker users should mount media into <span className="font-mono text-text">/media</span> and configuration into <span className="font-mono text-text">/config</span>.
                  </p>
                </div>
              </SettingsCard>
            </div>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function SettingsTabButton({ tab, active, onSelect }: { tab: SettingsTabDefinition; active: boolean; onSelect: (tab: SettingsTabId) => void }) {
  const Icon = tab.Icon;
  return (
    <button
      type="button"
      onClick={() => onSelect(tab.id)}
      className={`inline-flex h-9 items-center gap-2 rounded-md border px-3 text-sm font-semibold transition ${
        active
          ? "border-accent bg-selected text-text"
          : "border-transparent bg-transparent text-muted hover:border-border hover:bg-button-hover hover:text-text"
      }`}
    >
      <Icon size={16} />
      {tab.label}
    </button>
  );
}

function SettingsCard({ title, description, children }: { title: string; description?: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-border bg-panel p-4">
      <h2 className="text-base font-semibold">{title}</h2>
      {description ? <p className="mt-2 text-sm leading-6 text-muted">{description}</p> : null}
      <div className="mt-4">{children}</div>
    </section>
  );
}

function PresetEditor({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <label className="block">
      <span className="text-xs font-semibold text-muted">{label}</span>
      <textarea
        value={value}
        onChange={(event) => onChange(event.target.value)}
        rows={10}
        className="mt-2 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
      />
    </label>
  );
}

function normalizeLineList(value: string) {
  const seen = new Set<string>();
  return value
    .split(/\r?\n/g)
    .map((item) => item.trim())
    .filter((item) => {
      if (!item || seen.has(item.toLowerCase())) return false;
      seen.add(item.toLowerCase());
      return true;
    });
}

function normalizeRenameTemplates(value: string, selectedTemplate: string) {
  const seen = new Set<string>();
  return [selectedTemplate, ...value.split(/\r?\n/g)]
    .map((template) => template.trim())
    .filter((template) => {
      if (!template || seen.has(template.toLowerCase())) return false;
      seen.add(template.toLowerCase());
      return true;
    });
}

function parsePathMappings(value: string): WebMediaServerPathMapping[] {
  const seen = new Set<string>();
  return value
    .split(/\r?\n/g)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const separator = line.includes("=>") ? "=>" : "=";
      const [serverPathPrefix, containerPathPrefix] = line.split(separator).map((part) => part.trim());
      return { serverPathPrefix, containerPathPrefix };
    })
    .filter((mapping) => {
      if (!mapping.serverPathPrefix || !mapping.containerPathPrefix) return false;
      const key = mapping.serverPathPrefix.toLowerCase();
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
}

function formatPathMappings(mappings: WebMediaServerPathMapping[]) {
  return mappings
    .map((mapping) => `${mapping.serverPathPrefix} => ${mapping.containerPathPrefix}`)
    .join("\n");
}

function createLocalId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }

  return `local-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function formatDateTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function readStoredSettingsTab(): SettingsTabId {
  if (typeof window === "undefined") return "general";
  const stored = window.localStorage.getItem(settingsTabStorageKey);
  return stored && isSettingsTab(stored) ? stored : "general";
}

function isSettingsTab(value: string): value is SettingsTabId {
  return settingsTabs.some((tab) => tab.id === value);
}
