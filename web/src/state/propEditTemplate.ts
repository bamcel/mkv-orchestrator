import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { loadPropEditTemplate } from "../api";
import type { MediaFileRow } from "../api";
import { useMediaLibrary } from "./MediaLibraryContext";

/**
 * Reading a template means asking the host to resolve every selected file, so
 * it costs about a second over a network share. Track Properties used to pay
 * that on arrival, every arrival, because it fetched through a mutation and so
 * cached nothing.
 *
 * Keeping it in the query cache means the page opens with its tracks already
 * there, and warming it as soon as a scan lands means the first visit is ready
 * too.
 */

/**
 * Identity of a template read.
 *
 * The file set is part of it, not just the template path, because the template
 * describes the layout the whole selection is edited against. Track state is
 * folded in as well, so a rescan that finds different tracks is a different
 * question rather than a stale hit.
 */
export function propEditTemplateKey(templatePath: string, files: MediaFileRow[]) {
  const signature = files
    .map((file) => `${file.path}|${file.tracks.map((track) => `${track.type}:${track.id}:${track.language}:${track.name}`).join(",")}`)
    .join("\u0000");
  return ["propedit-template", templatePath, signature] as const;
}

function templateQueryOptions(templatePath: string, files: MediaFileRow[]) {
  return {
    queryKey: propEditTemplateKey(templatePath, files),
    queryFn: () => loadPropEditTemplate({ files, templatePath }),
    // The key already carries everything the answer depends on, so a cached
    // entry cannot go stale on its own. Editing files on disk is the exception,
    // and that is invalidated explicitly.
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 30 * 60_000
  };
}

export function usePropEditTemplate(templatePath: string, files: MediaFileRow[]) {
  return useQuery({
    ...templateQueryOptions(templatePath, files),
    enabled: files.length > 0 && templatePath.length > 0
  });
}

/**
 * Warms the template for whatever the library is holding.
 *
 * Mounted beside the routes rather than inside the library provider: that
 * provider is plain state and mounts without a query client, which its own
 * tests rely on. This renders nothing and exists only for the effect.
 */
export function PropEditTemplateWarmer(): null {
  const { files, templateFilePath } = useMediaLibrary();
  const queryClient = useQueryClient();

  const templatePath =
    templateFilePath || files.find((file) => file.extension.toLowerCase() === ".mkv")?.path || "";
  const enabled = files.length > 0 && templatePath.length > 0;
  // `files` is rebuilt on every scan, so the derived key -- not the array
  // identity -- decides whether this is actually new work.
  const key = propEditTemplateKey(templatePath, files).join("");

  useEffect(() => {
    if (!enabled) return;
    void queryClient.prefetchQuery(templateQueryOptions(templatePath, files));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, key]);

  return null;
}

/**
 * Drops cached templates after an edit lands.
 *
 * An apply changes the files on disk while the rows already in hand still
 * describe how they used to look, so the key alone would not notice.
 */
export function useInvalidatePropEditTemplate() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: ["propedit-template"] });
}
