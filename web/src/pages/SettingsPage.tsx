import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowDown,
  ArrowUp,
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
  Wrench,
  X
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  getBackendTransport,
  getStatus,
  getWebSettings,
  saveWebSettings,
  syncMediaServerLibraries,
  testMediaServerConnection,
  testRenameProvider
} from "../api";
import type { SourceRoot, WebMediaServer, WebSettings, WebSettingsRequest } from "../api";
import { FileBrowser } from "../components/FileBrowser";
import { SectionHeader } from "../components/SectionHeader";
import SecuritySection from "../components/SecuritySection";
import ffmpegLogo from "../assets/logos/ffmpeg.png";
import mkvtoolnixLogo from "../assets/logos/mkvtoolnix.png";
import tmdbLogo from "../assets/logos/tmdb.png";
import tvdbLogo from "../assets/logos/tvdb.png";
import { applyWebTheme, getAllWebThemes, getStoredWebThemeName, getWebTheme, loadCustomWebThemes, normalizeWebTheme, removeCustomWebTheme, replaceCustomWebThemes, saveCustomWebTheme, webThemes } from "../theme";

const settingsTabStorageKey = "mkvo.web.settingsTab";
const defaultRenameTemplate = "{series} - S{season:00}E{episode:00} - {episodeTitle}";
const defaultRenameTemplates = [
  "{title}",
  "{title} ({year})",
  defaultRenameTemplate,
  "S{season:00}E{episode:00} - {episodeTitle}",
  "{series} - {absolute:000} - {episodeTitle}"
];
const defaultAudioNamePresets = ["English", "Japanese", "Commentary"];
const defaultSubtitleNamePresets = ["Dialogue", "English", "English Forced", "English SDH", "Signs & Songs", "Fansub"];
const defaultLanguagePresets = ["eng", "jpn", "kor", "und"];
const defaultIgnoredSubfolders = ["Backdrops", "Extras", "Featurettes", "OVAs", "Sample", "Samples", "Specials", "Trailer", "Trailers"];
const autoSaveDelayMillis = 900;
const savedSecretPlaceholder = "••••••••••••";

/**
 * Third-party notices shown in About.
 *
 * The TMDB wording is prescribed by their API terms rather than chosen, so it
 * is reproduced verbatim; the rest match docs/ATTRIBUTION_AND_LOGOS.md.
 */
const attributions = [
  {
    name: "TMDB",
    logo: tmdbLogo,
    notice: "This product uses the TMDB API but is not endorsed or certified by TMDB."
  },
  { name: "TheTVDB", logo: tvdbLogo, notice: "Metadata provided by TheTVDB." },
  {
    name: "MKVToolNix",
    logo: mkvtoolnixLogo,
    notice:
      "MKVToolNix tools are used for MKV remuxing, metadata editing, extraction, and analysis."
  },
  { name: "FFmpeg", logo: ffmpegLogo, notice: "FFmpeg and ffprobe are used for media metadata analysis." }
] as const;

