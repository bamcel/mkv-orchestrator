import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

import { MediaLibraryProvider, useMediaLibrary } from "./MediaLibraryContext";
import { createMockBackendClient } from "../backend/mockClient";
import { setBackendClient } from "../backend/runtime";
import type { MediaFileRow } from "../api";

function file(path: string, extension = ".mkv"): MediaFileRow {
  return {
    path,
    fileName: path.split(/[\\/]/).pop() ?? path,
    extension,
    status: "Scanned",
    reader: "mkvmerge",
    codec: "AVC/H.264",
    resolution: "1920x1080",
    bitDepth: "8bit",
    hdr: "",
    videoSummary: "",
    audioSummary: "",
    subtitleSummary: "",
    attachmentSummary: "",
    tracks: [],
    attachments: []
  };
}

function library() {
  return renderHook(() => useMediaLibrary(), { wrapper: MediaLibraryProvider });
}

describe("working set persistence", () => {
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });

  /// Re-picking files across a large library after every reload is the whole
  /// complaint; the selection has to outlive the page.
  it("restores the selection after a reload", () => {
    const first = library();
    act(() => {
      first.result.current.setFiles([file("/media/a.mkv"), file("/media/b.mkv")]);
      first.result.current.setSelectedPaths(["/media/b.mkv"]);
    });
    first.unmount();

    // A fresh provider stands in for the reloaded page.
    const second = library();
    expect(second.result.current.selectedPaths).toEqual(["/media/b.mkv"]);
  });

  /// A remux or conversion changes which paths exist. A selection left pointing
  /// at a path that is gone would be sent to the next operation.
  it("drops selected paths a job removed when the file list refreshes", () => {
    const { result } = library();
    act(() => {
      result.current.setFiles([file("/media/a.mkv"), file("/media/b.mkv")]);
      result.current.setSelectedPaths(["/media/a.mkv", "/media/b.mkv"]);
    });

    act(() => {
      // The job consumed b.mkv and produced c.mkv.
      result.current.setFiles([file("/media/a.mkv"), file("/media/c.mkv")]);
    });

    expect(result.current.selectedPaths).toEqual(["/media/a.mkv"]);
  });

  /// A renamed file is still the file the user picked.
  it("follows a rename instead of dropping the selection", () => {
    const { result } = library();
    act(() => {
      result.current.setFiles([file("/media/old.mkv")]);
      result.current.setSelectedPaths(["/media/old.mkv"]);
    });

    act(() => {
      result.current.updateFilesAfterRename([
        { oldPath: "/media/old.mkv", newPath: "/media/new.mkv", newFileName: "new.mkv" }
      ]);
    });

    expect(result.current.selectedPaths).toEqual(["/media/new.mkv"]);
    expect(result.current.files[0].path).toBe("/media/new.mkv");
    expect(result.current.templateFilePath).toBe("/media/new.mkv");
  });

  /// Clearing the working set is a transient state on the way to restoring it
  /// from the backend; forgetting the selection there would defeat the point.
  it("keeps the selection when the file list is momentarily empty", () => {
    const { result } = library();
    act(() => {
      result.current.setFiles([file("/media/a.mkv")]);
      result.current.setSelectedPaths(["/media/a.mkv"]);
    });

    act(() => result.current.setFiles([]));
    expect(result.current.selectedPaths).toEqual(["/media/a.mkv"]);

    act(() => result.current.setFiles([file("/media/a.mkv")]));
    expect(result.current.selectedPaths).toEqual(["/media/a.mkv"]);
  });

  /// Windows reports the same file with different spellings depending on where
  /// the path came from, so reconciliation compares identity, not text.
  it("matches selected paths across separator and case differences", () => {
    const { result } = library();
    act(() => {
      result.current.setFiles([file(String.raw`C:\media\Show\a.mkv`)]);
      result.current.setSelectedPaths([String.raw`c:/media/show/a.mkv`]);
    });

    act(() => result.current.setFiles([file(String.raw`C:\media\Show\a.mkv`)]));
    expect(result.current.selectedPaths).toHaveLength(1);
  });

  /// Rust owns the working set, so a selection made in the UI has to reach it —
  /// otherwise an operation could run against a set the backend disagrees with.
  it("pushes every selection change to the backend", async () => {
    const setFileSelection = vi.fn().mockResolvedValue({
      updatedUtc: null,
      files: [],
      summary: { total: 0, mkv: 0, mp4: 0, failed: 0 },
      selectedPaths: ["/media/a.mkv"]
    });
    setBackendClient(createMockBackendClient({ setFileSelection }));

    const { result } = library();
    act(() => result.current.setFiles([file("/media/a.mkv")]));
    act(() => result.current.toggleSelectedPath("/media/a.mkv"));

    await waitFor(() => expect(setFileSelection).toHaveBeenCalledWith(["/media/a.mkv"]));
  });

  /// The backend is authoritative, so what it reports wins over whatever this
  /// tab happened to have cached.
  it("adopts the backend selection without echoing it back", async () => {
    const setFileSelection = vi.fn().mockResolvedValue({
      updatedUtc: null,
      files: [],
      summary: { total: 0, mkv: 0, mp4: 0, failed: 0 },
      selectedPaths: []
    });
    setBackendClient(createMockBackendClient({ setFileSelection }));

    const { result } = library();
    act(() => result.current.setFiles([file("/media/a.mkv"), file("/media/b.mkv")]));
    act(() => result.current.hydrateSelection(["/media/b.mkv"]));

    expect(result.current.selectedPaths).toEqual(["/media/b.mkv"]);
    expect(setFileSelection).not.toHaveBeenCalled();
  });

  it("toggles a path on and off", () => {
    const { result } = library();
    act(() => result.current.setFiles([file("/media/a.mkv")]));

    act(() => result.current.toggleSelectedPath("/media/a.mkv"));
    expect(result.current.selectedPaths).toEqual(["/media/a.mkv"]);

    act(() => result.current.toggleSelectedPath("/media/a.mkv"));
    expect(result.current.selectedPaths).toEqual([]);
  });
});

