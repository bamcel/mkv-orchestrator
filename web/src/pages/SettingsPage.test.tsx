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
    libraryRoots: [],
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

describe("Settings library folders", () => {
  /// The container case the setting exists for: one bind mount, several shares
  /// inside it, each wanted as its own entry in the browser.
  it("saves several folders from inside a single mount", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(
      settings({
        libraryRoots: [
          { name: "Anime", path: "/mnt/user/anime" },
          { name: "TV", path: "/mnt/user/tv" }
        ]
      })
    );

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve({ ...status, mediaRoot: "/mnt/user" }),
      getWebSettings: () => Promise.resolve(settings()),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^library$/i }));
    await user.click(await screen.findByRole("button", { name: /add folder/i }));
    await user.type(screen.getByLabelText(/library folder 1 name/i), "Anime");
    await user.type(screen.getByLabelText(/library folder 1 path/i), "/mnt/user/anime");
    await user.click(screen.getByRole("button", { name: /add folder/i }));
    await user.type(screen.getByLabelText(/library folder 2 name/i), "TV");
    await user.type(screen.getByLabelText(/library folder 2 path/i), "/mnt/user/tv");

    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].libraryRoots).toEqual([
      { name: "Anime", path: "/mnt/user/anime" },
      { name: "TV", path: "/mnt/user/tv" }
    ]);
  });

  it("loads folders already configured and can remove one", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings());

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () =>
        Promise.resolve(
          settings({
            libraryRoots: [
              { name: "Anime", path: "/mnt/user/anime" },
              { name: "TV", path: "/mnt/user/tv" }
            ]
          })
        ),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^library$/i }));
    await waitFor(() =>
      expect(screen.getByLabelText(/library folder 1 name/i)).toHaveValue("Anime")
    );

    await user.click(screen.getByRole("button", { name: /remove library folder 1/i }));
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].libraryRoots).toEqual([
      { name: "TV", path: "/mnt/user/tv" }
    ]);
  });

  /// A half-finished row must not be saved as an unnamed root.
  it("drops a row that was never filled in", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings());

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings()),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^library$/i }));
    await user.click(await screen.findByRole("button", { name: /add folder/i }));
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].libraryRoots).toEqual([]);
  });

  /// Typing a path by hand is fine, but browsing is the point on the desktop.
  it("fills a row from the browser and names it after the folder", async () => {
    const user = userEvent.setup();

    renderWithBackend(<SettingsPage />, {
      transport: "tauri",
      getStatus: () => Promise.resolve({ ...status, mediaRoot: "" }),
      getWebSettings: () => Promise.resolve(settings()),
      browseFileSystem: (path?: string) =>
        Promise.resolve(
          path
            ? { path, parentPath: "", entries: [] }
            : { path: "", parentPath: null, entries: [{ name: "D:", path: "D:\\", kind: "folder" as const, sizeBytes: null, modifiedUtc: "1970-01-01T00:00:00Z" }] }
        )
    });

    await user.click(await screen.findByRole("button", { name: /^library$/i }));
    await user.click(await screen.findByRole("button", { name: /add folder/i }));
    await user.click(screen.getByRole("button", { name: /^browse$/i }));

    // "D:" is both a sidebar shortcut and a row; the row is the one to open.
    const rows = await screen.findAllByText("D:");
    const cell = rows.find((element) => element.closest("tr"));
    await user.dblClick(cell!);
    await user.click(await screen.findByRole("button", { name: /select this folder/i }));

    await waitFor(() =>
      expect(screen.getByLabelText(/library folder 1 path/i)).toHaveValue("D:\\")
    );
  });
});

describe("Settings attribution", () => {
  /// TMDB's API terms require this notice. The Avalonia About tab carried it;
  /// when that UI was deleted this became the only place it can appear.
  it("shows the notice TMDB's terms require", async () => {
    const user = userEvent.setup();
    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings())
    });

    await user.click(await screen.findByRole("button", { name: /^about$/i }));

    expect(
      await screen.findByText(
        /this product uses the TMDB API but is not endorsed or certified by TMDB\./i
      )
    ).toBeInTheDocument();
  });

  it("credits every provider and tool it relies on", async () => {
    const user = userEvent.setup();
    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings())
    });

    await user.click(await screen.findByRole("button", { name: /^about$/i }));

    for (const name of ["TMDB", "TheTVDB", "MKVToolNix", "FFmpeg"]) {
      expect(await screen.findByAltText(`${name} logo`)).toBeInTheDocument();
    }
  });
});
