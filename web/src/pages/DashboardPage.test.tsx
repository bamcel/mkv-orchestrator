import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor } from "@testing-library/react";
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

describe("Dashboard scan source labels", () => {
  it("shows only the selected folder name while retaining the full path as its tooltip", async () => {
    window.sessionStorage.setItem("mkvo.web.scanSources", JSON.stringify(["/media/downloads/complete/Superstore"]));

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings())
    });

    const label = await screen.findByText("Superstore");
    expect(label).toHaveAttribute("title", "/media/downloads/complete/Superstore");
    expect(screen.queryByText("/media/downloads/complete/Superstore")).not.toBeInTheDocument();
  });

  it("shows only file names when several files are selected", async () => {
    window.sessionStorage.setItem("mkvo.web.scanSources", JSON.stringify([
      String.raw`C:\media\Show\Episode 01.mkv`,
      String.raw`C:\media\Show\Episode 02.mkv`
    ]));

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings())
    });

    expect(await screen.findByText("Episode 01.mkv")).toBeInTheDocument();
    expect(screen.getByText("Episode 02.mkv")).toBeInTheDocument();
    expect(screen.queryByText(String.raw`C:\media\Show\Episode 01.mkv`)).not.toBeInTheDocument();
  });

  it("removes file-info rows immediately when their source is cleared", async () => {
    const user = userEvent.setup();
    window.sessionStorage.setItem("mkvo.web.scanSources", JSON.stringify([
      "/media/ShowA",
      "/media/ShowB"
    ]));
    const showA = {
      path: "/media/ShowA/Episode 01.mkv",
      fileName: "Episode 01.mkv",
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
      tracks: [],
      attachments: []
    };
    const showB = { ...showA, path: "/media/ShowB/Episode 02.mkv", fileName: "Episode 02.mkv" };

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve({ ...emptyScan, files: [showA, showB] }),
      getWebSettings: () => Promise.resolve(settings())
    });

    expect(await screen.findByRole("row", { name: /Episode 01\.mkv/i })).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Episode 02\.mkv/i })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /clear selected sources/i }));
    await waitFor(() => {
      expect(screen.queryByRole("row", { name: /Episode 01\.mkv/i })).not.toBeInTheDocument();
      expect(screen.queryByRole("row", { name: /Episode 02\.mkv/i })).not.toBeInTheDocument();
    });
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
    expect(startScan.mock.calls[0][0].forceRefresh).toBe(false);
  });

  it("can force a rescan of sources that were already scanned", async () => {
    const user = userEvent.setup();
    const startScan = vi.fn().mockResolvedValue({
      id: "refresh-job",
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
      getStatus: () => Promise.resolve({
        ...emptyStatus,
        sourceRoots: [{ name: "Media", path: "/media" }]
      }),
      getCurrentScanFiles: () => Promise.resolve(emptyScan),
      getWebSettings: () => Promise.resolve(settings()),
      browseFileSystem: () =>
        Promise.resolve({ path: "/media", parentPath: null, entries: [] }),
      authorizeBrowsedRoot: () => Promise.resolve(),
      startScan
    });

    await user.click(await screen.findByRole("button", { name: /browse/i }));
    await user.click(await screen.findByRole("button", { name: /select this folder/i }));
    await user.click(await screen.findByRole("button", { name: /rescan files/i }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(1));
    expect(startScan.mock.calls[0][0]).toMatchObject({
      sources: ["/media"],
      forceRefresh: true
    });
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

  it("supports range selection, Ctrl+A, and Delete for scanned files", async () => {
    const user = userEvent.setup();
    const baseFile = {
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
      tracks: [],
      attachments: []
    };
    const secondFile = { ...baseFile, path: "/media/Show/Ep02.mkv", fileName: "Ep02.mkv" };
    const thirdFile = { ...baseFile, path: "/media/Show/Ep03.mkv", fileName: "Ep03.mkv" };

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve({
        ...emptyScan,
        files: [baseFile, secondFile, thirdFile],
        summary: { total: 3, mkv: 3, mp4: 0, failed: 0, cached: 0 }
      }),
      getWebSettings: () => Promise.resolve(settings()),
      setFileSelection: (paths: string[]) => Promise.resolve({ ...emptyScan, selectedPaths: paths })
    });

    const firstRow = (await screen.findAllByText("Ep01.mkv"))[0].closest("tr")!;
    const secondRow = (await screen.findAllByText("Ep02.mkv"))[0].closest("tr")!;
    const thirdRow = (await screen.findAllByText("Ep03.mkv"))[0].closest("tr")!;
    await user.click(firstRow);
    fireEvent.click(thirdRow, { shiftKey: true });
    expect(firstRow).toHaveClass("bg-selected");
    expect(secondRow).toHaveClass("bg-selected");
    expect(thirdRow).toHaveClass("bg-selected");

    fireEvent.click(secondRow, { ctrlKey: true });
    expect(secondRow).not.toHaveClass("bg-selected");

    const scannedFiles = screen.getByLabelText("Scanned files");
    fireEvent.keyDown(scannedFiles, { key: "a", ctrlKey: true });
    expect(secondRow).toHaveClass("bg-selected");
    fireEvent.keyDown(scannedFiles, { key: "Delete" });
    expect(await screen.findByText("No files scanned yet")).toBeInTheDocument();
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

  it("keeps template selection in the file context menu instead of the File Info header", async () => {
    renderWithTemplate();

    expect(await screen.findByText("Template: Ep01.mkv")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /use selected as template/i })).not.toBeInTheDocument();
  });

  it("setting a template does not collapse the selected batch", async () => {
    const user = userEvent.setup();
    const otherFile = {
      ...templateFile,
      path: "/media/Show/Ep02.mkv",
      fileName: "Ep02.mkv"
    };
    const setFileSelection = vi.fn();
    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () =>
        Promise.resolve({
          ...emptyScan,
          files: [templateFile, otherFile],
          selectedPaths: [templateFile.path, otherFile.path],
          summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 }
        }),
      getWebSettings: () => Promise.resolve(settings()),
      setFileSelection
    });

    const otherRow = await screen.findByRole("row", { name: /Ep02\.mkv/i });
    fireEvent.contextMenu(otherRow);
    await user.click(screen.getByRole("button", { name: /set as template/i }));

    expect(setFileSelection).not.toHaveBeenCalled();
    expect(screen.getByText("Template: Ep02.mkv")).toBeInTheDocument();
  });

  it("keeps the template filename white like the other file details", async () => {
    renderWithTemplate();

    const rows = await screen.findAllByText("Ep01.mkv");
    const detail = rows.find((element) => element.tagName === "DD");
    expect(detail).toBeDefined();
    expect(detail).toHaveClass("text-text");
  });

  it("labels the selected template beside its media reader", async () => {
    renderWithTemplate();

    const templateBadge = await screen.findByText("Template File");
    expect(templateBadge).toBeInTheDocument();
    expect(templateBadge.nextElementSibling).toHaveTextContent("mkvmerge");
  });

  it("uses purple text for the template row in File Info", async () => {
    renderWithTemplate();

    const templateRow = await screen.findByRole("row", { name: /Ep01\.mkv/i });
    expect(templateRow).toHaveClass("text-template");
  });

  it("keeps the template's track rows white for a uniform schema", async () => {
    renderWithTemplate();

    const codec = (await screen.findAllByText("AC-3"))[0];
    expect(codec).toHaveClass("text-text");

    expect(screen.getByText("Ep01.Release")).toHaveClass("text-text");
  });

  it("identifies extra tracks by the mkvmerge ID shown in Track Info", async () => {
    const user = userEvent.setup();
    const fileWithExtraTrack = {
      ...templateFile,
      path: "/media/Show/Ep02.mkv",
      fileName: "Ep02.mkv",
      subtitleSummary: "eng x1",
      tracks: [
        ...templateFile.tracks,
        { id: 3, trackNumber: 4, type: "subtitles", codec: "SubRip/SRT", language: "eng", name: "SDH", default: false, forced: false }
      ]
    };

    renderDashboard({
      getStatus: () => Promise.resolve(emptyStatus),
      getCurrentScanFiles: () => Promise.resolve({
        ...emptyScan,
        files: [templateFile, fileWithExtraTrack],
        summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 }
      }),
      getWebSettings: () => Promise.resolve(settings())
    });

    await user.click(await screen.findByRole("row", { name: /Ep02\.mkv/i }));
    expect(await screen.findByText("Track ID 3 is extra (subtitles).")).toBeInTheDocument();
    expect(screen.queryByText(/Track 4 is extra/i)).not.toBeInTheDocument();
  });
});
