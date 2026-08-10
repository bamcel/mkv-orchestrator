import { beforeEach, describe, expect, it, vi } from "vitest";
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

const emptyStatus = {
  name: "MKV Orchestrator",
  version: "0.1.0",
  mediaRoot: "/media",
  configRoot: "/config",
  sourceRoots: [],
  tools: [],
  contractVersion: 1
};

const emptyScan = { updatedUtc: null, files: [], summary: { total: 0, mkv: 0, mp4: 0, failed: 0, cached: 0 }, selectedPaths: [] };

// The library context persists the working set and the template choice, so
// without this one test's files leak into the next -- and the dashboard only
// adopts a fresh scan when it is holding none, so the leak wins.
beforeEach(() => {
  window.localStorage.clear();
  window.sessionStorage.clear();
});

function renderDashboard(overrides: Parameters<typeof renderWithBackend>[1]) {
  return renderWithBackend(
    <MediaLibraryProvider>
      <DashboardPage />
    </MediaLibraryProvider>,
    overrides
  );
}

describe("Dashboard empty state", () => {
  /// The desktop browses the whole machine. Telling it to mount a volume is
  /// instructions for a container the user is not running.
  it("tells a desktop user to browse rather than to mount a volume", async () => {
    renderDashboard({
      transport: "tauri",
      getStatus: () => Promise.resolve({ ...emptyStatus, mediaRoot: "C:\\media" }),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings())
    });

    expect(await screen.findByText(/browse for a folder or file/i)).toBeInTheDocument();
    expect(screen.queryByText(/mount media to/i)).not.toBeInTheDocument();
  });

  /// The served build really is reached through a bind mount, so the original
  /// guidance is still the right guidance there.
  it("keeps the mount guidance when served over HTTP", async () => {
    renderDashboard({
      transport: "http",
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings())
    });

    expect(await screen.findByText(/mount media to \/media/i)).toBeInTheDocument();
  });
});

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
      summary: { total: 0, mkv: 0, mp4: 0, failed: 0, cached: 0 },
      error: ""
    });

    renderDashboard({
      getStatus: () => Promise.resolve({ ...emptyStatus, sourceRoots: [{ name: "Media", path: "/media" }] }),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings({ ignoredScanFolderNames: ["Extras", "Samples"] })),
      browseFileSystem: () =>
        Promise.resolve({ path: "/media", parentPath: null, entries: [] }),
      // Browsing can range outside the authorized roots, so the chosen folder
      // is granted before it is used as a scan source.
      authorizeBrowsedRoot: () => Promise.resolve(),
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
          summary: { total: 0, mkv: 0, mp4: 0, failed: 0, cached: 0 },
          error: ""
        })
    });

    await waitFor(() =>
      expect(screen.getByLabelText(/ignored subfolders/i)).toHaveValue("Extras, Samples")
    );

    await user.click(await screen.findByRole("button", { name: /browse/i }));
    await user.click(await screen.findByRole("button", { name: /select this folder/i }));
    await user.click(await screen.findByRole("button", { name: /^scan$/i }));

    await waitFor(() => expect(startScan).toHaveBeenCalled());
    expect(startScan.mock.calls[0][0].ignoredFolderNames).toEqual(["Extras", "Samples"]);
  });
});

describe("Dashboard detail panels", () => {
  /// They used to appear only once a file was selected, so the dashboard
  /// reflowed as scans came and went and an empty page gave no hint of what
  /// these panels are for.
  it("keeps Media Info and Track Info in place with nothing scanned", async () => {
    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings())
    });

    expect(await screen.findByText("Media Info")).toBeInTheDocument();
    expect(screen.getByText("Track Info")).toBeInTheDocument();

    // The labels stay so the panel says what it will show.
    for (const label of ["File", "Codec", "Resolution", "Bit Depth", "HDR", "Status"]) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0);
    }
    expect(screen.getByText(/scan a folder, then select a file/i)).toBeInTheDocument();
  });

  it("fills the panels once a file is scanned", async () => {
    const file = {
      path: "/media/Show/Ep01.mkv",
      fileName: "Ep01.mkv",
      extension: ".mkv",
      status: "Scanned",
      reader: "mkvmerge",
      codec: "HEVC/H.265",
      resolution: "1920x1080",
      bitDepth: "",
      hdr: "",
      videoSummary: "HEVC/H.265 | 1920x1080",
      audioSummary: "eng x1",
      subtitleSummary: "eng x1",
      attachmentSummary: "None",
      tracks: [
        {
          id: 0,
          trackNumber: 1,
          type: "video",
          codec: "HEVC/H.265",
          language: "und",
          name: "",
          default: true,
          forced: false
        }
      ],
      attachments: []
    };

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () =>
        Promise.resolve({ ...emptyScan, files: [file], summary: { total: 1, mkv: 1, mp4: 0, failed: 0, cached: 0 } }),
      getWebSettings: () => Promise.resolve(settings())
    });

    await waitFor(() => expect(screen.getAllByText("HEVC/H.265").length).toBeGreaterThan(0));
    expect(screen.queryByText(/scan a folder, then select a file/i)).not.toBeInTheDocument();
  });
});

describe("Dashboard template highlighting", () => {
  const templateFile = {
    path: "/media/Show/Ep01.mkv",
    fileName: "Ep01.mkv",
    extension: ".mkv",
    status: "Scanned",
    reader: "mkvmerge",
    codec: "HEVC/H.265",
    resolution: "1920x1080",
    bitDepth: "",
    hdr: "",
    videoSummary: "HEVC/H.265 | 1920x1080",
    audioSummary: "eng x1",
    subtitleSummary: "eng x1",
    attachmentSummary: "None",
    tracks: [
      { id: 0, trackNumber: 1, type: "video", codec: "HEVC/H.265", language: "und", name: "Ep01.Release", default: true, forced: false },
      { id: 1, trackNumber: 2, type: "audio", codec: "AC-3", language: "eng", name: "", default: true, forced: false }
    ],
    attachments: []
  };

  function renderWithTemplate() {
    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () =>
        Promise.resolve({
          ...emptyScan,
          files: [templateFile],
          summary: { total: 1, mkv: 1, mp4: 0, failed: 0, cached: 0 }
        }),
      getWebSettings: () => Promise.resolve(settings())
    });
  }

  /// The other Media Info fields already marked the template in accent; the
  /// filename was left plain, so the one row naming the file did not match.
  it("colours the template's filename like the rest of its details", async () => {
    renderWithTemplate();

    const rows = await screen.findAllByText("Ep01.mkv");
    const detail = rows.find((element) => element.tagName === "DD");
    expect(detail).toBeDefined();
    expect(detail).toHaveClass("text-accent");
  });

  /// Track rows are compared against the template, so the template itself has
  /// nothing to compare against and reads as the reference.
  it("colours the template's track rows as the reference", async () => {
    renderWithTemplate();

    const codec = (await screen.findAllByText("AC-3"))[0];
    expect(codec).toHaveClass("text-accent");

    // A video track's name is exempt from comparison, but not from being
    // marked as the template.
    expect(screen.getByText("Ep01.Release")).toHaveClass("text-accent");
  });
});
