import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { MuxRemuxPage } from "./MuxRemuxPage";
import { MediaLibraryProvider } from "../state/MediaLibraryContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow, WebSettings } from "../generated/contracts";

function mediaFile(name: string): MediaFileRow {
  return {
    path: `/media/Show/${name}`,
    fileName: name,
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

beforeEach(() => {
  window.localStorage.clear();
  window.sessionStorage.clear();
});

describe("Mux/remux file selection", () => {
  it("selects every current file by default and offers select-all controls on right click", async () => {
    const user = userEvent.setup();
    const files = [mediaFile("Episode 01.mkv"), mediaFile("Episode 02.mkv")];
    const setFileSelection = vi.fn((selectedPaths: string[]) => Promise.resolve({
      updatedUtc: "2026-08-15T20:00:00Z",
      files,
      selectedPaths,
      summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 }
    }));

    renderWithBackend(
      <MediaLibraryProvider><MuxRemuxPage /></MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({
          updatedUtc: "2026-08-15T20:00:00Z",
          files,
          selectedPaths: ["/media/Old/removed.mkv"],
          summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 }
        }),
        getWebSettings: () => Promise.resolve({
          mkvMergeDefaultAudioLanguages: "eng,jpn",
          mkvMergeDefaultSubtitleLanguages: "eng"
        } as WebSettings),
        setFileSelection
      }
    );

    await screen.findByText("Episode 01.mkv");
    const selection = screen.getByLabelText("Mux/remux file selection");
    const checkboxes = within(selection).getAllByRole("checkbox");
    await waitFor(() => checkboxes.forEach((checkbox) => expect(checkbox).toBeChecked()));
    expect(setFileSelection).toHaveBeenCalledWith(files.map((file) => file.path));

    fireEvent.contextMenu(selection, { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: "Deselect all" }));
    await waitFor(() => checkboxes.forEach((checkbox) => expect(checkbox).not.toBeChecked()));

    fireEvent.contextMenu(selection, { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: "Select all" }));
    await waitFor(() => checkboxes.forEach((checkbox) => expect(checkbox).toBeChecked()));
  });
});
