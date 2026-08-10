import { describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SettingsPage } from "./SettingsPage";
import { renderWithBackend } from "../test/render";
import type { WebSettings } from "../generated/contracts";

function settings(overrides: Partial<WebSettings> = {}): WebSettings {
  return {
    hasTvdbApiKey: false,
    hasTvdbPin: false,
    hasTmdbApiKey: false,
    hasAnidbClient: false,
    tvdbLanguage: "eng",
    renameLookupProvider: "TVDB",
    renameTemplate: "{series}",
    renameTemplates: [],
    audioNamePresets: [],
    subtitleNamePresets: [],
    languagePresets: [],
    mkvMergeDefaultAudioLanguages: "eng",
    mkvMergeDefaultSubtitleLanguages: "eng",
    mkvToolNixDirectory: null,
    ffmpegDirectory: null,
    defaultRoot: null,
    ignoredScanFolderNames: [],
    useQuickHashOnUnreliableTimestamps: false,
    renamePreviewCompactView: false,
    maxScanWorkers: 4,
    maxEditWorkers: 2,
    maxRemuxWorkers: 1,
    watchFolders: [],
    enableLiveWatchFolderMonitoring: false,
    watchDebounceMillis: 750,
    watchReconciliationIntervalMinutes: 30,
    watchForcePolling: false,
    selectedThemeName: "Dark",
    customThemes: [],
    mediaServers: [],
    mediaServerPathMappings: [],
    ...overrides
  };
}

const status = {
  name: "MKV Orchestrator",
  version: "0.1.0",
  mediaRoot: "/media",
  configRoot: "/config",
  sourceRoots: [],
  tools: []
};

/// Provider settings live behind the Rename tab, so every test opens it first.
async function openRenameTab(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: /^rename$/i }));
}

describe("Settings providers", () => {
  /// The backend supports four providers. Anything missing from this list is
  /// unreachable however well the Rust side works.
  it("offers every provider the backend implements", async () => {
    const user = userEvent.setup();
    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings())
    });
    await openRenameTab(user);

    for (const provider of ["TVDB", "TMDB", "AniDB", "AniList"]) {
      expect(await screen.findByRole("option", { name: provider })).toBeInTheDocument();
    }
  });

  /// AniDB uses a registered client name, not an API key, so it needs its own
  /// field; without one the provider can search but never load episodes.
  it("exposes the AniDB client field and saves it", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings({ hasAnidbClient: true }));

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings()),
      saveWebSettings
    });

    await openRenameTab(user);
    const field = await screen.findByLabelText(/anidb client/i);
    await user.type(field, "mkvo/1");
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].anidbClient).toBe("mkvo/1");
  });

  /// A saved secret is never returned, only a `has*` flag, so the field must
  /// signal that leaving it blank keeps the stored value.
  it("does not resend a stored secret that was left untouched", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings({ hasTvdbApiKey: true }));

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings({ hasTvdbApiKey: true })),
      saveWebSettings
    });

    await openRenameTab(user);
    const field = await screen.findByLabelText(/tvdb api key/i);
    expect(field).toHaveAttribute("placeholder", expect.stringMatching(/saved/i));

    await user.click(screen.getByRole("button", { name: /save settings/i }));
    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].tvdbApiKey).toBeUndefined();
  });
});
