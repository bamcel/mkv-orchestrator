import { beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";

import { MediaLibraryProvider, useMediaLibrary } from "./MediaLibraryContext";
import { PropEditTemplateWarmer, usePropEditTemplate } from "./propEditTemplate";
import { setBackendClient } from "../backend/runtime";
import { createMockBackendClient } from "../backend/mockClient";
import type { MediaFileRow } from "../api";
import { useEffect } from "react";

function file(path: string, name = ""): MediaFileRow {
  return {
    path,
    fileName: path.split(/[\/]/).pop() ?? path,
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
    tracks: [
      { id: 1, trackNumber: 2, type: "audio", codec: "AC-3", language: "eng", name, default: true, forced: false }
    ],
    attachments: []
  };
}

/** Seeds the library, then renders the warmer against it. */
function Harness({ files }: { files: MediaFileRow[] }) {
  const { setFiles } = useMediaLibrary();
  // `setFiles` is rebuilt whenever the library changes, so depending on it here
  // would re-run this effect forever.
  useEffect(() => setFiles(files), [files]);
  return <PropEditTemplateWarmer />;
}

function renderWarmer(files: MediaFileRow[], loadPropEditTemplate: ReturnType<typeof vi.fn>) {
  setBackendClient(createMockBackendClient({ loadPropEditTemplate }));
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={client}>
      <MediaLibraryProvider>
        <Harness files={files} />
      </MediaLibraryProvider>
    </QueryClientProvider>
  );
  return { client, ...result };
}

const template = {
  templatePath: "C:\media\Ep01.mkv",
  templateFileName: "Ep01.mkv",
  audioTracks: [],
  subtitleTracks: [],
  defaultAudio: "",
  forcedAudio: "",
  defaultSubtitle: "",
  forcedSubtitle: ""
};

describe("prop edit template caching", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  /// Reading a template costs about a second over a share. Starting it when
  /// the scan lands is what stops Track Properties paying for it on arrival.
  it("reads the template as soon as files arrive, without anyone visiting the page", async () => {
    const loadPropEditTemplate = vi.fn().mockResolvedValue(template);
    renderWarmer([file("C:\media\Ep01.mkv")], loadPropEditTemplate);

    await waitFor(() => expect(loadPropEditTemplate).toHaveBeenCalledTimes(1));
  });

  /// The whole point of the cache: arriving at the page must not refetch.
  it("serves a warmed template from cache", async () => {
    const loadPropEditTemplate = vi.fn().mockResolvedValue(template);
    const files = [file("C:\media\Ep01.mkv")];
    const { client } = renderWarmer(files, loadPropEditTemplate);
    await waitFor(() => expect(loadPropEditTemplate).toHaveBeenCalledTimes(1));

    function Consumer() {
      const query = usePropEditTemplate("C:\media\Ep01.mkv", files);
      return <div>{query.data ? query.data.templateFileName : "loading"}</div>;
    }

    const view = render(
      <QueryClientProvider client={client}>
        <Consumer />
      </QueryClientProvider>
    );

    // Present on the very first paint, with no further call to the host.
    expect(view.getByText("Ep01.mkv")).toBeInTheDocument();
    expect(loadPropEditTemplate).toHaveBeenCalledTimes(1);
  });

  /// A rescan that finds different tracks is a different answer, so the cache
  /// must not serve the old one.
  it("re-reads when a file's tracks change", async () => {
    const loadPropEditTemplate = vi.fn().mockResolvedValue(template);
    const { rerender, client } = renderWarmer([file("C:\media\Ep01.mkv", "Original")], loadPropEditTemplate);
    await waitFor(() => expect(loadPropEditTemplate).toHaveBeenCalledTimes(1));

    rerender(
      <QueryClientProvider client={client}>
        <MediaLibraryProvider>
          <Harness files={[file("C:\media\Ep01.mkv", "Edited")]} />
        </MediaLibraryProvider>
      </QueryClientProvider>
    );

    await waitFor(() => expect(loadPropEditTemplate).toHaveBeenCalledTimes(2));
  });
});
