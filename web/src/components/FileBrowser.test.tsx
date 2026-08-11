import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { FileBrowser } from "./FileBrowser";
import { renderWithBackend } from "../test/render";
import type { FileSystemEntry, FileSystemResponse } from "../api";

function folder(name: string, path: string, modified = "2026-08-01T10:00:00Z"): FileSystemEntry {
  return { name, path, kind: "folder", sizeBytes: null, modifiedUtc: modified };
}

function file(
  name: string,
  path: string,
  sizeBytes: number,
  modified = "2026-08-02T10:00:00Z"
): FileSystemEntry {
  return { name, path, kind: "file", sizeBytes, modifiedUtc: modified };
}

/** A stub filesystem keyed by path, including the volume list at "". */
function filesystem(tree: Record<string, FileSystemResponse>) {
  return vi.fn((path?: string) => {
    const key = path ?? "";
    const listing = tree[key];
    return listing
      ? Promise.resolve(listing)
      : Promise.reject(new Error(`no such directory: ${key}`));
  });
}

const volumeList: FileSystemResponse = {
  path: "",
  parentPath: null,
  entries: [folder("C:", "C:\\"), folder("D:", "D:\\")]
};

describe("file browser navigation", () => {
  beforeEach(() => window.localStorage.clear());

  /// The desktop was previously stuck inside one mounted directory. Reaching a
  /// library on another drive is the whole point of the change.
  it("navigates to another volume from the sidebar", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": { path: "C:\\media", parentPath: "C:\\", entries: [] },
      "D:\\": { path: "D:\\", parentPath: "", entries: [folder("Shows", "D:\\Shows")] }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    const sidebar = await screen.findByRole("navigation");
    await user.click(await within(sidebar).findByRole("button", { name: /D:/ }));

    expect(await screen.findByText("Shows")).toBeInTheDocument();
  });

  /// Windows has no single filesystem root, so "up" from a drive is the volume
  /// list rather than a directory.
  it("goes up from a drive root to the volume list", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\": { path: "C:\\", parentPath: "", entries: [folder("media", "C:\\media")] }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    await screen.findByText("media");
    await user.click(screen.getByRole("button", { name: /up one level/i }));

    // The volume list renders the drives as rows, not just sidebar entries.
    await waitFor(() => expect(screen.getAllByText("C:").length).toBeGreaterThan(0));
  });

  it("opens a folder on double click and walks back with the crumb", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": { path: "C:\\media", parentPath: "C:\\", entries: [folder("Show", "C:\\media\\Show")] },
      "C:\\media\\Show": {
        path: "C:\\media\\Show",
        parentPath: "C:\\media",
        entries: [file("Ep01.mkv", "C:\\media\\Show\\Ep01.mkv", 2048)]
      }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    await user.dblClick(await screen.findByText("Show"));
    expect(await screen.findByText("Ep01.mkv")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "media" }));
    expect(await screen.findByText("Show")).toBeInTheDocument();
  });

  it("returns the current folder when nothing is picked", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": { path: "C:\\media", parentPath: "C:\\", entries: [folder("Show", "C:\\media\\Show")] }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={onSelect} />,
      { browseFileSystem }
    );

    await screen.findByText("Show");
    await user.click(screen.getByRole("button", { name: /select this folder/i }));

    expect(onSelect).toHaveBeenCalledWith("C:\\media", "folder");
  });

  it("returns a picked file rather than the folder holding it", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": {
        path: "C:\\media",
        parentPath: "C:\\",
        entries: [file("Ep01.mkv", "C:\\media\\Ep01.mkv", 2048)]
      }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={onSelect} />,
      { browseFileSystem }
    );

    await user.click(await screen.findByText("Ep01.mkv"));
    await user.click(screen.getByRole("button", { name: /select file/i }));

    expect(onSelect).toHaveBeenCalledWith("C:\\media\\Ep01.mkv", "file");
  });

  it("offers pin, open, and select actions when a folder is right-clicked", async () => {
    const user = userEvent.setup();
    const onPinToQuickAccess = vi.fn();
    const onSelect = vi.fn();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": {
        path: "C:\\media",
        parentPath: "C:\\",
        entries: [folder("Show", "C:\\media\\Show")]
      },
      "C:\\media\\Show": {
        path: "C:\\media\\Show",
        parentPath: "C:\\media",
        entries: [folder("Season 01", "C:\\media\\Show\\Season 01")]
      }
    });

    renderWithBackend(
      <FileBrowser
        initialPath="C:\media"
        roots={[]}
        onCancel={() => {}}
        onSelect={onSelect}
        onPinToQuickAccess={onPinToQuickAccess}
      />,
      { browseFileSystem }
    );

    const showRow = (await screen.findByText("Show")).closest("tr")!;
    fireEvent.contextMenu(showRow, { clientX: 120, clientY: 140 });
    expect(screen.getByRole("menuitem", { name: /pin to quick access/i })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /open folder/i })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /select folder/i })).toBeInTheDocument();

    await user.click(screen.getByRole("menuitem", { name: /pin to quick access/i }));
    expect(onPinToQuickAccess).toHaveBeenCalledWith("C:\\media\\Show", "Show");

    fireEvent.contextMenu(showRow, { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: /open folder/i }));
    const seasonRow = (await screen.findByText("Season 01")).closest("tr")!;

    fireEvent.contextMenu(seasonRow, { clientX: 120, clientY: 140 });
    await user.click(screen.getByRole("menuitem", { name: /select folder/i }));
    expect(onSelect).toHaveBeenCalledWith("C:\\media\\Show\\Season 01", "folder");
  });

  /// Folders lead regardless of sort, the way a file manager orders them.
  it("keeps folders above files when sorting by size", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": {
        path: "C:\\media",
        parentPath: "C:\\",
        entries: [
          file("big.mkv", "C:\\media\\big.mkv", 9_000_000),
          folder("Show", "C:\\media\\Show"),
          file("small.mkv", "C:\\media\\small.mkv", 10)
        ]
      }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    await screen.findByText("Show");
    await user.click(screen.getByRole("button", { name: /^Size/ }));

    const rows = screen.getAllByRole("row").slice(1);
    expect(within(rows[0]).getByText("Show")).toBeInTheDocument();
  });

  it("filters the current folder without navigating", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": {
        path: "C:\\media",
        parentPath: "C:\\",
        entries: [folder("Anime", "C:\\media\\Anime"), folder("Movies", "C:\\media\\Movies")]
      }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    await screen.findByText("Anime");
    await user.type(screen.getByLabelText(/filter this folder/i), "mov");

    expect(screen.queryByText("Anime")).not.toBeInTheDocument();
    expect(screen.getByText("Movies")).toBeInTheDocument();
  });

  /// A NAS share is reachable by typing its path, which is how a location that
  /// no drive letter points at gets into the browser in the first place.
  it("navigates to a UNC path typed into the address bar", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "C:\\media": { path: "C:\\media", parentPath: "C:\\", entries: [] },
      "\\\\nas\\downloads": {
        path: "\\\\nas\\downloads",
        parentPath: "\\\\nas",
        entries: [folder("completed", "\\\\nas\\downloads\\completed")]
      }
    });

    renderWithBackend(
      <FileBrowser initialPath="C:\media" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    await user.click(await screen.findByRole("button", { name: "Address bar" }));
    const address = screen.getByLabelText("Path");
    await user.clear(address);
    await user.type(address, "\\\\nas\\downloads{Enter}");

    expect(await screen.findByText("completed")).toBeInTheDocument();
  });

  /// Enumerating hosts on a network is slow and unreliable, so a server the
  /// user has actually reached is remembered instead.
  it("remembers a reached server under Network and navigates back to it", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "\\\\nas": {
        path: "\\\\nas",
        parentPath: "",
        entries: [folder("downloads", "\\\\nas\\downloads")]
      },
      "\\\\nas\\downloads": {
        path: "\\\\nas\\downloads",
        parentPath: "\\\\nas",
        entries: [folder("completed", "\\\\nas\\downloads\\completed")]
      }
    });

    renderWithBackend(
      <FileBrowser
        initialPath="\\nas\downloads"
        roots={[]}
        onCancel={() => {}}
        onSelect={() => {}}
      />,
      { browseFileSystem }
    );

    const sidebar = await screen.findByRole("navigation");
    const remembered = await within(sidebar).findByRole("button", { name: /^nas$/ });

    // The share list is a level the browser can only reach by enumeration,
    // since a bare server is not a directory.
    await user.click(remembered);
    expect(await screen.findByText("downloads")).toBeInTheDocument();
  });

  /// A share carries no timestamp, which arrives as the epoch. Rendering that
  /// literally would date every share to 1969.
  it("leaves the date blank for a share rather than showing the epoch", async () => {
    const browseFileSystem = filesystem({
      "": volumeList,
      "\\\\nas": {
        path: "\\\\nas",
        parentPath: "",
        entries: [folder("downloads", "\\\\nas\\downloads", "1970-01-01T00:00:00Z")]
      }
    });

    renderWithBackend(
      <FileBrowser initialPath="\\nas" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    const row = (await screen.findByText("downloads")).closest("tr");
    expect(row).not.toBeNull();
    expect(row!.textContent).not.toMatch(/19(69|70)/);
  });

  it("forgets a network location on request", async () => {
    const user = userEvent.setup();
    const browseFileSystem = filesystem({
      "": volumeList,
      "\\\\nas\\downloads": { path: "\\\\nas\\downloads", parentPath: "\\\\nas", entries: [] }
    });

    renderWithBackend(
      <FileBrowser
        initialPath="\\nas\downloads"
        roots={[]}
        onCancel={() => {}}
        onSelect={() => {}}
      />,
      { browseFileSystem }
    );

    const sidebar = await screen.findByRole("navigation");
    await within(sidebar).findByRole("button", { name: /^nas$/ });
    await user.click(within(sidebar).getByRole("button", { name: /forget/i }));

    await waitFor(() =>
      expect(within(sidebar).queryByRole("button", { name: /^nas$/ })).not.toBeInTheDocument()
    );
  });

  /// An unreadable folder is ordinary — a permission-denied system directory is
  /// one click away when browsing is unrestricted.
  it("reports a folder it cannot read instead of showing it as empty", async () => {
    const browseFileSystem = filesystem({ "": volumeList });

    renderWithBackend(
      <FileBrowser initialPath="C:\forbidden" roots={[]} onCancel={() => {}} onSelect={() => {}} />,
      { browseFileSystem }
    );

    expect(await screen.findByText(/no such directory/i)).toBeInTheDocument();
  });
});
