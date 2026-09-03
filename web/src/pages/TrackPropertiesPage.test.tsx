import { useMediaLibrary, MediaLibraryProvider } from "../state/MediaLibraryContext";
import { renderWithBackend } from "../test/render";
import type { MediaFileRow, PropEditTemplateRequest, PropEditTemplateResponse, WebSettings } from "../generated/contracts";
import { screen, waitFor, within } from "@testing-library/react";
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
  it("offers descriptive channel names separately from metadata-generated audio names", async () => {
    const scannedFile = file("Episode 01.mkv");
    const loadedTemplate = template(scannedFile.path);
    loadedTemplate.audioTracks = [
      {
        trackNumber: 1,
        trackLabel: "Audio 1",
        type: "audio",
        currentName: "",
        currentLanguage: "eng",
        currentCodec: "AAC",
        currentChannels: 6,
        currentDefault: true,
        editedName: "",
        nameFromMetadata: true,
        editedLanguage: "eng"
      },
      {
        trackNumber: 2,
        trackLabel: "Audio 2",
        type: "audio",
        currentName: "",
        currentLanguage: "eng",
        currentCodec: "AC-3",
        currentChannels: 2,
        currentDefault: false,
        editedName: "",
        nameFromMetadata: true,
        editedLanguage: "eng"
      }
    ];

    renderWithBackend(
      <MediaLibraryProvider>
        <TrackPropertiesPage />
      </MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({ updatedUtc: "2026-08-30T20:00:00Z", files: [scannedFile], selectedPaths: [scannedFile.path], summary: { total: 1, mkv: 1, mp4: 0, failed: 0, cached: 0 } }),
        getWebSettings: () => Promise.resolve({ audioNamePresets: [], subtitleNamePresets: [], languagePresets: [] } as unknown as WebSettings),
        loadPropEditTemplate: () => Promise.resolve(loadedTemplate)
      }
    );

    expect(await screen.findByRole("option", { name: "AAC English 5.1" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "5.1 Surround" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "AC-3 English 2.0" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "2.0 Stereo" })).toBeInTheDocument();
    expect(screen.getAllByRole("radio", { name: "Use episode title" })).toHaveLength(2);
    expect(screen.queryByRole("radio", { name: /custom/i })).not.toBeInTheDocument();
  });

  it("defaults audio and subtitle forced-track selections to None", async () => {
    const scannedFile = file("Episode 01.mkv");
    const loadedTemplate = template(scannedFile.path);
    loadedTemplate.forcedAudio = "";
    loadedTemplate.forcedSubtitle = "";

    renderWithBackend(
      <MediaLibraryProvider>
        <TrackPropertiesPage />
      </MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({ updatedUtc: "2026-08-25T20:00:00Z", files: [scannedFile], selectedPaths: [scannedFile.path], summary: { total: 1, mkv: 1, mp4: 0, failed: 0, cached: 0 } }),
        getWebSettings: () => Promise.resolve({ audioNamePresets: [], subtitleNamePresets: [], languagePresets: [] } as unknown as WebSettings),
        loadPropEditTemplate: () => Promise.resolve(loadedTemplate)
      }
    );

    await screen.findByText(`Template loaded: ${scannedFile.fileName}`);
    const forcedTrackSelections = screen.getAllByRole("combobox", { name: "Set forced track" });
    expect(forcedTrackSelections).toHaveLength(2);
    forcedTrackSelections.forEach((selection) => expect(selection).toHaveValue("None"));
  });

  it("can set or clear the first video track default flag", async () => {
    const user = userEvent.setup();
    const scannedFile = file("Episode 01.mkv");
    const loadedTemplate = template(scannedFile.path);
    const buildPropEditPreview = vi.fn(() => Promise.resolve({
      actions: [], skipped: [], noChange: [], summary: "No changes", status: "No changes",
      planId: null, planFingerprint: null, idempotencyKey: null
    }));

    renderWithBackend(
      <MediaLibraryProvider><TrackPropertiesPage /></MediaLibraryProvider>,
      {
        getCurrentScanFiles: () => Promise.resolve({ updatedUtc: "2026-08-25T20:00:00Z", files: [scannedFile], selectedPaths: [scannedFile.path], summary: { total: 1, mkv: 1, mp4: 0, failed: 0, cached: 0 } }),
        getWebSettings: () => Promise.resolve({ audioNamePresets: [], subtitleNamePresets: [], languagePresets: [] } as unknown as WebSettings),
        loadPropEditTemplate: () => Promise.resolve(loadedTemplate),
        buildPropEditPreview
      }
    );

    await screen.findByText(`Template loaded: ${scannedFile.fileName}`);
    const videoDefault = screen.getByRole("combobox", { name: "Set video default flag" });
    expect(within(videoDefault).getByRole("option", { name: "Default" })).toBeInTheDocument();
    expect(within(videoDefault).getByRole("option", { name: "None" })).toBeInTheDocument();

    await user.selectOptions(videoDefault, "Default");
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(buildPropEditPreview).toHaveBeenLastCalledWith(expect.objectContaining({ selectedDefaultVideo: "Default" })));

    await user.selectOptions(videoDefault, "None");
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(buildPropEditPreview).toHaveBeenLastCalledWith(expect.objectContaining({ selectedDefaultVideo: "None" })));
  });

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
