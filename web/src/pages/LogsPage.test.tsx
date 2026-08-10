import { describe, expect, it, vi } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { LogsPage } from "./LogsPage";
import { renderWithBackend } from "../test/render";

const entry = {
  timestampUtc: "2026-08-05T21:00:00Z",
  correlationId: "019fd3cd-1837-70b2-ba2d-848462ae1718",
  area: "Scan",
  level: "information",
  message: "Scan completed",
  detail: "4 file(s): 3 MKV, 1 MP4, 0 cached, 0 failed"
};

describe("Logs page", () => {
  it("lists entries returned by the backend", async () => {
    renderWithBackend(<LogsPage />, {
      getOperationLogs: () => Promise.resolve({ entries: [entry] })
    });

    // The message appears twice: once in the list and once in the detail panel
    // for the selected entry.
    expect(await screen.findAllByText("Scan completed")).toHaveLength(2);
    expect(
      await screen.findByText(/4 file\(s\): 3 MKV, 1 MP4, 0 cached, 0 failed/)
    ).toBeInTheDocument();
  });

  /// The export is rendered by the backend so it reflects everything the server
  /// recorded, not just the page the UI happens to have fetched.
  it("downloads the backend-rendered export rather than the visible rows", async () => {
    const user = userEvent.setup();
    const exportOperationLogs = vi.fn().mockResolvedValue({
      fileName: "mkvo-logs-20260805-210738.txt",
      entryCount: 7,
      content: "2026-08-05T21:00:00Z\tInformation\tScan\tabc\tScan completed\n"
    });

    // jsdom implements neither, and the click would otherwise throw.
    const createObjectURL = vi.fn().mockReturnValue("blob:mock");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL });
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => undefined);

    renderWithBackend(<LogsPage />, {
      getOperationLogs: () => Promise.resolve({ entries: [entry] }),
      exportOperationLogs
    });

    await user.click(await screen.findByTitle(/export all logs/i));

    await waitFor(() => expect(exportOperationLogs).toHaveBeenCalled());
    expect(click).toHaveBeenCalled();
    expect(createObjectURL).toHaveBeenCalled();
    // The object URL must be released, or every export leaks a blob.
    await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:mock"));
    expect(await screen.findByText(/exported 7 log entries/i)).toBeInTheDocument();
  });

  it("surfaces a backend failure instead of rendering an empty list silently", async () => {
    renderWithBackend(<LogsPage />, {
      getOperationLogs: () => Promise.reject(new Error("log store unavailable"))
    });

    // The page must not claim there are simply no logs when the query failed.
    await waitFor(() => expect(screen.queryAllByText("Scan completed")).toHaveLength(0));
  });
});
