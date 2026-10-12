import { useEffect } from "react";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";

import { SubtitleFormatPage } from "./SubtitleFormatPage";
import { MediaLibraryProvider, useMediaLibrary } from "../state/MediaLibraryContext";
import { OperationJobProvider } from "../state/OperationJobContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow, MuxPreviewRequest, WebSettings } from "../api";

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

function SrtScan() {
  const library = useMediaLibrary();
  useEffect(() => { library.setFiles([{ ...files[0], path: "/media/srt.mkv", fileName: "srt.mkv", tracks: [{ id: 4, trackNumber: 5, type: "subtitles", codec: "SubRip/SRT", language: "eng", name: "English", channels: null, default: true, forced: false }] }]); library.setSelectedPaths(["/media/srt.mkv"]); }, []);
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
  expect(buildMuxPreview.mock.calls[0][0]).toMatchObject({ formatSubtitles: true, subtitleFontFamily: "Noto Sans", subtitleFontSize: "48", subtitleTimingOffsetMs: 0, selectedPaths: ["/media/01.mkv", "/media/02.mkv"], subtitleTrackSelections: [
    { filePath: "/media/01.mkv", trackIds: [2, 3] },
    { filePath: "/media/02.mkv", trackIds: [2, 3] }
  ] });
});

it("applies the selected font size", async () => {
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><Scan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { buildMuxPreview });

  const size = await screen.findByRole("spinbutton", { name: "Font size" });
  await user.clear(size);
  await user.type(size, "56");
  await user.click(screen.getByRole("button", { name: "Preview Summary" }));

  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0].subtitleFontSize).toBe("56");
  expect(screen.getByText("56")).toBeInTheDocument();
});

it("offers SRT tracks for conversion to styled ASS", async () => {
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><SrtScan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { buildMuxPreview });

  expect(await screen.findByText(/SRT → ASS/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Preview Summary" }));
  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0].subtitleTrackSelections).toEqual([{ filePath: "/media/srt.mkv", trackIds: [4] }]);
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

it("batch selects and deselects a Track ID across all selected files", async () => {
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><Scan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { buildMuxPreview });

  const batchTrack = await screen.findByRole("checkbox", { name: "Batch Track ID 3" });
  expect(batchTrack).toBeChecked();
  await user.click(batchTrack);
  expect(screen.getByRole("checkbox", { name: "01.mkv track 4 Signs/Songs" })).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: "02.mkv track 4 Signs/Songs" })).not.toBeChecked();
  await user.click(screen.getByRole("button", { name: "Preview Summary" }));

  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0].subtitleTrackSelections).toEqual([
    { filePath: "/media/01.mkv", trackIds: [2] },
    { filePath: "/media/02.mkv", trackIds: [2] }
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

it("shows an imported font immediately and saves it in MKVO settings", async () => {
  const getWebSettings = vi.fn(() => Promise.resolve({ subtitleFonts: [] } as unknown as WebSettings));
  const saveWebSettings = vi.fn((request) => Promise.resolve({ subtitleFonts: request.subtitleFonts ?? [] } as unknown as WebSettings));
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><OperationJobProvider><Scan/><SubtitleFormatPage/></OperationJobProvider></MediaLibraryProvider>, { getWebSettings, saveWebSettings });

  const font = new File([new Uint8Array([79, 84, 84, 79])], "DrawingBrutalDemo.otf", { type: "font/otf" });
  await user.upload(screen.getByLabelText("Import font file"), font);

  await waitFor(() => expect(saveWebSettings).toHaveBeenCalled());
  expect(screen.getByRole("combobox", { name: "Saved font" })).toHaveDisplayValue("DrawingBrutalDemo");
  expect(screen.getByRole("button", { name: "Remove imported" })).toBeEnabled();
  expect(saveWebSettings.mock.calls.at(-1)?.[0].subtitleFonts).toEqual([
    expect.objectContaining({ family: "DrawingBrutalDemo", fileName: "DrawingBrutalDemo.otf", dataBase64: "T1RUTw==" })
  ]);
});