const settingsTabs = [
  { id: "general", label: "General", Icon: Wrench },
  { id: "providers", label: "API Providers", Icon: KeyRound },
  { id: "rename", label: "Rename", Icon: KeyRound },
  { id: "presets", label: "Presets", Icon: SlidersHorizontal },
  { id: "library", label: "Library", Icon: Database },
  { id: "appearance", label: "Appearance", Icon: Palette },
  { id: "security", label: "Privacy / Security", Icon: KeyRound },
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

const themeColorOptions = [
  { name: "Accent", label: "Accent", cssVariable: "--color-accent" },
  { name: "AccentHover", label: "Accent hover", cssVariable: "--color-accent-hover" },
  { name: "AppTitle", label: "AppTitle", cssVariable: "--color-app-title" },
  { name: "Border", label: "Border", cssVariable: "--color-border" },
  { name: "BorderStrong", label: "Strong border", cssVariable: "--color-border-strong" },
  { name: "Brand", label: "Brand", cssVariable: "--color-brand" },
  { name: "Button", label: "Button", cssVariable: "--color-button" },
  { name: "ButtonHover", label: "Button hover", cssVariable: "--color-button-hover" },
  { name: "Card", label: "Card", cssVariable: "--color-card" },
  { name: "Disabled", label: "Disabled", cssVariable: "--color-disabled" },
  { name: "Input", label: "Input", cssVariable: "--color-input" },
  { name: "InputHover", label: "Input hover", cssVariable: "--color-input-hover" },
  { name: "Muted", label: "Muted text", cssVariable: "--color-muted" },
  { name: "Panel", label: "Panel", cssVariable: "--color-panel" },
  { name: "Selected", label: "Selected", cssVariable: "--color-selected" },
  { name: "Sidebar", label: "Sidebar", cssVariable: "--color-sidebar" },
  { name: "Subtle", label: "Subtle text", cssVariable: "--color-subtle" },
  { name: "Success", label: "Success", cssVariable: "--color-success" },
  { name: "TemplateHighlight", label: "Template highlight", cssVariable: "--color-template" },
  { name: "Text", label: "Primary text", cssVariable: "--color-text" },
  { name: "Warning", label: "Warning", cssVariable: "--color-warning" },
  { name: "Window", label: "Window background", cssVariable: "--color-window" }
] as const;

type ThemeColorName = typeof themeColorOptions[number]["name"];

export function SettingsPage() {
  const queryClient = useQueryClient();
  const backendTransport = getBackendTransport();
  const isDesktop = backendTransport === "tauri";
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const webSettings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const [activeTab, setActiveTab] = useState<SettingsTabId>(() => readStoredSettingsTab());
  const [tvdbApiKey, setTvdbApiKey] = useState("");
  const [tvdbPin, setTvdbPin] = useState("");
  const [tmdbApiKey, setTmdbApiKey] = useState("");
  const [anidbClient, setAnidbClient] = useState("");
  const [testingProvider, setTestingProvider] = useState<string | null>(null);
  const [language, setLanguage] = useState("eng");
  const [provider, setProvider] = useState("TVDB");
  const [template, setTemplate] = useState(defaultRenameTemplate);
  const [renameTemplatesText, setRenameTemplatesText] = useState("");
  const [audioNamePresetsText, setAudioNamePresetsText] = useState("");
  const [subtitleNamePresetsText, setSubtitleNamePresetsText] = useState("");
  const [languagePresetsText, setLanguagePresetsText] = useState("");
  const [ignoredSubfoldersText, setIgnoredSubfoldersText] = useState("");
  const [muxAudioDefaults, setMuxAudioDefaults] = useState("eng,jpn");
  const [muxSubtitleDefaults, setMuxSubtitleDefaults] = useState("eng");
  const [mkvToolNixDirectory, setMkvToolNixDirectory] = useState("");
  const [ffmpegDirectory, setFfmpegDirectory] = useState("");
  const [libraryRoots, setLibraryRoots] = useState<SourceRoot[]>([]);
  const [defaultDirectory, setDefaultDirectory] = useState("");
  const [defaultDirectoryName, setDefaultDirectoryName] = useState("Home");
  // Which row the browser is filling in, so one dialog serves every row.
  const [browsingRow, setBrowsingRow] = useState<number | "home" | "mkvtoolnix" | "ffmpeg" | null>(null);
  const [watchFoldersText, setWatchFoldersText] = useState("");
  const [liveWatcherEnabled, setLiveWatcherEnabled] = useState(false);
  const [mediaServers, setMediaServers] = useState<EditableMediaServer[]>([]);
  const [newServerName, setNewServerName] = useState("Media Server");
  const [newServerType, setNewServerType] = useState("Emby");
  const [newServerUrl, setNewServerUrl] = useState("");
  const [newServerApiKey, setNewServerApiKey] = useState("");
  const [makeNewServerDefault, setMakeNewServerDefault] = useState(false);
  const [availableThemes, setAvailableThemes] = useState(() => getAllWebThemes());
  const [themeName, setThemeName] = useState(() => getStoredWebThemeName());
  const [themeJson, setThemeJson] = useState(() => JSON.stringify(getWebTheme(getStoredWebThemeName()), null, 2));
  const [selectedThemeColor, setSelectedThemeColor] = useState<ThemeColorName>("AppTitle");
  const [customThemeName, setCustomThemeName] = useState("My Theme");
  const [settingsStatus, setSettingsStatus] = useState("");
  const lastSavedFingerprint = useRef("");

  useEffect(() => {
    if (typeof window !== "undefined") {
      window.localStorage.setItem(settingsTabStorageKey, activeTab);
    }
  }, [activeTab]);

  useEffect(() => {
    if (!webSettings.data) return;
    setLanguage(webSettings.data.tvdbLanguage || "eng");
    setProvider(webSettings.data.renameLookupProvider || "TVDB");
    setTemplate(webSettings.data.renameTemplate || defaultRenameTemplate);
    setRenameTemplatesText((webSettings.data.renameTemplates ?? []).join("\n"));
    setAudioNamePresetsText((webSettings.data.audioNamePresets ?? []).join("\n"));
    setSubtitleNamePresetsText((webSettings.data.subtitleNamePresets ?? []).join("\n"));
    setLanguagePresetsText((webSettings.data.languagePresets ?? []).join("\n"));
    setIgnoredSubfoldersText((webSettings.data.ignoredScanFolderNames ?? []).join("\n"));
    setMuxAudioDefaults(webSettings.data.mkvMergeDefaultAudioLanguages || "eng,jpn");
    setMuxSubtitleDefaults(webSettings.data.mkvMergeDefaultSubtitleLanguages || "eng");
    setMkvToolNixDirectory(webSettings.data.mkvToolNixDirectory ?? "");
    setFfmpegDirectory(webSettings.data.ffmpegDirectory ?? "");
    const loadedRoots = webSettings.data.libraryRoots ?? [];
    const loadedHome = webSettings.data.defaultRoot
      ?? loadedRoots[0]?.path
      ?? (isDesktop ? "" : status.data?.mediaRoot || "/media");
    const loadedHomeName = webSettings.data.defaultRoot
      ? webSettings.data.defaultRootName
      : loadedRoots[0]?.name || webSettings.data.defaultRootName;
    setDefaultDirectory(loadedHome);
    setDefaultDirectoryName(loadedHomeName || "Home");
    setLibraryRoots(loadedRoots.filter((root) => !sameFolderPath(root.path, loadedHome)));
    setWatchFoldersText((webSettings.data.watchFolders ?? []).join("\n"));
    setLiveWatcherEnabled(Boolean(webSettings.data.enableLiveWatchFolderMonitoring));
    setMediaServers((webSettings.data.mediaServers ?? []).map((server) => ({ ...server, apiKey: "" })));
    const themes = replaceCustomWebThemes(webSettings.data.customThemes ?? []);
    setAvailableThemes(themes);
    const selectedTheme = getWebTheme(webSettings.data.selectedThemeName).name;
    setThemeName(selectedTheme);
    const applied = applyWebTheme(selectedTheme);
    setThemeJson(JSON.stringify(applied, null, 2));
    lastSavedFingerprint.current = settingsFingerprint(settingsRequestFromSaved(webSettings.data));
  }, [isDesktop, status.data?.mediaRoot, webSettings.data]);

  const pendingSettingsRequest: WebSettingsRequest = {
    tvdbApiKey: tvdbApiKey || undefined,
    tvdbPin: tvdbPin || undefined,
    tmdbApiKey: tmdbApiKey || undefined,
    anidbClient: anidbClient || undefined,
    tvdbLanguage: language,
    renameLookupProvider: provider,
    renameTemplate: template,
    renameTemplates: normalizeRenameTemplates(renameTemplatesText, template),
    audioNamePresets: normalizeLineList(audioNamePresetsText),
    subtitleNamePresets: normalizeLineList(subtitleNamePresetsText),
    languagePresets: normalizeLineList(languagePresetsText),
    ignoredScanFolderNames: normalizeLineList(ignoredSubfoldersText),
    mkvMergeDefaultAudioLanguages: muxAudioDefaults,
    mkvMergeDefaultSubtitleLanguages: muxSubtitleDefaults,
    mkvToolNixDirectory: mkvToolNixDirectory.trim() || null,
    ffmpegDirectory: ffmpegDirectory.trim() || null,
    defaultRoot: defaultDirectory.trim() || null,
    defaultRootName: defaultDirectoryName.trim() || "Home",
    libraryRoots: libraryRoots
      .map((root) => ({ name: root.name.trim(), path: root.path.trim() }))
      .filter((root) => root.name && root.path),
    watchFolders: normalizeLineList(watchFoldersText),
    enableLiveWatchFolderMonitoring: liveWatcherEnabled,
    selectedThemeName: themeName,
    customThemes: loadCustomWebThemes(),
    mediaServers: mediaServers.map((server) => ({
      id: server.id,
      name: server.name,
      type: server.type,
      serverUrl: server.serverUrl,
      apiKey: server.apiKey || undefined,
      isDefault: server.isDefault,
      libraries: server.libraries
    }))
  };
  const pendingSettingsFingerprint = settingsFingerprint(pendingSettingsRequest);

  async function saveSettings(
    request: WebSettingsRequest = pendingSettingsRequest,
    automatic = false
  ): Promise<WebSettings | null> {
    try {
      if (automatic) setSettingsStatus("Saving changes...");
      const saved = await saveWebSettings(request);
      lastSavedFingerprint.current = settingsFingerprint(request);
      queryClient.setQueryData(["web-settings"], saved);
      queryClient.setQueryData(["settings"], saved);

      if (automatic) {
        setSettingsStatus("Settings saved automatically.");
        status.refetch();
        return saved;
      }

      setTvdbApiKey("");
      setTvdbPin("");
      setTmdbApiKey("");
      setAnidbClient("");
      setSettingsStatus("Settings saved.");
      webSettings.refetch();
      setLanguage(saved.tvdbLanguage);
      setProvider(saved.renameLookupProvider);
      setTemplate(saved.renameTemplate);
      setRenameTemplatesText(saved.renameTemplates.join("\n"));
      setAudioNamePresetsText(saved.audioNamePresets.join("\n"));
      setSubtitleNamePresetsText(saved.subtitleNamePresets.join("\n"));
      setLanguagePresetsText(saved.languagePresets.join("\n"));
      setIgnoredSubfoldersText(saved.ignoredScanFolderNames.join("\n"));
      setMuxAudioDefaults(saved.mkvMergeDefaultAudioLanguages);
      setMuxSubtitleDefaults(saved.mkvMergeDefaultSubtitleLanguages);
      setMkvToolNixDirectory(saved.mkvToolNixDirectory ?? "");
      setFfmpegDirectory(saved.ffmpegDirectory ?? "");
      setDefaultDirectory(saved.defaultRoot ?? "");
      setDefaultDirectoryName(saved.defaultRootName || "Home");
      setLibraryRoots(saved.libraryRoots.filter((root) => !sameFolderPath(root.path, saved.defaultRoot ?? "")));
      setWatchFoldersText(saved.watchFolders.join("\n"));
      setLiveWatcherEnabled(saved.enableLiveWatchFolderMonitoring);
      setMediaServers(saved.mediaServers.map((server) => ({ ...server, apiKey: "" })));
      const themes = replaceCustomWebThemes(saved.customThemes);
      setAvailableThemes(themes);
      setThemeName(saved.selectedThemeName);
      setThemeJson(JSON.stringify(applyWebTheme(saved.selectedThemeName), null, 2));
      status.refetch();
      return saved;
    } catch (error) {
      setSettingsStatus(error instanceof Error ? error.message : "Settings could not be saved.");
      return null;
    }
  }

  useEffect(() => {
    if (!webSettings.data || pendingSettingsFingerprint === lastSavedFingerprint.current) return;
    setSettingsStatus("Changes pending...");
    const timer = window.setTimeout(() => {
      void saveSettings(pendingSettingsRequest, true);
    }, autoSaveDelayMillis);
    return () => window.clearTimeout(timer);
  }, [pendingSettingsFingerprint, webSettings.data]);

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

  async function testNewServer() {
    if (!newServerUrl.trim()) {
      setSettingsStatus("Enter a media server URL before testing it.");
      return;
    }

    await testServer({
      id: "new-media-server",
      name: newServerName.trim() || newServerType,
      type: newServerType,
      serverUrl: newServerUrl.trim(),
      apiKey: newServerApiKey,
      hasApiKey: Boolean(newServerApiKey.trim()),
      isDefault: makeNewServerDefault,
      lastSyncedUtc: null,
      libraries: []
    });
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

  async function testProvider(providerName: "TVDB" | "TMDB") {
    setTestingProvider(providerName);
    setSettingsStatus(`Testing ${providerName} connection...`);
    try {
      const result = await testRenameProvider({ provider: providerName, language });
      setSettingsStatus(result.status);
    } catch (error) {
      setSettingsStatus(error instanceof Error ? error.message : "Provider test failed.");
    } finally {
      setTestingProvider(null);
    }
  }

  function reloadTheme() {
    const theme = applyWebTheme(themeName);
    setThemeJson(JSON.stringify(theme, null, 2));
    setSettingsStatus(`Theme reloaded: ${theme.name}`);
  }

  function updateThemeColor(colorName: ThemeColorName, cssVariable: string, color: string) {
    try {
      const parsed = JSON.parse(themeJson);
      parsed.colors = { ...(parsed.colors ?? parsed.Colors ?? {}), [colorName]: color };
      delete parsed.Colors;
      setThemeJson(JSON.stringify(normalizeWebTheme(parsed), null, 2));
      document.documentElement.style.setProperty(cssVariable, color);
      if (colorName === "Window") {
        document.documentElement.style.backgroundColor = color;
        document.body.style.backgroundColor = color;
      } else if (colorName === "Text") {
        document.body.style.color = color;
      }
    } catch {
      setSettingsStatus("Theme JSON must be valid before editing theme colors.");
    }
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
      <SectionHeader title="Settings" description="Configure MKVO behavior, provider keys, presets, library paths, themes, and media tools." />

      <section className="flex min-h-0 flex-1 flex-col rounded-xl border border-border bg-card shadow-[0_1.25rem_3.75rem_rgba(0,0,0,0.18)]">
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-border p-3">
          <div className="flex flex-wrap items-center gap-2">
            {settingsTabs.map((tab) => (
              <SettingsTabButton key={tab.id} tab={tab} active={activeTab === tab.id} onSelect={setActiveTab} />
            ))}
          </div>
          {activeTab !== "security" ? <div className="flex items-center gap-3">
            <span className="max-w-[22.5rem] truncate text-sm text-success" title={settingsStatus}>{settingsStatus}</span>
            <button
              type="button"
              onClick={() => void saveSettings()}
              className="h-9 rounded-md bg-accent px-4 text-sm font-semibold text-window transition hover:bg-accent-hover"
            >
              Save Settings
            </button>
          </div> : null}
        </div>

        <div className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden p-5">
          {activeTab === "general" ? (
            <div className="grid min-w-0 items-start gap-3 xl:h-full xl:grid-cols-[minmax(0,1.1fr)_minmax(25rem,0.9fr)] xl:items-stretch">
              <SettingsCard
                className="flex min-h-0 flex-col xl:h-full"
                contentClassName="flex min-h-0 flex-1 flex-col"
                title={isDesktop ? "Dashboard" : "Default Directory"}
                description={isDesktop
                  ? "Choose the default folder the Dashboard opens for browsing and scans."
                  : "Choose the Home folder used when the Dashboard opens for browsing and scans. The mounted media path is used as the starting value."}
              >
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <input
                    aria-label="Default Directory Name"
                    value={defaultDirectoryName}
                    onChange={(event) => setDefaultDirectoryName(event.target.value)}
                    placeholder="Home"
                    className="h-9 w-32 shrink-0 rounded-md border border-border bg-input px-2 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                  <input
                    aria-label="Default Directory"
                    value={defaultDirectory}
                    onChange={(event) => setDefaultDirectory(event.target.value)}
                    placeholder={isDesktop
                      ? "Choose the folder where browsing and scans should start"
                      : status.data?.mediaRoot || "/media"}
                    className="h-9 min-w-[11.25rem] flex-1 rounded-md border border-border bg-input px-2 font-mono text-xs text-text outline-none placeholder:font-sans placeholder:text-subtle focus:border-accent"
                  />
                  <button
                    type="button"
                    aria-label="Browse for Default Directory"
                    onClick={() => {
                      setBrowsingRow("home");
                    }}
                    className="h-9 shrink-0 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                  >
                    Browse
                  </button>
                </div>

                <div className="mt-4 flex min-h-0 flex-1 flex-col border-t border-border pt-3">
                  <div className="mb-2">
                    <h3 className="text-base font-semibold text-text">Quick Access</h3>
                    <p className="mt-1 text-xs leading-5 text-subtle">
                      {isDesktop
                        ? "Add named folder shortcuts for faster browsing. These are separate from the default directory above."
                        : "Add named folder shortcuts for faster browsing inside the server's mounted storage."}
                    </p>
                  </div>
                  <div className="min-h-0 flex-1 space-y-2 overflow-auto pr-1">
                    {libraryRoots.length === 0 ? (
                      <div className="rounded-lg border border-border bg-input p-3 text-sm text-subtle">
                        {isDesktop
                          ? "No quick-access folders yet. Browsing starts at This PC until you add one."
                          : "No quick-access folders yet. Browsing starts at the media root until you add one."}
                      </div>
                    ) : null}

                    {libraryRoots.map((root, index) => (
                      <div key={index} className="grid items-center gap-2 sm:grid-cols-[7rem_minmax(10rem,1fr)_auto_auto_auto_auto]">
                        <input
                          value={root.name}
                          onChange={(event) =>
                            setLibraryRoots((current) =>
                              current.map((item, position) =>
                                position === index ? { ...item, name: event.target.value } : item
                              )
                            )
                          }
                          placeholder="Anime"
                          aria-label={`Quick access folder ${index + 1} name`}
                          className="h-9 w-full rounded-md border border-border bg-input px-2 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                        />
                        <input
                          value={root.path}
                          onChange={(event) =>
                            setLibraryRoots((current) =>
                              current.map((item, position) =>
                                position === index ? { ...item, path: event.target.value } : item
                              )
                            )
                          }
                          placeholder={isDesktop ? "D:\\Anime" : "/mnt/user/anime"}
                          aria-label={`Quick access folder ${index + 1} path`}
                          className="h-9 min-w-0 rounded-md border border-border bg-input px-2 font-mono text-xs text-text outline-none placeholder:text-subtle focus:border-accent"
                        />
                        <button
                          type="button"
                          aria-label={`Browse for quick access folder ${index + 1}`}
                          onClick={() => setBrowsingRow(index)}
                          className="h-9 shrink-0 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                        >
                          Browse
                        </button>
                        <button
                          type="button"
                          disabled={index === 0}
                          onClick={() =>
                            setLibraryRoots((current) => moveItem(current, index, index - 1))
                          }
                          aria-label={`Move quick access folder ${index + 1} up`}
                          title="Move up"
                          className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-subtle transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
                        >
                          <ArrowUp size={14} />
                        </button>
                        <button
                          type="button"
                          disabled={index === libraryRoots.length - 1}
                          onClick={() =>
                            setLibraryRoots((current) => moveItem(current, index, index + 1))
                          }
                          aria-label={`Move quick access folder ${index + 1} down`}
                          title="Move down"
                          className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-subtle transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
                        >
                          <ArrowDown size={14} />
                        </button>
                        <button
                          type="button"
                          onClick={() =>
                            setLibraryRoots((current) =>
                              current.filter((_, position) => position !== index)
                            )
                          }
                          aria-label={`Remove quick access folder ${index + 1}`}
                          className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-subtle transition hover:bg-button-hover hover:text-text"
                        >
                          <X size={14} />
                        </button>
                      </div>
                    ))}

                  </div>

                    <button
                      type="button"
                      onClick={() => setLibraryRoots((current) => [...current, { name: "", path: "" }])}
                      className="mt-2 h-9 self-start rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                    >
                      Add folder
                    </button>
                </div>
              </SettingsCard>

              <SettingsCard
                className="xl:h-full"
                title="Media Tools"
                description={isDesktop
                  ? "MKVO resolves installed MKVToolNix and FFmpeg commands for scan, remux, extraction, and property workflows."
                  : "The server image provides MKVToolNix and FFmpeg for scan, remux, extraction, and property workflows."}
              >
                {isDesktop ? (
                  <div className="mb-3 grid gap-2">
                    <ToolDirectoryField
                      label="MKVToolNix directory"
                      value={mkvToolNixDirectory}
                      placeholder="C:\\Program Files\\MKVToolNix"
                      onChange={setMkvToolNixDirectory}
                      onBrowse={() => setBrowsingRow("mkvtoolnix")}
                    />
                    <ToolDirectoryField
                      label="FFmpeg directory"
                      value={ffmpegDirectory}
                      placeholder="C:\\ffmpeg\\bin"
                      onChange={setFfmpegDirectory}
                      onBrowse={() => setBrowsingRow("ffmpeg")}
                    />
                    <p className="text-xs leading-5 text-subtle">
                      Choose the folders containing the tool executables. FFmpeg requires both <span className="font-mono text-muted">ffmpeg.exe</span> and <span className="font-mono text-muted">ffprobe.exe</span>. Leave a field blank to search MKVO's tools folder and the system PATH.
                    </p>
                  </div>
                ) : null}
                <div className="min-w-0 overflow-hidden rounded-lg border border-border bg-panel">
                  <table className="w-full table-fixed border-collapse text-left text-sm">
                    <colgroup>
                      <col className="w-[8.75rem]" />
                      <col className="w-[8.5rem]" />
                      <col />
                      <col className="w-[9rem]" />
                    </colgroup>
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
                          <td className="break-all border-b border-border px-3 py-2 font-mono text-xs leading-5 text-muted">{tool.resolvedPath}</td>
                          <td className="break-words border-b border-border px-3 py-2 text-muted">{tool.version || "-"}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "providers" ? (
            <div className="grid min-w-0 gap-3">
              <SettingsCard title="API Providers" description="TVDB and TMDB lookup requires your own API keys. Leave saved key fields blank to keep existing values.">
                <div className="grid grid-cols-3 gap-3">
                  <div className="rounded-lg border border-border bg-card p-3">
                    <h3 className="mb-3 text-sm font-semibold text-text">TVDB</h3>
                    <div className="grid grid-cols-2 gap-3">
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">TVDB API Key</span>
                      <input
                        value={tvdbApiKey}
                        onChange={(event) => setTvdbApiKey(event.target.value)}
                        placeholder={webSettings.data?.hasTvdbApiKey ? savedSecretPlaceholder : "User-provided TVDB API key"}
                        type="password"
                        className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                      />
                    </label>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">TVDB PIN</span>
                      <input
                        value={tvdbPin}
                        onChange={(event) => setTvdbPin(event.target.value)}
                        placeholder={webSettings.data?.hasTvdbPin ? savedSecretPlaceholder : "Optional TVDB subscriber PIN"}
                        type="password"
                        className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                      />
                    </label>
                    </div>
                    <ProviderTestButton provider="TVDB" testingProvider={testingProvider} onTest={testProvider} />
                  </div>
                  <div className="rounded-lg border border-border bg-card p-3">
                    <h3 className="mb-3 text-sm font-semibold text-text">TMDB</h3>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">TMDB API Key</span>
                      <input
                        value={tmdbApiKey}
                        onChange={(event) => setTmdbApiKey(event.target.value)}
                        placeholder={webSettings.data?.hasTmdbApiKey ? savedSecretPlaceholder : "User-provided TMDB API key"}
                        type="password"
                        className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                      />
                    </label>
                    <ProviderTestButton provider="TMDB" testingProvider={testingProvider} onTest={testProvider} />
                  </div>
                  <div className="rounded-lg border border-border bg-card p-3">
                    <h3 className="mb-3 text-sm font-semibold text-text">AniDB</h3>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">AniDB Client</span>
                      <input
                        value={anidbClient}
                        onChange={(event) => setAnidbClient(event.target.value)}
                        placeholder={webSettings.data?.hasAnidbClient ? "Saved - leave blank to keep" : "Registered client name, e.g. mkvo/1"}
                        className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                      />
                    </label>
                    <p className="mt-1.5 text-xs text-subtle">
                      AniDB identifies callers by a registered client name and version rather than an API key. Searching works without it; loading episodes requires it.
                    </p>
                  </div>
                  <div className="col-span-3 grid grid-cols-2 gap-3 rounded-lg border border-border bg-card p-3">
                  <div className="col-span-2">
                    <h3 className="text-sm font-semibold text-text">Provider Defaults</h3>
                    <p className="mt-1 text-xs text-subtle">Choose the language and provider used for new metadata searches.</p>
                  </div>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">Metadata Language</span>
                    <input
                      value={language}
                      onChange={(event) => setLanguage(event.target.value)}
                      className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    />
                  </label>
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">Default Rename Provider</span>
                    <select
                      value={provider}
                      onChange={(event) => setProvider(event.target.value)}
                      className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    >
                      <option value="TVDB">TVDB</option>
                      <option value="TMDB">TMDB</option>
                      <option value="AniDB">AniDB</option>
                      <option value="AniList">AniList</option>
                    </select>
                  </label>
                  </div>
                </div>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "rename" ? (
            <div className="grid min-w-0 gap-3">
              <SettingsCard title="Rename Templates" description="One template per line. The selected default template is always preserved when settings are saved." actions={
                  <button
                    type="button"
                    onClick={() => {
                      setTemplate(defaultRenameTemplate);
                      setRenameTemplatesText(defaultRenameTemplates.join("\n"));
                    }}
                    className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                  >
                    <RefreshCw size={14} />
                    Reset to Defaults
                  </button>
              }>
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Default Rename Template</span>
                  <input
                    value={template}
                    onChange={(event) => setTemplate(event.target.value)}
                    className="mt-1.5 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                  />
                </label>
                <p className="text-xs leading-5 text-muted">
                  Series templates can use {"{series}"}, {"{season:00}"}, {"{episode:00}"}, {"{episodeTitle}"}, and {"{absolute:000}"}. Movie templates can use {"{title}"} and {"{year}"}.
                </p>
                <textarea
                  value={renameTemplatesText}
                  onChange={(event) => setRenameTemplatesText(event.target.value)}
                  rows={6}
                  className="mt-3 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
                />
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "security" ? (
            isDesktop ? <p className="text-sm text-muted">Login and session settings apply to the container web server. The native desktop app uses your operating-system account.</p> : <SecuritySection />
          ) : null}

          {activeTab === "presets" ? (
            <div className="grid min-w-0 gap-3">
              <SettingsCard title="Track Presets" description="These lists feed Rename language choices and Track Properties name/language selectors." actions={
                  <button
                    type="button"
                    onClick={() => {
                      setAudioNamePresetsText(defaultAudioNamePresets.join("\n"));
                      setSubtitleNamePresetsText(defaultSubtitleNamePresets.join("\n"));
                      setLanguagePresetsText(defaultLanguagePresets.join("\n"));
                      setIgnoredSubfoldersText(defaultIgnoredSubfolders.join("\n"));
                      setMuxAudioDefaults("eng,jpn");
                      setMuxSubtitleDefaults("eng");
                    }}
                    className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
                  >
                    <RefreshCw size={14} />
                    Reset All Presets
                  </button>
              }>
                <div className="grid grid-cols-4 gap-3">
                  <PresetEditor label="Audio Name Presets" value={audioNamePresetsText} onChange={setAudioNamePresetsText} />
                  <PresetEditor label="Subtitle Name Presets" value={subtitleNamePresetsText} onChange={setSubtitleNamePresetsText} />
                  <PresetEditor label="Language Presets" value={languagePresetsText} onChange={setLanguagePresetsText} />
                  <PresetEditor label="Ignored Subfolders" value={ignoredSubfoldersText} onChange={setIgnoredSubfoldersText} />
                </div>
              </SettingsCard>

              <SettingsCard title="MKV Operations Defaults" description="Default keep-language values for track removal workflows.">
                <div className="grid grid-cols-2 gap-3">
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Default audio languages to keep</span>
                  <input
                    value={muxAudioDefaults}
                    onChange={(event) => setMuxAudioDefaults(event.target.value)}
                    placeholder="eng,jpn"
                    className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Default subtitle languages to keep</span>
                  <input
                    value={muxSubtitleDefaults}
                    onChange={(event) => setMuxSubtitleDefaults(event.target.value)}
                    placeholder="eng"
                    className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
                </div>
              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "library" ? (
            <div className="grid min-w-0 items-start gap-3 xl:h-full xl:grid-cols-[minmax(0,2fr)_minmax(18rem,0.8fr)] xl:items-stretch">
              <SettingsCard className="xl:col-start-2 xl:row-start-1 xl:h-full" title="Manual Watch Folders" description="Fallback paths available with or without a media server.">
                <label className="block">
                  <span className="text-xs font-semibold text-muted">Watch folders</span>
                  <textarea
                    value={watchFoldersText}
                    onChange={(event) => setWatchFoldersText(event.target.value)}
                    rows={5}
                    placeholder={"/media/anime\n/media/movies"}
                    className="mt-2 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
                  />
                </label>
                <label className="mt-3 flex items-center gap-2 text-sm text-muted">
                  <input
                    type="checkbox"
                    checked={liveWatcherEnabled}
                    onChange={(event) => setLiveWatcherEnabled(event.target.checked)}
                  />
                  Enable live watch-folder monitoring
                </label>
                <div className="mt-3 rounded-md border border-border bg-input px-3 py-2 text-xs leading-5 text-subtle">
                  Use container paths, usually under <span className="font-mono text-text">/media</span> or <span className="font-mono text-text">/downloads</span>.
                </div>
              </SettingsCard>

              <SettingsCard className="xl:col-start-1 xl:row-start-1 xl:h-full" title="Media Servers" description="Connect Emby, Jellyfin, or Plex. API keys and tokens are encrypted before they are stored.">
                <div className="max-h-64 space-y-2 overflow-auto pr-1">
                  {mediaServers.length === 0 ? (
                    <div className="rounded-md border border-border bg-input px-3 py-2 text-sm text-subtle">
                      No media servers configured. Manual watch folders remain the fallback.
                    </div>
                  ) : mediaServers.map((server) => (
                    <div key={server.id} className="rounded-lg border border-border bg-input p-3">
                      <div className="grid gap-2 lg:grid-cols-[minmax(9rem,1fr)_8rem_minmax(12rem,1.35fr)_minmax(10rem,1fr)_auto] lg:items-end">
                        <label className="block min-w-0">
                          <span className="text-[0.6875rem] font-semibold text-muted">Name</span>
                            <input
                              value={server.name}
                              onChange={(event) => updateMediaServer(server.id, { name: event.target.value })}
                            className="mt-1 h-9 w-full rounded-md border border-border bg-card px-3 text-sm font-semibold text-text outline-none focus:border-accent"
                            />
                        </label>
                        <label className="block min-w-0">
                          <span className="text-[0.6875rem] font-semibold text-muted">Type</span>
                            <select
                              value={server.type}
                              onChange={(event) => updateMediaServer(server.id, { type: event.target.value })}
                            className="mt-1 h-9 w-full rounded-md border border-border bg-card px-3 text-sm text-text outline-none focus:border-accent"
                            >
                              <option value="Emby">Emby</option>
                              <option value="Jellyfin">Jellyfin</option>
                              <option value="Plex">Plex</option>
                            </select>
                        </label>
                        <label className="block min-w-0">
                          <span className="text-[0.6875rem] font-semibold text-muted">Server URL</span>
                          <input
                            value={server.serverUrl}
                            onChange={(event) => updateMediaServer(server.id, { serverUrl: event.target.value })}
                            placeholder="http://localhost:8096"
                            className="mt-1 h-9 w-full rounded-md border border-border bg-card px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                          />
                        </label>
                        <label className="block min-w-0">
                          <span className="text-[0.6875rem] font-semibold text-muted">API key / token</span>
                          <input
                            value={server.apiKey ?? ""}
                            onChange={(event) => updateMediaServer(server.id, { apiKey: event.target.value })}
                            placeholder={server.hasApiKey ? savedSecretPlaceholder : "API key or token"}
                            type="password"
                            className="mt-1 h-9 w-full rounded-md border border-border bg-card px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                          />
                        </label>
                        <div className="flex gap-2">
                          <button
                            type="button"
                            onClick={() => testServer(server)}
                            title="Test connection"
                            aria-label={`Test ${server.name}`}
                            className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-button text-muted transition hover:bg-button-hover hover:text-text"
                          >
                            <CheckCircle2 size={14} />
                          </button>
                          <button
                            type="button"
                            onClick={() => syncServer(server)}
                            title="Sync libraries"
                            aria-label={`Sync ${server.name}`}
                            className="inline-flex h-9 w-9 items-center justify-center rounded-md bg-accent text-window transition hover:bg-accent-hover"
                          >
                            <RefreshCw size={14} />
                          </button>
                          <button
                            type="button"
                            onClick={() => removeMediaServer(server.id)}
                            title="Remove server"
                            aria-label={`Remove ${server.name}`}
                            className="inline-flex h-9 w-9 items-center justify-center rounded-md border border-border bg-button text-muted transition hover:bg-button-hover hover:text-text"
                          >
                            <Trash2 size={14} />
                          </button>
                        </div>
                      </div>
                      <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-2">
                          <label className="flex items-center gap-2 text-xs text-muted">
                            <input
                              type="checkbox"
                              checked={server.isDefault}
                              onChange={(event) => updateMediaServer(server.id, { isDefault: event.target.checked })}
                            />
                            Use as default server
                          </label>
                          {server.isDefault ? <span className="rounded bg-accent/20 px-2 py-1 text-[0.6875rem] font-semibold text-accent">default</span> : null}
                          {server.lastSyncedUtc ? <span className="text-[0.6875rem] text-subtle">Last synced: {formatDateTime(server.lastSyncedUtc)}</span> : null}
                      </div>
                      {server.libraries.length > 0 ? (
                        <div className="mt-2 max-h-24 overflow-auto rounded-md border border-border bg-card">
                          {server.libraries.map((library) => (
                            <label key={library.id} className="grid grid-cols-[1.5rem_minmax(7.5rem,11.25rem)_1fr] gap-2 border-b border-border px-3 py-2 text-xs last:border-b-0">
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
                        <div className="mt-2 rounded-md border border-border bg-card px-3 py-2 text-xs text-subtle">
                          No synced libraries yet. Save settings, then click Sync.
                        </div>
                      )}
                    </div>
                  ))}
                </div>

                <div className="mt-3 rounded-lg border border-border bg-panel p-3">
                  <h3 className="text-sm font-semibold">Add a server</h3>
                  <div className="mt-2 grid gap-2 md:grid-cols-2">
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">Name</span>
                      <input
                        value={newServerName}
                        onChange={(event) => setNewServerName(event.target.value)}
                        className="mt-1 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                      />
                    </label>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">Type</span>
                      <select
                        value={newServerType}
                        onChange={(event) => setNewServerType(event.target.value)}
                        className="mt-1 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
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
                        className="mt-1 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none placeholder:text-subtle focus:border-accent"
                      />
                    </label>
                    <label className="block">
                      <span className="text-xs font-semibold text-muted">API key / token</span>
                      <input
                        value={newServerApiKey}
                        onChange={(event) => setNewServerApiKey(event.target.value)}
                        type="password"
                        className="mt-1 h-9 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
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
                    <button
                      type="button"
                      onClick={() => void testNewServer()}
                      disabled={!newServerUrl.trim()}
                      className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      <CheckCircle2 size={16} />
                      Test connection
                    </button>
                  </div>
                </div>

              </SettingsCard>
            </div>
          ) : null}

          {activeTab === "appearance" ? (
            <div className="grid min-w-0 items-start gap-3 xl:h-full xl:grid-cols-2 xl:items-stretch">
              <SettingsCard
                title="Theme JSON"
                description="Edit or paste the complete theme definition."
                className="flex min-h-0 flex-col xl:h-full"
                contentClassName="flex min-h-0 flex-1 flex-col"
              >
                <label className="flex min-h-0 flex-1 flex-col">
                  <span className="text-xs font-semibold text-muted">Theme JSON</span>
                  <textarea
                    value={themeJson}
                    onChange={(event) => setThemeJson(event.target.value)}
                    rows={16}
                    className="mt-2 min-h-80 w-full flex-1 resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent xl:min-h-0"
                  />
                </label>
              </SettingsCard>

              <div className="grid min-w-0 gap-3 xl:h-full xl:grid-rows-[auto_1fr]">
              <SettingsCard title="Theme" description="Themes are shared by the desktop and browser interfaces.">
                <div className="flex items-end gap-3">
                  <label className="block flex-1">
                    <span className="text-xs font-semibold text-muted">Theme</span>
                    <select
                      value={themeName}
                      onChange={(event) => {
                        const nextName = event.target.value;
                        setThemeName(nextName);
                        const applied = applyWebTheme(nextName);
                        setThemeJson(JSON.stringify(applied, null, 2));
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
                <div className="mt-3 grid gap-3 sm:grid-cols-2">
                  <label className="block">
                    <span className="text-xs font-semibold text-muted">Color label</span>
                    <select
                      aria-label="Theme color label"
                      value={selectedThemeColor}
                      onChange={(event) => setSelectedThemeColor(event.target.value as ThemeColorName)}
                      className="mt-2 h-10 w-full rounded-md border border-border bg-input px-3 text-sm text-text outline-none focus:border-accent"
                    >
                      {themeColorOptions.map((option) => (
                        <option key={option.name} value={option.name}>{option.label}</option>
                      ))}
                    </select>
                  </label>
                  <ThemeColorField
                    label={themeColorOptions.find((option) => option.name === selectedThemeColor)?.label ?? selectedThemeColor}
                    value={themeColorValue(
                      themeJson,
                      selectedThemeColor,
                      getWebTheme(themeName).colors[selectedThemeColor] ?? "#000000"
                    )}
                    onChange={(color) => {
                      const option = themeColorOptions.find((candidate) => candidate.name === selectedThemeColor);
                      if (option) updateThemeColor(option.name, option.cssVariable, color);
                    }}
                  />
                </div>
                <p className="mt-2 text-xs text-subtle">Choose a label, then use the color picker to preview that theme color immediately. Save the edited theme below to keep it.</p>
              </SettingsCard>

              <SettingsCard className="xl:h-full" title="Custom Theme" description="Save the edited JSON as a named theme or remove the selected custom theme.">
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
                  disabled={webThemes.some((theme) => theme.name === themeName)}
                  className="mt-2 h-10 w-full rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
                >
                  Remove Custom Theme
                </button>
              </SettingsCard>
              </div>
            </div>
          ) : null}

          {activeTab === "about" ? (
            <div className="grid min-w-0 items-start gap-3 xl:h-full xl:grid-cols-2 xl:items-stretch">
              <div className="grid min-w-0 gap-3 xl:h-full xl:grid-rows-[auto_1fr]">
              <SettingsCard
                title={isDesktop ? "About MKV Orchestrator Desktop" : "About MKV Orchestrator Server"}
                description={isDesktop ? "The native Tauri desktop backed by the shared Rust runtime." : "The Rust server for browser, Docker, and NAS access."}
              >
                <div className="space-y-2 text-sm leading-5 text-muted">
                  <p>
                    This React interface uses the same Rust application services through {isDesktop ? "typed Tauri IPC" : "the typed HTTP API"}.
                  </p>
                  <p>
                    Current supported workflows include MKV and MP4 scanning, metadata inspection, provider-based rename previews, rename apply and undo batches, MKV operations, lossless MP4 to MKV conversion, MKV track property edits, library audit views, logs, and container tool checks.
                  </p>
                  <p>
                    TVDB and TMDB lookup requires your own API keys. Saved keys stay in the {isDesktop ? "local application configuration directory" : "mounted server configuration directory"}; never commit or publish that data.
                  </p>
                </div>
              </SettingsCard>

              <SettingsCard className="xl:h-full" title="Configuration Safety" description="Keep deployment-specific data out of source control.">
                <div className="space-y-2 text-sm leading-5 text-muted">
                  <p>
                    The repository should contain application code and default examples only. Personal paths, API keys, cache databases, and runtime logs belong in mounted volumes or environment variables.
                  </p>
                  <p>
                    Docker users should mount media into <span className="font-mono text-text">/media</span> and configuration into <span className="font-mono text-text">/config</span>.
                  </p>
                </div>
              </SettingsCard>
              </div>

              {/* Required, not decorative: TMDB's API terms oblige the notice
                  below. Kept in About and away from the workflow screens, per
                  docs/ATTRIBUTION_AND_LOGOS.md. */}
              <SettingsCard
                className="xl:h-full"
                title="Attribution"
                description="MKVO relies on these third-party services and tools."
              >
                <ul className="grid gap-3 sm:grid-cols-2">
                  {attributions.map((item) => (
                    <li
                      key={item.name}
                      className="flex min-w-0 items-center gap-3 rounded-lg border border-border bg-input p-3"
                    >
                      <img
                        src={item.logo}
                        alt={`${item.name} logo`}
                        className="h-10 w-20 shrink-0 object-contain"
                      />
                      <p className="min-w-0 text-xs leading-5 text-muted">{item.notice}</p>
                    </li>
                  ))}
                </ul>
              </SettingsCard>
            </div>
          ) : null}
        </div>
      </section>

      {browsingRow !== null ? (
        <FileBrowser
          initialPath={browsingRow === "home"
            ? defaultDirectory
            : browsingRow === "mkvtoolnix"
              ? mkvToolNixDirectory
              : browsingRow === "ffmpeg"
                ? ffmpegDirectory
                : libraryRoots[browsingRow]?.path || defaultDirectory || status.data?.mediaRoot || ""}
          homeRoot={{ name: defaultDirectoryName || "Home", path: defaultDirectory }}
          roots={libraryRoots}
          onCancel={() => setBrowsingRow(null)}
          removableRootPaths={libraryRoots.map((root) => root.path)}
          onUnpinFromQuickAccess={(path) => {
            setLibraryRoots((current) => current.filter((root) => !sameFolderPath(root.path, path)));
          }}
          onPinToQuickAccess={(path, name) => {
            setLibraryRoots((current) =>
              current.some((root) => sameFolderPath(root.path, path))
                ? current
                : [...current, { name: name || folderName(path), path }]
            );
          }}
          onSelect={(path, kind) => {
            // These settings all need a directory. Accepting an executable is
            // convenient and resolves it to the folder that contains it.
            const folder = kind === "file" ? parentFolder(path) : path;
            if (browsingRow === "home") {
              setDefaultDirectory(folder);
            } else if (browsingRow === "mkvtoolnix") {
              setMkvToolNixDirectory(folder);
            } else if (browsingRow === "ffmpeg") {
              setFfmpegDirectory(folder);
            } else {
              setLibraryRoots((current) =>
                current.map((item, position) =>
                  position === browsingRow
                    ? { ...item, path: folder, name: item.name || folderName(folder) }
                    : item
                )
              );
            }
            setBrowsingRow(null);
          }}
        />
      ) : null}
    </div>
  );
}

function ToolDirectoryField({
  label,
  value,
  placeholder,
  onChange,
  onBrowse
}: {
  label: string;
  value: string;
  placeholder: string;
  onChange: (value: string) => void;
  onBrowse: () => void;
}) {
  return (
    <label className="block">
      <span className="text-xs font-semibold text-muted">{label}</span>
      <div className="mt-2 flex min-w-0 items-center gap-2">
        <input
          aria-label={label}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          className="h-9 min-w-[11.25rem] flex-1 rounded-md border border-border bg-input px-2 font-mono text-xs text-text outline-none placeholder:text-subtle focus:border-accent"
        />
        <button
          type="button"
          aria-label={`Browse for ${label}`}
          onClick={onBrowse}
          className="h-9 shrink-0 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text"
        >
          Browse
        </button>
      </div>
    </label>
  );
}

function ThemeColorField({ label, value, onChange }: { label: string; value: string; onChange: (color: string) => void }) {
  return (
    <label className="block">
      <span className="text-xs font-semibold text-muted">{label}</span>
      <div className="mt-2 flex h-10 items-center gap-2 rounded-md border border-border bg-input px-2">
        <input
          type="color"
          aria-label={`${label} color`}
          value={value}
          onChange={(event) => onChange(event.target.value.toUpperCase())}
          className="h-7 w-9 cursor-pointer border-0 bg-transparent p-0"
        />
        <span className="font-mono text-xs text-text">{value.toUpperCase()}</span>
      </div>
    </label>
  );
}

function themeColorValue(themeJson: string, colorName: string, fallback: string) {
  try {
    const parsed = JSON.parse(themeJson);
    const value = parsed.colors?.[colorName] ?? parsed.Colors?.[colorName];
    return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

/** The folder holding a path, so picking a file still yields a directory. */
function parentFolder(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  return cut > 0 ? trimmed.slice(0, cut) : trimmed;
}

/** A sensible default name so a picked folder does not save as unnamed. */
function folderName(path: string): string {
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]+/);
  return parts[parts.length - 1] ?? "";
}

function sameFolderPath(left: string, right: string): boolean {
  return left.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase()
    === right.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

function moveItem<T>(items: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= items.length || to >= items.length) {
    return items;
  }
  const reordered = [...items];
  const [item] = reordered.splice(from, 1);
  reordered.splice(to, 0, item);
  return reordered;
}

function settingsRequestFromSaved(settings: WebSettings): WebSettingsRequest {
  return {
    tvdbLanguage: settings.tvdbLanguage,
    renameLookupProvider: settings.renameLookupProvider,
    renameTemplate: settings.renameTemplate,
    renameTemplates: settings.renameTemplates,
    audioNamePresets: settings.audioNamePresets,
    subtitleNamePresets: settings.subtitleNamePresets,
    languagePresets: settings.languagePresets,
    ignoredScanFolderNames: settings.ignoredScanFolderNames,
    mkvMergeDefaultAudioLanguages: settings.mkvMergeDefaultAudioLanguages,
    mkvMergeDefaultSubtitleLanguages: settings.mkvMergeDefaultSubtitleLanguages,
    defaultRoot: settings.defaultRoot,
    defaultRootName: settings.defaultRootName,
    libraryRoots: settings.libraryRoots,
    watchFolders: settings.watchFolders,
    enableLiveWatchFolderMonitoring: settings.enableLiveWatchFolderMonitoring,
    selectedThemeName: settings.selectedThemeName,
    customThemes: settings.customThemes,
    mediaServers: settings.mediaServers.map((server) => ({
      id: server.id,
      name: server.name,
      type: server.type,
      serverUrl: server.serverUrl,
      isDefault: server.isDefault,
      libraries: server.libraries
    })),
    mediaServerPathMappings: settings.mediaServerPathMappings
  };
}

function settingsFingerprint(request: WebSettingsRequest): string {
  return JSON.stringify(request);
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

function ProviderTestButton({
  provider,
  testingProvider,
  onTest
}: {
  provider: "TVDB" | "TMDB";
  testingProvider: string | null;
  onTest: (provider: "TVDB" | "TMDB") => void;
}) {
  const testing = testingProvider === provider;
  return (
    <button
      type="button"
      onClick={() => onTest(provider)}
      disabled={testingProvider !== null}
      className="mt-2 inline-flex h-8 items-center gap-2 rounded-md border border-border bg-button px-3 text-xs font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
    >
      <CheckCircle2 size={14} />
      {testing ? "Testing..." : `Test ${provider}`}
    </button>
  );
}

function SettingsCard({ title, description, children, actions, className = "", contentClassName = "" }: { title: string; description?: string; children: React.ReactNode; actions?: React.ReactNode; className?: string; contentClassName?: string }) {
  return (
    <section className={`min-w-0 rounded-lg border border-border bg-panel p-4 ${className}`}>
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-base font-semibold">{title}</h2>
        {actions ? <div className="shrink-0">{actions}</div> : null}
      </div>
      {description ? <p className="mt-2 text-sm leading-6 text-muted">{description}</p> : null}
      <div className={`mt-4 min-w-0 ${contentClassName}`}>{children}</div>
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
