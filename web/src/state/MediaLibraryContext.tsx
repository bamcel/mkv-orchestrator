import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { CurrentScanResponse, getCurrentScanFiles, MediaFileRow, setFileSelection } from "../api";

type MediaLibraryContextValue = {
  files: MediaFileRow[];
  setFiles: (files: MediaFileRow[]) => void;
  /** Replace the visible operation scope without replacing the backend scan. */
  setWorkingView: (files: MediaFileRow[], selectedPaths: string[], templatePath: string) => void;
  isWorkingView: boolean;
  /**
   * Paths the user has selected to operate on.
   *
   * Rust owns this: it is pushed to the backend on every change and hydrated
   * from `getCurrentScanFiles`, so it is the same on the desktop and in the
   * browser and cannot disagree with the working set an operation runs against.
   * The local copy is a cache that keeps the UI responsive.
   */
  selectedPaths: string[];
  selectionError: string | null;
  setSelectedPaths: (paths: string[]) => void;
  toggleSelectedPath: (path: string) => void;
  /** Adopt the selection the backend reports, without echoing it back. */
  hydrateSelection: (paths: string[]) => void;
  /**
   * Adopt the backend's working set when it is newer than what is held.
   *
   * Rust owns the working set and updates it as operations land, so pages read
   * it rather than guessing. They used to adopt it only while holding nothing,
   * which meant the very first scan won and every later change -- a rename, a
   * property edit -- was invisible until the app was restarted.
   */
  syncFromBackend: (scan: CurrentScanResponse) => void;
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

/**
 * Send the selection to the backend, which owns it.
 *
 * A rejected push is logged rather than surfaced: the backend rejects paths it
 * does not know about, and the next `getCurrentScanFiles` hydrates the UI back
 * to the authoritative answer anyway.
 */
export function MediaLibraryProvider({ children }: { children: ReactNode }) {
  const [files, setFilesState] = useState<MediaFileRow[]>(() =>
    readStored<MediaFileRow[]>(sessionStorage, storageKey, [])
  );
  const [selectedPaths, setSelectedPathsState] = useState<string[]>(() =>
    readStored<string[]>(localStorage, selectionStorageKey, [])
  );
  // What the adopted set was stamped with, so a repeat of the same answer does
  // not keep replacing local state on every refetch.
  const [adoptedUpdatedUtc, setAdoptedUpdatedUtc] = useState<string | null>(null);
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const selectionPush = useRef<Promise<void>>(Promise.resolve());
  const workingViewPaths = useRef<Set<string> | null>(null);
  const [isWorkingView, setIsWorkingView] = useState(false);
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

  const pushSelection = useCallback((paths: string[]): void => {
    selectionPush.current = selectionPush.current
      .catch(() => undefined)
      .then(async () => {
        try {
          const response = await setFileSelection(paths);
          setSelectedPathsState(response.selectedPaths);
          setSelectionError(null);
        } catch (error) {
          const message = error instanceof Error ? error.message : "The selection could not be saved.";
          setSelectionError(message);
          try {
            const authoritative = await getCurrentScanFiles();
            setSelectedPathsState(authoritative.selectedPaths);
          } catch {
            // Keep the visible error; the next successful refresh reconciles it.
          }
        }
      });
  }, []);

  const value = useMemo<MediaLibraryContextValue>(() => ({
    files,
    setFiles: (nextFiles) => {
      // A direct replacement is a new Dashboard scan or an explicit clear and
      // therefore ends a scoped Library handoff.
      workingViewPaths.current = null;
      setIsWorkingView(false);
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
    setWorkingView: (nextFiles, nextSelectedPaths, nextTemplatePath) => {
      workingViewPaths.current = new Set(nextFiles.map((file) => pathKey(file.path)));
      setIsWorkingView(true);
      setFilesState(nextFiles);
      setSelectedPathsState(nextSelectedPaths);
      setTemplateFilePath(nextTemplatePath || nextFiles[0]?.path || "");
      pushSelection(nextSelectedPaths);
    },
    isWorkingView,
    selectedPaths,
    selectionError,
    setSelectedPaths: (paths) => {
      setSelectedPathsState(paths);
      pushSelection(paths);
    },
    toggleSelectedPath: (path) => {
      setSelectedPathsState((current) => {
        const next = current.some((selected) => pathKey(selected) === pathKey(path))
          ? current.filter((selected) => pathKey(selected) !== pathKey(path))
          : [...current, path];
        pushSelection(next);
        return next;
      });
    },
    hydrateSelection: (paths) => setSelectedPathsState(paths),
    syncFromBackend: (scan) => {
      if (scan.updatedUtc && adoptedUpdatedUtc && scan.updatedUtc <= adoptedUpdatedUtc) {
        return;
      }

      setAdoptedUpdatedUtc(scan.updatedUtc ?? null);
      const viewPaths = workingViewPaths.current;
      const nextFiles = viewPaths
        ? scan.files.filter((file) => viewPaths.has(pathKey(file.path)))
        : scan.files;
      setFilesState(nextFiles);
      if (viewPaths) {
        const available = new Set(nextFiles.map((file) => pathKey(file.path)));
        setSelectedPathsState((current) => current.filter((path) => available.has(pathKey(path))));
      } else {
        setSelectedPathsState(scan.selectedPaths);
      }
      setTemplateFilePath((current) => {
        if (nextFiles.length === 0) return "";
        return current && nextFiles.some((file) => pathKey(file.path) === pathKey(current)) ? current : nextFiles[0].path;
      });
    },
    templateFilePath,
    setTemplateFilePath,
    updateFilesAfterRename: (renames) => {
      if (workingViewPaths.current) {
        workingViewPaths.current = new Set([...workingViewPaths.current].map((path) => {
          const rename = renames.find((item) => pathKey(item.oldPath) === path);
          return rename ? pathKey(rename.newPath) : path;
        }));
      }
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
  }), [files, selectedPaths, selectionError, templateFilePath, adoptedUpdatedUtc, isWorkingView, pushSelection]);

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
