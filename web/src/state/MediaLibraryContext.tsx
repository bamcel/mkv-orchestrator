import { createContext, ReactNode, useContext, useEffect, useMemo, useState } from "react";
import { MediaFileRow } from "../api";

type MediaLibraryContextValue = {
  files: MediaFileRow[];
  setFiles: (files: MediaFileRow[]) => void;
  templateFilePath: string;
  setTemplateFilePath: (path: string) => void;
  updateFilesAfterRename: (renames: Array<{ oldPath: string; newPath: string; newFileName: string; status?: string }>) => void;
};

const MediaLibraryContext = createContext<MediaLibraryContextValue | null>(null);
const storageKey = "mkvo.web.scannedFiles";
const templateStorageKey = "mkvo.web.templateFilePath";

export function MediaLibraryProvider({ children }: { children: ReactNode }) {
  const [files, setFilesState] = useState<MediaFileRow[]>(() => {
    try {
      const value = sessionStorage.getItem(storageKey);
      return value ? JSON.parse(value) as MediaFileRow[] : [];
    } catch {
      return [];
    }
  });
  const [templateFilePath, setTemplateFilePath] = useState(() => {
    try {
      return sessionStorage.getItem(templateStorageKey) ?? "";
    } catch {
      return "";
    }
  });

  useEffect(() => {
    try {
      sessionStorage.setItem(storageKey, JSON.stringify(files));
    } catch {
      // Session storage is a convenience cache; scans still work without it.
    }
  }, [files]);

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
    },
    templateFilePath,
    setTemplateFilePath,
    updateFilesAfterRename: (renames) => {
      setFilesState((current) => current.map((file) => {
        const rename = renames.find((item) => item.oldPath.toLowerCase() === file.path.toLowerCase());
        if (!rename) return file;

        return {
          ...file,
          path: rename.newPath,
          fileName: rename.newFileName,
          status: rename.status ?? "Renamed"
        };
      }));
      setTemplateFilePath((current) => {
        const rename = renames.find((item) => item.oldPath.toLowerCase() === current.toLowerCase());
        return rename ? rename.newPath : current;
      });
    }
  }), [files, templateFilePath]);

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
