import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { buildLibraryTitles, LibraryPage } from "./LibraryPage";
import { MediaLibraryProvider } from "../state/MediaLibraryContext";
import { renderWithBackend } from "../test/render";
import type { LibraryAuditRow, MediaFileRow, ScanJobResponse, WebSettings } from "../api";

function auditRow(season: number, hasIssues: boolean): LibraryAuditRow {
  const root = `/media/tv/Example Show/Season ${season}`;
  return {
    folderPath: root,
    folderName: `Example Show / Season ${season}`,
    fileCount: 2,
    standardVideo: "1920x1080 HEVC/H.265 10bit",
    standardAudio: "eng:E-AC-3",
    standardSubtitles: "eng:SubRip/SRT",
    templateFilePath: `${root}/Episode 01.mkv`,
    templateFileName: "Episode 01.mkv",
    hasIssues,
    issueSummary: hasIssues ? "1 issue(s)" : "Standard",
    issues: hasIssues ? ["Episode 02.mkv: subtitles mismatch"] : [],
    issueFilePaths: hasIssues ? [`${root}/Episode 02.mkv`] : [],
    allFilePaths: [`${root}/Episode 01.mkv`, `${root}/Episode 02.mkv`]
  };
}

function media(path: string): MediaFileRow {
  return {
    path,
    fileName: path.split("/").pop()!,
    extension: ".mkv",
    status: "Scanned",
    reader: "mkvmerge",
    codec: "HEVC/H.265",
    resolution: "1920x1080",
    bitDepth: "10",
    hdr: "None",
    videoSummary: "HEVC/H.265",
    audioSummary: "eng x1",
    subtitleSummary: "eng x1",
    attachmentSummary: "None",
    tracks: [],
    attachments: []
  };
}

function settings(): WebSettings {
  return {
    watchFolders: [],
    ignoredScanFolderNames: [],
    mediaServers: [{
      id: "server-1",
      name: "Jellyfin",
      type: "Jellyfin",
      serverUrl: "http://jellyfin",
      hasApiKey: true,
      isDefault: true,
      lastSyncedUtc: null,
      libraries: [{ id: "tv-1", name: "TV Shows", type: "tvshows", serverPath: "/srv/tv", containerPath: "/media/tv", isEnabled: true }]
    }]
  } as WebSettings;
}

beforeEach(() => {
  window.localStorage.clear();
  window.sessionStorage.clear();
});

describe("Library title aggregation", () => {
  it("combines every season into one series poster and matches media-server metadata", () => {
    const titles = buildLibraryTitles(
      [auditRow(1, true), auditRow(2, false)],
      [{ id: "series-1", title: "Example Show", year: 2024, mediaType: "series", hasPoster: true }]
    );

    expect(titles).toHaveLength(1);
    expect(titles[0]).toMatchObject({ title: "Example Show", fileCount: 4, mismatchFileCount: 1, hasIssues: true });
    expect(titles[0].seasons).toHaveLength(2);
    expect(titles[0].catalogItem?.id).toBe("series-1");
  });

  it("combines audit groups whose folder labels include the series and season", () => {
    const season1 = {
      ...auditRow(1, false),
      folderPath: "/media/tv/Roseanne (1988)/Roseanne - Season 01",
      folderName: "Roseanne - Season 01 / movie/single folder"
    };
    const season2 = {
      ...auditRow(2, false),
      folderPath: "/media/tv/Roseanne (1988)/Roseanne - Season 02",
      folderName: "Roseanne - Season 02 / movie/single folder"
    };

    const titles = buildLibraryTitles([season1, season2], []);

    expect(titles).toHaveLength(1);
    expect(titles[0].title).toBe("Roseanne (1988)");
    expect(titles[0].seasons).toHaveLength(2);
  });
});

