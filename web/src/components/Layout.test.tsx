import userEvent from "@testing-library/user-event";
import { fireEvent, screen, within } from "@testing-library/react";
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

it("shows a styled tooltip for collapsed sidebar links on hover and focus", async () => {
  window.localStorage.setItem("mkvo.sidebar.collapsed", "true");
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><Routes><Route element={<Layout />}><Route path="*" element={<div>Dashboard content</div>} /></Route></Routes></MediaLibraryProvider>, {
    getStatus: () => Promise.resolve({ name: "MKVO", version: "test", mediaRoot: "/media", configRoot: "/config", sourceRoots: [], tools: [], contractVersion: 1 })
  });

  const removeTracks = screen.getByRole("link", { name: "Remove Tracks" });
  expect(removeTracks).not.toHaveAttribute("title");
  await user.hover(removeTracks);
  expect(screen.getByRole("tooltip")).toHaveTextContent("Remove Tracks");
  await user.unhover(removeTracks);
  expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  fireEvent.focus(removeTracks);
  expect(screen.getByRole("tooltip")).toHaveTextContent("Remove Tracks");
});

it("mounts the sidebar toggle on the sidebar edge and updates its direction", async () => {
  window.localStorage.setItem("mkvo.sidebar.collapsed", "false");
  const user = userEvent.setup();
  renderWithBackend(<MediaLibraryProvider><Routes><Route element={<Layout />}><Route path="*" element={<div>Dashboard content</div>} /></Route></Routes></MediaLibraryProvider>, {
    getStatus: () => Promise.resolve({ name: "MKVO", version: "test", mediaRoot: "/media", configRoot: "/config", sourceRoots: [], tools: [], contractVersion: 1 })
  });

  const collapse = screen.getByRole("button", { name: "Collapse sidebar" });
  expect(collapse).toHaveClass("sidebar-collapse-button");
  expect(collapse).toHaveAttribute("data-tooltip", "Collapse sidebar");
  await user.click(collapse);
  const expand = screen.getByRole("button", { name: "Expand sidebar" });
  expect(expand).toHaveAttribute("data-tooltip", "Expand sidebar");
  expect(window.localStorage.getItem("mkvo.sidebar.collapsed")).toBe("true");
});
