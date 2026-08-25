import { useMediaLibrary, MediaLibraryProvider } from "../state/MediaLibraryContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow, PropEditTemplateRequest, PropEditTemplateResponse, WebSettings } from "../generated/contracts";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TrackPropertiesPage } from "./TrackPropertiesPage";

function file(name: string): MediaFileRow {
  return {
    path: `/media/${name}`,
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

function template(path: string): PropEditTemplateResponse {
  return {
    templatePath: path,
    templateFileName: path.split("/").pop() ?? path,
    audioTracks: [],
    subtitleTracks: [],
    defaultAudio: "Keep existing",
    forcedAudio: "Keep existing",
    defaultSubtitle: "Keep existing",
    forcedSubtitle: "Keep existing"
  };
}

function SelectSecondTemplate({ path }: { path: string }) {
  const { setTemplateFilePath } = useMediaLibrary();
  return <button type="button" onClick={() => setTemplateFilePath(path)}>Set second template</button>;
}

beforeEach(() => {
  window.localStorage.clear();
  window.sessionStorage.clear();
});

describe("Track Properties template synchronization", () => {
  it("reloads its track template when Dashboard changes the shared template file", async () => {
    const user = userEvent.setup();
    const first = file("Episode 01.mkv");
    const second = file("Episode 02.mkv");
    const files = [first, second];
    window.sessionStorage.setItem("mkvo.web.scannedFiles", JSON.stringify(files));
    window.sessionStorage.setItem("mkvo.web.templateFilePath", first.path);
    const loadPropEditTemplate = vi.fn((request: PropEditTemplateRequest) => Promise.resolve(template(request.templatePath ?? "")));

    renderWithBackend(
      <MediaLibraryProvider>
        <SelectSecondTemplate path={second.path} />
        <TrackPropertiesPage />
      </MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({ updatedUtc: "2026-08-25T20:00:00Z", files, selectedPaths: files.map((item) => item.path), summary: { total: 2, mkv: 2, mp4: 0, failed: 0, cached: 0 } }),
        getWebSettings: () => Promise.resolve({ audioNamePresets: [], subtitleNamePresets: [], languagePresets: [] } as unknown as WebSettings),
        loadPropEditTemplate
      }
    );

    await waitFor(() => expect(loadPropEditTemplate).toHaveBeenCalledWith({ files, templatePath: first.path }));
    await user.click(screen.getByRole("button", { name: "Set second template" }));
    await waitFor(() => expect(loadPropEditTemplate).toHaveBeenCalledWith({ files, templatePath: second.path }));
    expect(screen.getByRole("combobox", { name: /template file/i })).toHaveValue(second.path);
    expect(await screen.findByText(`Template loaded: ${second.fileName}`)).toBeInTheDocument();
  });
});