describe("Library poster workflow", () => {
  it("shows a slate placeholder and offers both Dashboard handoff scopes", async () => {
    const user = userEvent.setup();
    const rows = [auditRow(1, true), auditRow(2, false)];
    const files = rows.flatMap((row) => row.allFilePaths.map(media));
    const setFileSelection = vi.fn().mockImplementation((selectedPaths: string[]) => Promise.resolve({
      updatedUtc: "2026-08-22T20:00:00Z",
      files,
      selectedPaths,
      summary: { total: 4, mkv: 4, mp4: 0, failed: 0, cached: 0 }
    }));
    const getLibraryLocalArtwork = vi.fn().mockRejectedValue(new Error("No local poster"));
    const completedJob = {
      id: "scan-1",
      status: "Completed",
      completed: 4,
      total: 4,
      files,
      summary: { total: 4, mkv: 4, mp4: 0, failed: 0, cached: 0 },
      skipped: [],
      error: ""
    } as ScanJobResponse;

    renderWithBackend(
      <MediaLibraryProvider><LibraryPage /></MediaLibraryProvider>,
      {
        getWebSettings: () => Promise.resolve(settings()),
        getLibraryCatalog: () => Promise.resolve({ items: [{ id: "series-1", title: "Example Show", year: 2024, mediaType: "series", hasPoster: false }] }),
        getLibraryLocalArtwork,
        startScan: () => Promise.resolve({ ...completedJob, status: "Queued", files: [] }),
        getScanJob: () => Promise.resolve(completedJob),
        buildLibraryAudit: () => Promise.resolve({ summary: { groups: 2, files: 4, issueGroups: 1, standardGroups: 1 }, items: rows }),
        setFileSelection
      }
    );

    const card = await screen.findByRole("button", { name: /open example show library details/i });
    await waitFor(() => expect(getLibraryLocalArtwork).toHaveBeenCalledWith({
      folderPaths: [
        "/media/tv/Example Show/Season 1",
        "/media/tv/Example Show/Season 2"
      ]
    }));
    expect(screen.getByText(/no poster found/i)).toBeInTheDocument();
    expect(screen.getByText(/mismatch · 1/i)).toBeInTheDocument();

    await user.click(card);
    expect(await screen.findByRole("button", { name: /send mismatches \+ template to dashboard/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /send all files to dashboard/i })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: /send mismatches \+ template to dashboard/i }));

    await waitFor(() => expect(setFileSelection).toHaveBeenCalledWith([
      "/media/tv/Example Show/Season 1/Episode 01.mkv",
      "/media/tv/Example Show/Season 1/Episode 02.mkv"
    ]));
  });

  it("loads every library automatically and restores each one immediately", async () => {
    const user = userEvent.setup();
    const rowFor = (title: string, folder: string): LibraryAuditRow => {
      const base = `/media/${folder}/${title}/Season 1`;
      return {
        ...auditRow(1, false),
        folderPath: base,
        folderName: `${title} / Season 1`,
        templateFilePath: `${base}/Episode 01.mkv`,
        issueFilePaths: [],
        allFilePaths: [`${base}/Episode 01.mkv`, `${base}/Episode 02.mkv`]
      };
    };
    const alpha = rowFor("Alpha Show", "alpha");
    const beta = rowFor("Beta Show", "beta");
    const scanFiles = new Map<string, MediaFileRow[]>();
    let scanNumber = 0;
    const startScan = vi.fn().mockImplementation((request: { sources: string[] }) => {
      scanNumber += 1;
      const id = `scan-${scanNumber}`;
      const rows = [
        ...(request.sources.some((source) => source.includes("alpha")) ? [alpha] : []),
        ...(request.sources.some((source) => source.includes("beta")) ? [beta] : [])
      ];
      scanFiles.set(id, rows.flatMap((row) => row.allFilePaths.map(media)));
      return Promise.resolve({ id, status: "Queued", files: [], completed: 0, total: 0 } as ScanJobResponse);
    });
    const persistentSettings = {
      ...settings(),
      mediaServers: [{
        ...settings().mediaServers[0],
        id: "persistent-server",
        libraries: [
          { id: "alpha", name: "Alpha", type: "tvshows", serverPath: "/srv/alpha", containerPath: "/media/alpha", isEnabled: true },
          { id: "beta", name: "Beta", type: "tvshows", serverPath: "/srv/beta", containerPath: "/media/beta", isEnabled: true }
        ]
      }]
    } as WebSettings;

    renderWithBackend(
      <MediaLibraryProvider><LibraryPage /></MediaLibraryProvider>,
      {
        getWebSettings: () => Promise.resolve(persistentSettings),
        getLibraryCatalog: ({ libraryName }) => Promise.resolve({
          items: [{ id: libraryName, title: `${libraryName} Show`, year: 2024, mediaType: "series", hasPoster: false }]
        }),
        getLibraryLocalArtwork: () => Promise.reject(new Error("No local poster")),
        startScan,
        getScanJob: (id) => {
          const files = scanFiles.get(id) ?? [];
          return Promise.resolve({
            id,
            status: "Completed",
            completed: files.length,
            total: files.length,
            files,
            summary: { total: files.length, mkv: files.length, mp4: 0, failed: 0, cached: 0 },
            skipped: [],
            error: ""
          } as ScanJobResponse);
        },
        buildLibraryAudit: (files) => {
          const row = files[0]?.path.includes("/alpha/") ? alpha : beta;
          return Promise.resolve({
            summary: { groups: 1, files: files.length, issueGroups: 0, standardGroups: 1 },
            items: [row]
          });
        }
      }
    );

    expect(await screen.findByRole("button", { name: /open alpha show library details/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Beta", exact: true }));
    expect(await screen.findByRole("button", { name: /open beta show library details/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /rebuild selected library/i }));
    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(2));
    expect(startScan).toHaveBeenLastCalledWith(expect.objectContaining({
      sources: ["/media/beta"],
      forceRefresh: false
    }));

    await user.click(screen.getByRole("button", { name: "Alpha", exact: true }));
    expect(screen.getByRole("button", { name: /open alpha show library details/i })).toBeInTheDocument();
    expect(startScan).toHaveBeenNthCalledWith(1, expect.objectContaining({
      sources: expect.arrayContaining(["/media/alpha", "/media/beta"]),
      forceRefresh: false
    }));
  });

  it("rebuilds only the right-clicked title and refreshes its cached audit", async () => {
    const user = userEvent.setup();
    const rows = [auditRow(1, false), auditRow(2, false)];
    const files = rows.flatMap((row) => row.allFilePaths.map(media));
    const startScan = vi.fn()
      .mockResolvedValueOnce({ id: "overview-scan", status: "Queued", files: [], completed: 0, total: 0 } as ScanJobResponse)
      .mockResolvedValueOnce({ id: "title-scan", status: "Queued", files: [], completed: 0, total: 0 } as ScanJobResponse);
    const buildLibraryAudit = vi.fn().mockImplementation((scannedFiles: MediaFileRow[]) => Promise.resolve({
      summary: { groups: rows.length, files: scannedFiles.length, issueGroups: 0, standardGroups: rows.length },
      items: rows
    }));
    const titleSettings = {
      ...settings(),
      mediaServers: [{
        ...settings().mediaServers[0],
        id: "title-rebuild-server",
        libraries: [{ id: "title-tv", name: "Title TV", type: "tvshows", serverPath: "/srv/tv", containerPath: "/media/tv", isEnabled: true }]
      }]
    } as WebSettings;

    renderWithBackend(
      <MediaLibraryProvider><LibraryPage /></MediaLibraryProvider>,
      {
        getWebSettings: () => Promise.resolve(titleSettings),
        getLibraryCatalog: () => Promise.resolve({ items: [{ id: "example", title: "Example Show", year: 2024, mediaType: "series", hasPoster: false }] }),
        getLibraryLocalArtwork: () => Promise.reject(new Error("No local poster")),
        startScan,
        getScanJob: (id) => Promise.resolve({
          id,
          status: "Completed",
          completed: files.length,
          total: files.length,
          files,
          summary: { total: files.length, mkv: files.length, mp4: 0, failed: 0, cached: 0 },
          skipped: [],
          error: ""
        } as ScanJobResponse),
        buildLibraryAudit
      }
    );

    const card = await screen.findByRole("button", { name: /open example show library details/i });
    fireEvent.contextMenu(card, { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: /rebuild this title/i }));

    await waitFor(() => expect(startScan).toHaveBeenCalledTimes(2));
    expect(startScan).toHaveBeenLastCalledWith({
      sources: ["/media/tv/Example Show"],
      ignoredFolderNames: [],
      forceRefresh: true
    });
    await waitFor(() => expect(buildLibraryAudit).toHaveBeenCalledTimes(2));
    expect(await screen.findByText(/example show rebuilt: 4 files/i)).toBeInTheDocument();
  });
});
