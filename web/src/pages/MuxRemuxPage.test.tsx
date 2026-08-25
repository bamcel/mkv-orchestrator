import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { buildRemovedTrackDetails, MuxRemuxPage } from "./MuxRemuxPage";
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

describe("MKV Operations file selection", () => {
  it("restores a running operation after navigating away and back", async () => {
    const files = [mediaFile("Episode 01.mkv")];
    window.sessionStorage.setItem("mkvo.web.activeOperationJob", JSON.stringify({ id: "mux-job", label: "MKV Operations" }));

    renderWithBackend(
      <MediaLibraryProvider><MuxRemuxPage /></MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({
          updatedUtc: "2026-08-25T20:00:00Z",
          files,
          selectedPaths: files.map((file) => file.path),
          summary: { total: 1, mkv: 1, mp4: 0, failed: 0, cached: 0 }
        }),
        getWebSettings: () => Promise.resolve({ mkvMergeDefaultAudioLanguages: "eng", mkvMergeDefaultSubtitleLanguages: "eng" } as WebSettings),
        getOperationJob: () => Promise.resolve({
          id: "mux-job",
          kind: "Remux",
          status: "Running",
          createdUtc: "2026-08-25T20:00:00Z",
          startedUtc: "2026-08-25T20:00:01Z",
          completedUtc: null,
          completed: 10,
          failed: 0,
          skipped: 0,
          total: 283,
          currentFile: "Episode 11.mkv",
          currentFilePercent: 50,
          lines: [],
          muxResult: null,
          propEditResult: null,
          error: ""
        })
      }
    );

    expect(await screen.findByText(/Applying 10\/283.*Episode 11\.mkv 50%/i)).toBeInTheDocument();
  });

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
          // A partial selection from another page must not change MKV
          // Operations' full-batch default.
          selectedPaths: [files[0].path],
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
    expect(screen.getByRole("button", { name: "Tracks" })).toBeInTheDocument();
    const selection = screen.getByLabelText("MKV Operations file selection");
    const checkboxes = within(selection).getAllByRole("checkbox");
    await waitFor(() => checkboxes.forEach((checkbox) => expect(checkbox).toBeChecked()));
    expect(setFileSelection).toHaveBeenCalledWith(files.map((file) => file.path));

    const rows = within(selection).getAllByRole("row").slice(1);
    await user.click(rows[0]);
    fireEvent.click(rows[1], { ctrlKey: true });
    fireEvent.keyDown(selection, { key: " " });
    await waitFor(() => checkboxes.forEach((checkbox) => expect(checkbox).not.toBeChecked()));

    fireEvent.keyDown(selection, { key: " " });
    await waitFor(() => checkboxes.forEach((checkbox) => expect(checkbox).toBeChecked()));

    await user.click(rows[0]);
    fireEvent.contextMenu(rows[0], { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: "Deselect highlighted rows" }));
    await waitFor(() => expect(checkboxes[0]).not.toBeChecked());
    expect(checkboxes[1]).toBeChecked();

    fireEvent.contextMenu(rows[0], { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: "Select highlighted rows" }));
    await waitFor(() => expect(checkboxes[0]).toBeChecked());
  });

  it("sorts the visible file rows while preserving path-based selection", async () => {
    const user = userEvent.setup();
    const files = [mediaFile("Episode 10.mkv"), mediaFile("Episode 2.mkv")];
    renderWithBackend(
      <MediaLibraryProvider><MuxRemuxPage /></MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({
          updatedUtc: "2026-08-25T20:00:00Z",
          files,
          selectedPaths: files.map((file) => file.path),
          summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 }
        }),
        getWebSettings: () => Promise.resolve({ mkvMergeDefaultAudioLanguages: "eng", mkvMergeDefaultSubtitleLanguages: "eng" } as WebSettings),
        setFileSelection: (selectedPaths) => Promise.resolve({ updatedUtc: "2026-08-25T20:00:00Z", files, selectedPaths, summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 } })
      }
    );

    const table = await screen.findByLabelText("MKV Operations file selection");
    const rows = () => within(table).getAllByRole("row").slice(1);
    await waitFor(() => expect(rows()[0]).toHaveTextContent("Episode 2.mkv"));
    await user.click(within(table).getByRole("button", { name: "Sort by File" }));
    expect(rows()[0]).toHaveTextContent("Episode 10.mkv");
    within(table).getAllByRole("checkbox").forEach((checkbox) => expect(checkbox).toBeChecked());
  });
});

describe("MKV Operations removed-track preview", () => {
  it("lists exact IDs and metadata removed by explicit and language filters", () => {
    const file = mediaFile("Episode 01.mkv");
    file.tracks = [
      { id: 0, trackNumber: 1, type: "video", codec: "HEVC/H.265", language: "und", name: "Main", channels: null, default: true, forced: false },
      { id: 1, trackNumber: 2, type: "audio", codec: "E-AC-3", language: "eng", name: "English", channels: 6, default: true, forced: false },
      { id: 2, trackNumber: 3, type: "audio", codec: "AAC", language: "spa", name: "Spanish", channels: 2, default: false, forced: false },
      { id: 3, trackNumber: 4, type: "subtitle", codec: "SubRip/SRT", language: "eng", name: "SDH", channels: null, default: false, forced: true }
    ];

    const details = buildRemovedTrackDetails({
      files: [file],
      selectedPaths: [file.path],
      removeUnwantedAudioLanguages: true,
      keepAudioLanguages: "eng",
      removeUnwantedSubtitleLanguages: false,
      keepSubtitleLanguages: "eng",
      removeUnwantedTrackIds: true,
      removeTrackIdsText: "3"
    })[file.path.toLowerCase()];

    expect(details).toEqual([
      "Removed tracks (2):",
      "• ID 2 · audio · spa · AAC · Spanish",
      "• ID 3 · subtitle · eng · SubRip/SRT · SDH · forced"
    ]);
  });
});