describe("adopting the backend working set", () => {
  /// Pages used to adopt the backend only while holding nothing, so the first
  /// scan won and every later change -- a rename, a property edit -- stayed
  /// invisible until the app restarted.
  it("adopts a set newer than the one it holds", () => {
    const { result } = library();

    act(() =>
      result.current.syncFromBackend({
        updatedUtc: "2026-08-09T10:00:00Z",
        files: [file("/media/before.mkv")],
        selectedPaths: []
      })
    );
    act(() =>
      result.current.syncFromBackend({
        updatedUtc: "2026-08-09T10:05:00Z",
        files: [file("/media/after.mkv")],
        selectedPaths: []
      })
    );

    expect(result.current.files.map((item) => item.fileName)).toEqual(["after.mkv"]);
  });

  /// A property edit leaves paths alone and changes what is inside them, which
  /// is the case a path comparison would miss.
  it("adopts a newer set even when the paths are unchanged", () => {
    const { result } = library();
    const edited = { ...file("/media/show.mkv"), audioSummary: "eng x2" };

    act(() =>
      result.current.syncFromBackend({
        updatedUtc: "2026-08-09T10:00:00Z",
        files: [file("/media/show.mkv")],
        selectedPaths: []
      })
    );
    act(() =>
      result.current.syncFromBackend({
        updatedUtc: "2026-08-09T10:05:00Z",
        files: [edited],
        selectedPaths: []
      })
    );

    expect(result.current.files[0].audioSummary).toBe("eng x2");
  });

  /// Refetches repeat the same answer constantly; re-adopting each time would
  /// discard local state for nothing.
  it("ignores a repeat of the set it already adopted", () => {
    const { result } = library();
    const stamp = "2026-08-09T10:00:00Z";

    act(() =>
      result.current.syncFromBackend({
        updatedUtc: stamp,
        files: [file("/media/first.mkv")],
        selectedPaths: []
      })
    );
    act(() =>
      result.current.syncFromBackend({
        updatedUtc: stamp,
        files: [file("/media/second.mkv")],
        selectedPaths: []
      })
    );

    expect(result.current.files.map((item) => item.fileName)).toEqual(["first.mkv"]);
  });

  /// An empty response is a cleared or not-yet-scanned host, not a reason to
  /// throw away what the page is showing.
  it("does not empty the library on an empty response", () => {
    const { result } = library();

    act(() =>
      result.current.syncFromBackend({
        updatedUtc: "2026-08-09T10:00:00Z",
        files: [file("/media/kept.mkv")],
        selectedPaths: []
      })
    );
    act(() =>
      result.current.syncFromBackend({
        updatedUtc: "2026-08-09T10:05:00Z",
        files: [],
        selectedPaths: []
      })
    );

    expect(result.current.files.map((item) => item.fileName)).toEqual(["kept.mkv"]);
  });
});
