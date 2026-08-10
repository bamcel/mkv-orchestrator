import { describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { DashboardPage } from "./DashboardPage";
import { MediaLibraryProvider } from "../state/MediaLibraryContext";
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

const emptyStatus = {
  name: "MKV Orchestrator",
  version: "0.1.0",
  mediaRoot: "/media",
  configRoot: "/config",
  sourceRoots: [],
  tools: []
};

const emptyScan = { updatedUtc: null, files: [], summary: { total: 0, mkv: 0, mp4: 0, failed: 0 } };

function renderDashboard(overrides: Parameters<typeof renderWithBackend>[1]) {
  return renderWithBackend(
    <MediaLibraryProvider>
      <DashboardPage />
    </MediaLibraryProvider>,
    overrides
  );
}

describe("Dashboard ignored folders", () => {
  /// The ignored list is sent with every scan request, so seeding it from a
  /// local constant silently overrode whatever the user configured in Settings.
  it("seeds the ignored list from saved settings rather than a local default", async () => {
    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () =>
        Promise.resolve(settings({ ignoredScanFolderNames: ["Featurettes", "Trailers"] }))
    });

    const field = await screen.findByLabelText(/ignored subfolders/i);
    await waitFor(() => expect(field).toHaveValue("Featurettes, Trailers"));
  });

  it("does not discard an edit in progress when settings arrive", async () => {
    const user = userEvent.setup();
    let resolveSettings: (value: WebSettings) => void = () => {};
    const pending = new Promise<WebSettings>((resolve) => {
      resolveSettings = resolve;
    });

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => pending
    });

    const field = await screen.findByLabelText(/ignored subfolders/i);
    await user.type(field, "MyOwnFolder");
    resolveSettings(settings({ ignoredScanFolderNames: ["Extras"] }));

    // The saved value must not clobber what the user is actively typing.
    await waitFor(() => expect(field).toHaveValue("MyOwnFolder"));
  });

  it("sends the configured folders with the scan request", async () => {
    const user = userEvent.setup();
    const startScan = vi.fn().mockResolvedValue({
      id: "job-1",
      status: "Queued",
      createdUtc: new Date().toISOString(),
      startedUtc: null,
      completedUtc: null,
      currentSource: "",
      completed: 0,
      total: 0,
      files: [],
      skipped: [],
      summary: { total: 0, mkv: 0, mp4: 0, failed: 0 },
      error: ""
    });

    renderDashboard({
      getStatus: () => Promise.resolve({ ...emptyStatus, sourceRoots: [{ name: "Media", path: "/media" }] }),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings({ ignoredScanFolderNames: ["Extras", "Samples"] })),
      browseFileSystem: () =>
        Promise.resolve({ path: "/media", parentPath: null, entries: [] }),
      startScan,
      getScanJob: () =>
        Promise.resolve({
          id: "job-1",
          status: "Completed" as const,
          createdUtc: new Date().toISOString(),
          startedUtc: null,
          completedUtc: null,
          currentSource: "",
          completed: 0,
          total: 0,
          files: [],
          skipped: [],
          summary: { total: 0, mkv: 0, mp4: 0, failed: 0 },
          error: ""
        })
    });

    await waitFor(() =>
      expect(screen.getByLabelText(/ignored subfolders/i)).toHaveValue("Extras, Samples")
    );

    await user.click(await screen.findByRole("button", { name: /browse/i }));
    await user.click(await screen.findByRole("button", { name: /add current folder/i }));
    await user.click(await screen.findByRole("button", { name: /^scan$/i }));

    await waitFor(() => expect(startScan).toHaveBeenCalled());
    expect(startScan.mock.calls[0][0].ignoredFolderNames).toEqual(["Extras", "Samples"]);
  });
});
