import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, CircleAlert } from "lucide-react";
import { getStatus, getWebSettings, saveWebSettings } from "../api";
import { SectionHeader } from "../components/SectionHeader";

export function SettingsPage() {
  const status = useQuery({ queryKey: ["status"], queryFn: getStatus });
  const webSettings = useQuery({ queryKey: ["web-settings"], queryFn: getWebSettings });
  const [tvdbApiKey, setTvdbApiKey] = useState("");
  const [tvdbPin, setTvdbPin] = useState("");
  const [tmdbApiKey, setTmdbApiKey] = useState("");
  const [language, setLanguage] = useState("eng");
  const [provider, setProvider] = useState("TVDB");
  const [template, setTemplate] = useState("{series} - S{season:00}E{episode:00} - {episodeTitle}");
  const [settingsStatus, setSettingsStatus] = useState("");

  useEffect(() => {
    if (!webSettings.data) return;
    setLanguage(webSettings.data.tvdbLanguage || "eng");
    setProvider(webSettings.data.renameLookupProvider || "TVDB");
    setTemplate(webSettings.data.renameTemplate || "{series} - S{season:00}E{episode:00} - {episodeTitle}");
  }, [webSettings.data]);

  async function saveSettings() {
    const saved = await saveWebSettings({
      tvdbApiKey: tvdbApiKey || undefined,
      tvdbPin: tvdbPin || undefined,
      tmdbApiKey: tmdbApiKey || undefined,
      tvdbLanguage: language,
      renameLookupProvider: provider,
      renameTemplate: template
    });

    setTvdbApiKey("");
    setTvdbPin("");
    setTmdbApiKey("");
    setSettingsStatus("Settings saved.");
    webSettings.refetch();
    setLanguage(saved.tvdbLanguage);
    setProvider(saved.renameLookupProvider);
    setTemplate(saved.renameTemplate);
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
    </>
  );
}
