import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
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

    await user.click(await screen.findByRole("button", { name: /build library/i }));
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
});
