import { createContext, ReactNode, useContext, useEffect, useMemo, useState } from "react";
import { MediaFileRow } from "../api";

type MediaLibraryContextValue = {
  files: MediaFileRow[];
  setFiles: (files: MediaFileRow[]) => void;
  /**
   * Paths the user has selected to operate on, shared by every page so a
   * selection made on the Dashboard carries into Mux/Remux and Track
   * Properties, and survives a reload.
   */
  selectedPaths: string[];
  setSelectedPaths: (paths: string[]) => void;
  toggleSelectedPath: (path: string) => void;
  templateFilePath: string;
  setTemplateFilePath: (path: string) => void;
  updateFilesAfterRename: (renames: Array<{ oldPath: string; newPath: string; newFileName: string; status?: string }>) => void;
};

const MediaLibraryContext = createContext<MediaLibraryContextValue | null>(null);
const storageKey = "mkvo.web.scannedFiles";
const templateStorageKey = "mkvo.web.templateFilePath";
// Selection outlives the tab: it is a deliberate user action, and losing it
// after a restart means re-picking files across a large library.
const selectionStorageKey = "mkvo.web.selectedPaths";

function readStored<T>(storage: Storage, key: string, fallback: T): T {
  try {
    const value = storage.getItem(key);
    return value ? (JSON.parse(value) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(storage: Storage, key: string, value: unknown): void {
  try {
    storage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage is a convenience cache; every workflow still works without it.
  }
}

function pathKey(path: string): string {
  return path.replace(/\\/g, "/").toLowerCase();
}

export function MediaLibraryProvider({ children }: { children: ReactNode }) {
  const [files, setFilesState] = useState<MediaFileRow[]>(() =>
    readStored<MediaFileRow[]>(sessionStorage, storageKey, [])
  );
  const [selectedPaths, setSelectedPathsState] = useState<string[]>(() =>
    readStored<string[]>(localStorage, selectionStorageKey, [])
  );
  const [templateFilePath, setTemplateFilePath] = useState(() => {
    try {
      return sessionStorage.getItem(templateStorageKey) ?? "";
    } catch {
      return "";
    }
  });

  useEffect(() => write(sessionStorage, storageKey, files), [files]);
  useEffect(() => write(localStorage, selectionStorageKey, selectedPaths), [selectedPaths]);

  useEffect(() => {
    try {
      sessionStorage.setItem(templateStorageKey, templateFilePath);
    } catch {
      // Session storage is a convenience cache; scans still work without it.
    }
  }, [templateFilePath]);

  const value = useMemo<MediaLibraryContextValue>(() => ({
    files,
    setFiles: (nextFiles) => {
      setFilesState(nextFiles);
      setTemplateFilePath((current) => {
        if (nextFiles.length === 0) return "";
        if (current && nextFiles.some((file) => file.path === current)) return current;
        return nextFiles[0].path;
      });
      // A job that remuxes, converts, or renames changes which paths exist.
      // Selections pointing at paths that are gone would silently be sent to
      // the next operation, so the selection is reconciled against the new
      // list rather than left stale. An empty list is a cleared working set,
      // not a reason to forget a selection that will be restored with it.
      if (nextFiles.length > 0) {
        const available = new Set(nextFiles.map((file) => pathKey(file.path)));
        setSelectedPathsState((current) =>
          current.filter((path) => available.has(pathKey(path)))
        );
      }
    },
    selectedPaths,
    setSelectedPaths: setSelectedPathsState,
    toggleSelectedPath: (path) => {
      setSelectedPathsState((current) =>
        current.some((selected) => pathKey(selected) === pathKey(path))
          ? current.filter((selected) => pathKey(selected) !== pathKey(path))
          : [...current, path]
      );
    },
    templateFilePath,
    setTemplateFilePath,
    updateFilesAfterRename: (renames) => {
      setFilesState((current) => current.map((file) => {
        const rename = renames.find((item) => pathKey(item.oldPath) === pathKey(file.path));
        if (!rename) return file;

        return {
          ...file,
          path: rename.newPath,
          fileName: rename.newFileName,
          status: rename.status ?? "Renamed"
        };
      }));
      // A renamed file is still the file the user picked, so the selection
      // follows it to its new path instead of being dropped.
      setSelectedPathsState((current) => current.map((path) => {
        const rename = renames.find((item) => pathKey(item.oldPath) === pathKey(path));
        return rename ? rename.newPath : path;
      }));
      setTemplateFilePath((current) => {
        const rename = renames.find((item) => pathKey(item.oldPath) === pathKey(current));
        return rename ? rename.newPath : current;
      });
    }
  }), [files, selectedPaths, templateFilePath]);

  return (
    <MediaLibraryContext.Provider value={value}>
      {children}
    </MediaLibraryContext.Provider>
  );
}

export function useMediaLibrary() {
  const context = useContext(MediaLibraryContext);
  if (!context) {
    throw new Error("useMediaLibrary must be used inside MediaLibraryProvider");
  }

  return context;
}
