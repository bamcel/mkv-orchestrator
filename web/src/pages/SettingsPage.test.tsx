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
    defaultRootName: "Home",
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
  tools: [],
  contractVersion: 1
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

  /// A saved secret is never returned, only a `has*` flag, so the field shows
  /// masking dots while keeping its real value empty and preserving the key.
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
    expect(field).toHaveAttribute("placeholder", "••••••••••••");
    expect(field).toHaveValue("");

    await user.click(screen.getByRole("button", { name: /save settings/i }));
    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].tvdbApiKey).toBeUndefined();
  });
});

describe("Settings library folders", () => {
  it("uses the mounted server media path as an editable Home directory", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(
      settings({ defaultRoot: "/media/tv" })
    );

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve({ ...status, mediaRoot: "/media" }),
      getWebSettings: () => Promise.resolve(settings()),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    expect(await screen.findByRole("heading", { name: "Default Directory" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /server storage/i })).not.toBeInTheDocument();
    expect(await screen.findByRole("textbox", { name: /default directory name/i })).toHaveValue("Home");
    const directory = await screen.findByRole("textbox", { name: /^default directory$/i });
    expect(directory).toHaveValue("/media");
    expect(screen.queryByText(/paths are resolved inside/i)).not.toBeInTheDocument();
    expect(screen.queryByText("Config Root")).not.toBeInTheDocument();

    await user.clear(directory);
    await user.type(directory, "/media/tv");

    await waitFor(() => {
      expect(saveWebSettings.mock.calls.at(-1)?.[0].defaultRoot).toBe("/media/tv");
      expect(saveWebSettings.mock.calls.at(-1)?.[0].defaultRootName).toBe("Home");
    }, { timeout: 2500 });
  });

  it("automatically saves a changed desktop default directory", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(
      settings({ defaultRoot: "D:\\Media" })
    );

    renderWithBackend(<SettingsPage />, {
      transport: "tauri",
      getStatus: () => Promise.resolve({ ...status, mediaRoot: "" }),
      getWebSettings: () => Promise.resolve(settings()),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    const homeName = await screen.findByRole("textbox", { name: /default directory name/i });
    await user.clear(homeName);
    await user.type(homeName, "Downloads");
    await user.type(await screen.findByRole("textbox", { name: /^default directory$/i }), "D:\\Media");

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled(), { timeout: 2500 });
    expect(saveWebSettings.mock.calls.at(-1)?.[0].defaultRoot).toBe("D:\\Media");
    expect(saveWebSettings.mock.calls.at(-1)?.[0].defaultRootName).toBe("Downloads");
    expect(saveWebSettings.mock.calls.at(-1)?.[0].libraryRoots).toEqual([]);
  });

  it("saves the desktop default directory separately from Quick Access", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(
      settings({ defaultRoot: "D:\\Media" })
    );

    renderWithBackend(<SettingsPage />, {
      transport: "tauri",
      getStatus: () => Promise.resolve({ ...status, mediaRoot: "" }),
      getWebSettings: () => Promise.resolve(settings()),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    const directory = await screen.findByRole("textbox", { name: /^default directory$/i });
    await user.type(directory, "D:\\Media");
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].defaultRoot).toBe("D:\\Media");
    expect(saveWebSettings.mock.calls[0][0].libraryRoots).toEqual([]);
  });

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

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    await user.click(await screen.findByRole("button", { name: /add folder/i }));
    await user.type(screen.getByLabelText(/quick access folder 1 name/i), "Anime");
    await user.type(screen.getByLabelText(/quick access folder 1 path/i), "/mnt/user/anime");
    await user.click(screen.getByRole("button", { name: /add folder/i }));
    await user.type(screen.getByLabelText(/quick access folder 2 name/i), "TV");
    await user.type(screen.getByLabelText(/quick access folder 2 path/i), "/mnt/user/tv");

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
            defaultRoot: "/mnt/user",
            libraryRoots: [
              { name: "Anime", path: "/mnt/user/anime" },
              { name: "TV", path: "/mnt/user/tv" }
            ]
          })
        ),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    await waitFor(() =>
      expect(screen.getByLabelText(/quick access folder 1 name/i)).toHaveValue("Anime")
    );

    await user.click(screen.getByRole("button", { name: /remove quick access folder 1/i }));
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
    expect(saveWebSettings.mock.calls[0][0].libraryRoots).toEqual([
      { name: "TV", path: "/mnt/user/tv" }
    ]);
  });

  it("reorders Quick Access folders and automatically saves their order", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings());

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings({
        defaultRoot: "/media",
        libraryRoots: [
          { name: "Anime", path: "/media/anime" },
          { name: "TV", path: "/media/tv" },
          { name: "Movies", path: "/media/movies" }
        ]
      })),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    await user.click(await screen.findByRole("button", { name: /move quick access folder 2 up/i }));

    expect(screen.getByLabelText(/quick access folder 1 name/i)).toHaveValue("TV");
    expect(screen.getByLabelText(/quick access folder 2 name/i)).toHaveValue("Anime");
    await waitFor(() => {
      expect(saveWebSettings.mock.calls.at(-1)?.[0].libraryRoots).toEqual([
        { name: "TV", path: "/media/tv" },
        { name: "Anime", path: "/media/anime" },
        { name: "Movies", path: "/media/movies" }
      ]);
    }, { timeout: 2500 });
  });

  it("promotes the first legacy shortcut to Home", async () => {
    const saveWebSettings = vi.fn().mockResolvedValue(
      settings({
        defaultRoot: "/mnt/user/anime",
        libraryRoots: [{ name: "TV", path: "/mnt/user/tv" }]
      })
    );

    renderWithBackend(<SettingsPage />, {
      transport: "tauri",
      getStatus: () => Promise.resolve({ ...status, mediaRoot: "/mnt/user/anime" }),
      getWebSettings: () => Promise.resolve(settings({
        libraryRoots: [
          { name: "Anime", path: "/mnt/user/anime" },
          { name: "TV", path: "/mnt/user/tv" }
        ]
      })),
      saveWebSettings
    });

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled(), { timeout: 2500 });
    const request = saveWebSettings.mock.calls.at(-1)?.[0];
    expect(request.defaultRoot).toBe("/mnt/user/anime");
    expect(request.libraryRoots).toEqual([{ name: "TV", path: "/mnt/user/tv" }]);
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

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
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

    await user.click(await screen.findByRole("button", { name: /^general$/i }));
    await user.click(await screen.findByRole("button", { name: /add folder/i }));
    await user.click(screen.getByRole("button", { name: /browse for quick access folder 1/i }));

    // "D:" is both a sidebar shortcut and a row; the row is the one to open.
    const rows = await screen.findAllByText("D:");
    const cell = rows.find((element) => element.closest("tr"));
    await user.dblClick(cell!);
    await user.click(await screen.findByRole("button", { name: /select this folder/i }));

    await waitFor(() =>
      expect(screen.getByLabelText(/quick access folder 1 path/i)).toHaveValue("D:\\")
    );
  });
});

