import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, CircleAlert } from "lucide-react";
import { getStatus, getWebSettings, saveWebSettings } from "../api";
import { SectionHeader } from "../components/SectionHeader";
import { applyWebTheme, getAllWebThemes, getStoredWebThemeName, getWebTheme, removeCustomWebTheme, saveCustomWebTheme } from "../theme";

export function SettingsPage() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const webSettings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
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
  const [availableThemes, setAvailableThemes] = useState(() => getAllWebThemes());
  const [themeName, setThemeName] = useState(() => getStoredWebThemeName());
  const [themeJson, setThemeJson] = useState(() => JSON.stringify(getWebTheme(getStoredWebThemeName()), null, 2));
  const [customThemeName, setCustomThemeName] = useState("My Theme");
  const [settingsStatus, setSettingsStatus] = useState("");

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
  }, [webSettings.data]);

  async function saveSettings() {
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
      enableLiveWatchFolderMonitoring: liveWatcherEnabled
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
    <>
      <SectionHeader title="Settings" description="Review container paths and bundled media tool availability." />
      <section className="rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">Container</h2>
        <dl className="mt-4 grid grid-cols-[160px_1fr] gap-x-4 gap-y-3 text-sm">
          <dt className="text-muted">Media Root</dt>
          <dd className="font-mono text-text">{status.data?.mediaRoot ?? "/media"}</dd>
          <dt className="text-muted">Config Root</dt>
          <dd className="font-mono text-text">{status.data?.configRoot ?? "/config"}</dd>
        </dl>
      </section>

      <section className="mt-5 rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">Workflow Presets</h2>
        <p className="mt-2 text-sm leading-6 text-muted">
          These lists feed Rename language choices, Track Properties name/language selectors, and Mux / Remux default keep-language values.
        </p>
        <div className="mt-4 grid grid-cols-3 gap-4">
          <PresetEditor label="Audio Name Presets" value={audioNamePresetsText} onChange={setAudioNamePresetsText} />
          <PresetEditor label="Subtitle Name Presets" value={subtitleNamePresetsText} onChange={setSubtitleNamePresetsText} />
          <PresetEditor label="Language Presets" value={languagePresetsText} onChange={setLanguagePresetsText} />
        </div>
        <div className="mt-4 grid grid-cols-2 gap-4">
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
      </section>

      <section className="mt-5 rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">Library</h2>
        <p className="mt-2 text-sm leading-6 text-muted">
          Use container paths here. For Docker bind mounts, that usually means paths under <span className="font-mono text-text">/media</span>.
        </p>
        <label className="mt-4 block">
          <span className="text-xs font-semibold text-muted">Watch folders</span>
          <textarea
            value={watchFoldersText}
            onChange={(event) => setWatchFoldersText(event.target.value)}
            rows={5}
            placeholder={"/media/anime\n/media/movies"}
            className="mt-2 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
          />
        </label>
        <label className="mt-4 flex items-center gap-2 text-sm text-muted">
          <input
            type="checkbox"
            checked={liveWatcherEnabled}
            onChange={(event) => setLiveWatcherEnabled(event.target.checked)}
          />
          Enable live watch-folder monitoring
        </label>
        <div className="mt-2 text-xs leading-5 text-subtle">
          The web container currently uses these paths for cache builds and startup settings. Live watcher behavior depends on the mounted filesystem and container runtime.
        </div>
      </section>

      <section className="mt-5 rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">Theme</h2>
        <p className="mt-2 text-sm leading-6 text-muted">
          Themes apply to this browser session and are remembered locally for the web GUI.
        </p>
        <div className="mt-4 flex max-w-xl items-end gap-3">
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
        <div className="mt-4 grid grid-cols-[minmax(0,1fr)_240px] gap-4">
          <label className="block">
            <span className="text-xs font-semibold text-muted">Theme JSON</span>
            <textarea
              value={themeJson}
              onChange={(event) => setThemeJson(event.target.value)}
              rows={12}
              className="mt-2 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
            />
          </label>
          <div>
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
              disabled={["Modern", "Midnight", "Light"].includes(themeName)}
              className="mt-2 h-10 w-full rounded-md border border-border bg-button px-4 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text disabled:cursor-not-allowed disabled:text-disabled"
            >
              Remove Custom Theme
            </button>
          </div>
        </div>
      </section>

      <section className="mt-5 rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">API Providers</h2>
        <div className="mt-4 grid grid-cols-2 gap-4">
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
        <div className="mt-5 rounded-lg border border-border bg-panel p-4">
          <h3 className="text-sm font-semibold">Rename Templates</h3>
          <p className="mt-2 text-xs leading-5 text-muted">
            One template per line. Series templates can use {"{series}"}, {"{season:00}"}, {"{episode:00}"}, {"{episodeTitle}"}, and {"{absolute:000}"}. Movie templates can use {"{title}"} and {"{year}"}.
          </p>
          <textarea
            value={renameTemplatesText}
            onChange={(event) => setRenameTemplatesText(event.target.value)}
            rows={7}
            className="mt-3 w-full resize-none rounded-md border border-border bg-input p-3 font-mono text-xs leading-5 text-text outline-none placeholder:text-subtle focus:border-accent"
          />
        </div>
        <div className="mt-4 flex items-center gap-3">
          <button
            type="button"
            onClick={saveSettings}
            className="rounded-md bg-accent px-4 py-2 text-sm font-semibold text-window transition hover:bg-accent-hover"
          >
            Save
          </button>
          <span className="text-sm text-success">{settingsStatus}</span>
        </div>
      </section>

      <section className="mt-5 rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">Media Tools</h2>
        <p className="mt-2 text-sm leading-6 text-muted">
          The Docker image bundles MKVToolNix and FFmpeg. Paths shown here are resolved inside the container, not from the Windows host.
        </p>
        <div className="mt-4 overflow-hidden rounded-lg border border-border bg-panel">
          <table className="w-full border-collapse text-left text-sm">
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
      </section>

      <section className="mt-5 rounded-xl border border-border bg-card p-5 shadow-[0_20px_60px_rgba(0,0,0,0.18)]">
        <h2 className="text-base font-semibold">About MKV Orchestrator Web</h2>
        <div className="mt-3 space-y-3 text-sm leading-6 text-muted">
          <p>
            This web build is a single-container companion for server or NAS-style access. It shares the desktop app's core media logic where possible and exposes it through a React web interface.
          </p>
          <p>
            Current supported workflows include MKV and MP4 scanning, metadata inspection, provider-based rename previews, rename apply and undo batches, MKV mux/remux operations, MKV track property edits, library audit views, logs, and container tool checks.
          </p>
          <p>
            TVDB and TMDB lookup requires your own API keys. Saved keys are written to the container config volume under <span className="font-mono text-text">/config</span>, so do not commit or publish that volume.
          </p>
        </div>
      </section>
    </>
  );
}

function PresetEditor({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <label className="block">
      <span className="text-xs font-semibold text-muted">{label}</span>
      <textarea
        value={value}
        onChange={(event) => onChange(event.target.value)}
        rows={7}
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
