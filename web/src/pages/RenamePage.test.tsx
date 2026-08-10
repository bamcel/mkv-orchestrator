import { beforeEach, describe, expect, it } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";

import { RenamePage } from "./RenamePage";
import { MediaLibraryProvider, useMediaLibrary } from "../state/MediaLibraryContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow } from "../api";

function mediaFile(fileName: string): MediaFileRow {
  return {
    path: String.raw`C:\media` + "\\" + fileName,
    fileName,
    extension: ".mkv",
    status: "Scanned",
    reader: "mkvmerge",
    codec: "HEVC/H.265",
    resolution: "1920x1080",
    bitDepth: "",
    hdr: "",
    videoSummary: "",
    audioSummary: "eng x1",
    subtitleSummary: "",
    attachmentSummary: "None",
    tracks: [],
    attachments: []
  };
}

/**
 * Puts a scan into the live library, the way the Dashboard does.
 *
 * Mounting a second tree would not do: the working set is persisted, so a
 * fresh mount restores the previous scan rather than adopting a new one.
 */
function Scan({ fileNames }: { fileNames: string[] }) {
  const { setFiles } = useMediaLibrary();
  // `setFiles` is rebuilt whenever the library changes, so depending on it
  // here would re-run this forever.
  useEffect(() => setFiles(fileNames.map(mediaFile)), [fileNames]);
  return null;
}

function renderRename(fileNames: string[]) {
  return renderWithBackend(
    <MediaLibraryProvider>
      <Scan fileNames={fileNames} />
      <RenamePage />
    </MediaLibraryProvider>,
    {
      getCurrentScanFiles: () =>
        Promise.resolve({
          updatedUtc: null,
          files: [],
          summary: { total: 0, mkv: 0, mp4: 0, failed: 0, cached: 0 },
          selectedPaths: []
        })
    }
  );
}

function rerenderWith(rerender: (ui: React.ReactElement) => void, fileNames: string[]) {
  rerender(
    <MediaLibraryProvider>
      <Scan fileNames={fileNames} />
      <RenamePage />
    </MediaLibraryProvider>
  );
}

async function searchTitleField() {
  return (await screen.findAllByRole("textbox"))[0];
}

const searchResult = (format: "movie" | "series") => ({
  id: format === "movie" ? "movie:12345" : "12345",
  name: "Obsession",
  year: "2026",
  overview: "",
  provider: "tmdb",
  format,
  databaseUrl: "",
  displayName: "Obsession (2026)",
  providerDisplay: "TMDB"
});

/** Renders the page and runs a search that returns one result of this kind. */
async function searchReturning(format: "movie" | "series") {
  const user = userEvent.setup();
  renderWithBackend(
    <MediaLibraryProvider>
      <Scan fileNames={["Obsession (Bluray) 2026.mkv"]} />
      <RenamePage />
    </MediaLibraryProvider>,
    {
      getCurrentScanFiles: () =>
        Promise.resolve({
          updatedUtc: null,
          files: [],
          summary: { total: 0, mkv: 0, mp4: 0, failed: 0, cached: 0 },
          selectedPaths: []
        }),
      searchRenameMetadata: () => Promise.resolve({ results: [searchResult(format)] }),
      loadRenameScopes: () =>
        Promise.resolve({ scopes: [{ key: "all", label: "All episodes (78)", isSelected: false }] })
    }
  );

  const title = (await screen.findAllByRole("textbox"))[0];
  await user.type(title, "Obsession");
  await user.click(screen.getByRole("button", { name: /^search$/i }));
  return user;
}

describe("rename episode scope", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  /// A film has no seasons to pick between, so offering the list as though a
  /// choice were pending is misleading.
  it("greys the episode list out for a movie", async () => {
    await searchReturning("movie");

    await waitFor(() =>
      expect(screen.getByText(/not applicable to a movie/i)).toBeInTheDocument()
    );
    const scope = screen.getByText(/not applicable to a movie/i).parentElement;
    expect(scope).toHaveAttribute("aria-disabled", "true");
  });

  it("keeps the episode list usable for a series", async () => {
    await searchReturning("series");

    await waitFor(() => expect(screen.getByText("All episodes (78)")).toBeInTheDocument());
    expect(screen.queryByText(/not applicable to a movie/i)).not.toBeInTheDocument();
  });
});

describe("rename search title", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  it("fills itself in from the scanned filenames", async () => {
    renderRename(["Justified.S01E01.1080p.mkv", "Justified.S01E02.1080p.mkv"]);

    await waitFor(async () => expect(await searchTitleField()).toHaveValue("Justified"));
  });

  /// The field is persisted, so the title from the first scan used to stick for
  /// the whole session and a later scan kept searching for the wrong show.
  it("follows a later scan of a different show", async () => {
    const { rerender } = renderRename(["Justified.S01E01.mkv"]);
    await waitFor(async () => expect(await searchTitleField()).toHaveValue("Justified"));

    rerenderWith(rerender, ["Cowboy.Bebop.S01E01.mkv", "Cowboy.Bebop.S01E02.mkv"]);

    await waitFor(async () => expect(await searchTitleField()).toHaveValue("Cowboy Bebop"));
  });

  /// Scanning a second film while the first film's title sat in the box meant
  /// searching for the wrong one, so a scan replaces whatever is there.
  it("replaces a typed title when a new scan arrives", async () => {
    const user = userEvent.setup();
    const { rerender } = renderRename(["Justified.S01E01.mkv"]);
    const field = await searchTitleField();
    await waitFor(() => expect(field).toHaveValue("Justified"));

    await user.clear(field);
    await user.type(field, "My Own Search");

    rerenderWith(rerender, ["Cowboy.Bebop.S01E01.mkv"]);

    // Give the effect a chance to fire before concluding it did not.
    await waitFor(async () => expect(await searchTitleField()).toHaveValue("Cowboy Bebop"));
  });
});