describe("Settings defaults", () => {
  it("resets rename templates and automatically saves them", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings({
      renameTemplate: "{series} - S{season:00}E{episode:00} - {episodeTitle}",
      renameTemplates: ["{title}", "{title} ({year})", "{series} - S{season:00}E{episode:00} - {episodeTitle}", "S{season:00}E{episode:00} - {episodeTitle}", "{series} - {absolute:000} - {episodeTitle}"]
    }));

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings({ renameTemplate: "custom", renameTemplates: ["custom"] })),
      saveWebSettings
    });

    await openRenameTab(user);
    await user.click(await screen.findByRole("button", { name: /reset to defaults/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled(), { timeout: 2500 });
    expect(saveWebSettings.mock.calls.at(-1)?.[0].renameTemplates).toHaveLength(5);
    expect(saveWebSettings.mock.calls.at(-1)?.[0].renameTemplate).toContain("{episodeTitle}");
  });

  it("resets every preset list and mux default", async () => {
    const user = userEvent.setup();
    const saveWebSettings = vi.fn().mockResolvedValue(settings());

    renderWithBackend(<SettingsPage />, {
      getStatus: () => Promise.resolve(status),
      getWebSettings: () => Promise.resolve(settings({
        audioNamePresets: ["Custom"],
        subtitleNamePresets: ["Custom"],
        languagePresets: ["zzz"],
        mkvMergeDefaultAudioLanguages: "zzz",
        mkvMergeDefaultSubtitleLanguages: "zzz"
      })),
      saveWebSettings
    });

    await user.click(await screen.findByRole("button", { name: /^presets$/i }));
    await user.click(await screen.findByRole("button", { name: /reset all presets/i }));

    await waitFor(() => expect(saveWebSettings).toHaveBeenCalled(), { timeout: 2500 });
    const request = saveWebSettings.mock.calls.at(-1)?.[0];
    expect(request.audioNamePresets).toEqual(["English", "Japanese", "Commentary"]);
    expect(request.subtitleNamePresets).toEqual([
      "English",
      "English Forced",
      "English SDH",
      "Dialogue",
      "Signs & Songs",
      "Commentary"
    ]);
    expect(request.languagePresets).toContain("eng");
    expect(request.mkvMergeDefaultAudioLanguages).toBe("eng,jpn");
    expect(request.mkvMergeDefaultSubtitleLanguages).toBe("eng");
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
