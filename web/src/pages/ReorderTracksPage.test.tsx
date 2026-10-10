import { useEffect } from "react";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";

import { ReorderTracksPage } from "./ReorderTracksPage";
import { MediaLibraryProvider, useMediaLibrary } from "../state/MediaLibraryContext";
import { OperationJobProvider } from "../state/OperationJobContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow, MuxPreviewRequest } from "../api";

const tracks = [
  { id: 0, trackNumber: 1, type: "video", codec: "HEVC", language: "und", name: "", channels: null, default: true, forced: false },
  { id: 1, trackNumber: 2, type: "subtitles", codec: "ASS", language: "eng", name: "Dialogue", channels: null, default: true, forced: false },
  { id: 2, trackNumber: 3, type: "subtitles", codec: "ASS", language: "eng", name: "Signs/Songs", channels: null, default: false, forced: false }
];

function file(name: string): MediaFileRow {
  return { path: `/media/${name}`, fileName: name, extension: ".mkv", status: "Scanned", reader: "mkvmerge", codec: "HEVC", resolution: "1080p", bitDepth: "10", hdr: "", videoSummary: "", audioSummary: "", subtitleSummary: "eng x2", attachmentSummary: "", tracks, attachments: [] };
}

function Scan({ files }: { files: MediaFileRow[] }) {
  const library = useMediaLibrary();
  useEffect(() => {
    library.setFiles(files);
    library.setSelectedPaths(files.map((item) => item.path));
  }, []);
  return null;
}

it("builds a batch request from the user-arranged template order", async () => {
  localStorage.clear(); sessionStorage.clear();
  const files = [file("01.mkv"), file("02.mkv")];
  const buildMuxPreview = vi.fn((_request: MuxPreviewRequest) => Promise.resolve({ actions: [], noChangeFiles: [], summary: "ready", status: "ready", planId: null, planFingerprint: null, idempotencyKey: null }));
  const user = userEvent.setup();
  renderWithBackend(
    <MediaLibraryProvider><OperationJobProvider><Scan files={files}/><ReorderTracksPage/></OperationJobProvider></MediaLibraryProvider>,
    { buildMuxPreview, setFileSelection: (paths) => Promise.resolve({ files, selectedPaths: paths, updatedUtc: null, summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 } }) }
  );

  await screen.findByText("Dialogue");
  await user.click(screen.getByRole("button", { name: "Move Dialogue down" }));
  await user.click(screen.getByRole("button", { name: "Preview" }));

  await waitFor(() => expect(buildMuxPreview).toHaveBeenCalled());
  expect(buildMuxPreview.mock.calls[0][0]).toMatchObject({
    reorderTracks: true,
    reorderTemplatePath: "/media/01.mkv",
    reorderTemplateTrackIds: [0, 2, 1],
    preserveOriginal: false
  });
});
