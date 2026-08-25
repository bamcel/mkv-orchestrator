import { screen } from "@testing-library/react";
import { Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it } from "vitest";
import { Layout } from "./Layout";
import { MediaLibraryProvider } from "../state/MediaLibraryContext";
import { renderWithBackend } from "../test/render";

beforeEach(() => {
  window.localStorage.clear();
  window.sessionStorage.clear();
});

describe("global operation status", () => {
  it("shows a running batch while another route is open", async () => {
    window.sessionStorage.setItem("mkvo.web.activeOperationJob", JSON.stringify({ id: "mux-job", label: "MKV Operations" }));
    renderWithBackend(
      <MediaLibraryProvider>
        <Routes>
          <Route element={<Layout />}>
            <Route path="*" element={<div>Track Properties route</div>} />
          </Route>
        </Routes>
      </MediaLibraryProvider>,
      {
        getStatus: () => Promise.resolve({ name: "MKVO", version: "0.1.0", mediaRoot: "/media", configRoot: "/config", sourceRoots: [], tools: [], contractVersion: 1 }),
        getOperationJob: () => Promise.resolve({
          id: "mux-job", kind: "Remux", status: "Running", createdUtc: "2026-08-25T20:00:00Z", startedUtc: "2026-08-25T20:00:01Z", completedUtc: null,
          completed: 40, failed: 1, skipped: 2, total: 283, currentFile: "Episode 44.mkv", currentFilePercent: 75,
          lines: [], muxResult: null, propEditResult: null, error: ""
        })
      }
    );

    expect(await screen.findByText(/MKV Operations: 43\/283.*Episode 44\.mkv 75%/i)).toBeInTheDocument();
    expect(screen.getByText("Track Properties route")).toBeInTheDocument();
  });
});
