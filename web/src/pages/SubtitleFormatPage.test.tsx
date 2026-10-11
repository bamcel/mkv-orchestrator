import { useEffect } from "react";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";

import { SubtitleFormatPage } from "./SubtitleFormatPage";
import { MediaLibraryProvider, useMediaLibrary } from "../state/MediaLibraryContext";
import { OperationJobProvider } from "../state/OperationJobContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow, MuxPreviewRequest } from "../api";

const files: MediaFileRow[] = ["01.mkv", "02.mkv"].map((fileName) => ({
  path: `/media/${fileName}`, fileName, extension: ".mkv", status: "Scanned", reader: "mkvmerge", codec: "HEVC", resolution: "1080p", bitDepth: "10", hdr: "", videoSummary: "", audioSummary: "jpn", subtitleSummary: "eng", attachmentSummary: "", attachments: [],
  tracks: [
    { id: 2, trackNumber: 3, type: "subtitles", codec: "SubStationAlpha", language: "eng", name: "Dialogue", channels: null, default: true, forced: false },
    { id: 3, trackNumber: 4, type: "subtitles", codec: "ASS", language: "eng", name: "Signs/Songs", channels: null, default: false, forced: false }
  ]
}));

function Scan() {
  const library = useMediaLibrary();
  useEffect(() => { library.setFiles(files); library.setSelectedPaths(files.map((file) => file.path)); }, []);
  return null;
}

it("keeps timing by default and formats every selected file", async () => {
  localStorage.clear(); sessionStorage.clear();
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><Scan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { buildMuxPreview });

  expect(await screen.findByRole("radio", { name: "Keep the same timing" })).toBeChecked();
  expect(screen.getByText("Old subtitle format")).toBeInTheDocument();
  expect(screen.getByText("New subtitle format")).toBeInTheDocument();
  await user.clear(screen.getByRole("textbox", { name: "New font family" }));
  await user.type(screen.getByRole("textbox", { name: "New font family" }), "Noto Sans");
  await user.click(screen.getByRole("button", { name: "Preview Summary" }));

  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0]).toMatchObject({ formatSubtitles: true, subtitleFontFamily: "Noto Sans", subtitleTimingOffsetMs: 0, selectedPaths: ["/media/01.mkv", "/media/02.mkv"], subtitleTrackSelections: [
    { filePath: "/media/01.mkv", trackIds: [2, 3] },
    { filePath: "/media/02.mkv", trackIds: [2, 3] }
  ] });
});

it("formats only the subtitle tracks selected for each file", async () => {
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><Scan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { buildMuxPreview });

  await user.click(await screen.findByRole("checkbox", { name: "01.mkv track 4 Signs/Songs" }));
  await user.click(screen.getByRole("button", { name: "Preview Summary" }));

  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0].subtitleTrackSelections).toEqual([
    { filePath: "/media/01.mkv", trackIds: [2] },
    { filePath: "/media/02.mkv", trackIds: [2, 3] }
  ]);
});

it("sends a negative cue offset when timing is changed", async () => {
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><Scan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { buildMuxPreview });
  await user.click(await screen.findByRole("radio", { name: "Shift all cues" }));
  await user.clear(screen.getByRole("spinbutton", { name: "Timing offset in seconds" }));
  await user.type(screen.getByRole("spinbutton", { name: "Timing offset in seconds" }), "-1.5");
  await user.click(screen.getByRole("button", { name: "Preview Summary" }));
  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0].subtitleTimingOffsetMs).toBe(-1500);
});
