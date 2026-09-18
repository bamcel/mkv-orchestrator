import userEvent from "@testing-library/user-event";
import { screen, within } from "@testing-library/react";
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

    const [operationStatus] = await screen.findAllByText(/MKV Operations: 43\/283.*Episode 44\.mkv 75%/i);
    expect(operationStatus).toBeInTheDocument();
    expect(operationStatus).toHaveClass("break-words", "[overflow-wrap:anywhere]");
    expect(screen.getByText("Track Properties route")).toBeInTheDocument();
  });
});


it("navigates from the mobile menu and closes it after changing routes", async () => {
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><Routes><Route element={<Layout />}><Route path="*" element={<div>Dashboard content</div>} /><Route path="/subtitles" element={<div>Subtitle content</div>} /></Route></Routes></MediaLibraryProvider>, {
    getStatus: () => Promise.resolve({ name: "MKVO", version: "test", mediaRoot: "/media", configRoot: "/config", sourceRoots: [], tools: [], contractVersion: 1 })
  });
  await user.click(screen.getByRole("button", { name: "Menu" }));
  const menu = screen.getByRole("navigation", { name: "Mobile navigation" });
  await user.click(within(menu).getByRole("link", { name: "Subtitles" }));
  expect(await screen.findByText("Subtitle content")).toBeInTheDocument();
  expect(screen.queryByRole("navigation", { name: "Mobile navigation" })).not.toBeInTheDocument();

});
