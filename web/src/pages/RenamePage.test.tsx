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
    path: `C:\media\${fileName}`,
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
          summary: { total: 0, mkv: 0, mp4: 0, failed: 0 },
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

  /// A title the user typed is theirs; a scan must not overwrite it.
  it("leaves a typed title alone when a new scan arrives", async () => {
    const user = userEvent.setup();
    const { rerender } = renderRename(["Justified.S01E01.mkv"]);
    const field = await searchTitleField();
    await waitFor(() => expect(field).toHaveValue("Justified"));

    await user.clear(field);
    await user.type(field, "My Own Search");

    rerenderWith(rerender, ["Cowboy.Bebop.S01E01.mkv"]);

    // Give the effect a chance to fire before concluding it did not.
    await waitFor(() => expect(screen.getByText(/scanned file\(s\)/i)).toBeInTheDocument());
    expect(await searchTitleField()).toHaveValue("My Own Search");
  });
});
