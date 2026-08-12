import { describe, expect, it } from "vitest";
import type { WebSettings } from "../api";
import { librarySourceOptions } from "./LibraryPage";

function settings(overrides: Partial<WebSettings> = {}): WebSettings {
  return {
    hasTvdbApiKey: false, hasTvdbPin: false, hasTmdbApiKey: false, hasAnidbClient: false,
    tvdbLanguage: "eng", renameLookupProvider: "TVDB", renameTemplate: "{series}", renameTemplates: [],
    audioNamePresets: [], subtitleNamePresets: [], languagePresets: [], mkvMergeDefaultAudioLanguages: "eng,jpn",
    mkvMergeDefaultSubtitleLanguages: "eng", mkvToolNixDirectory: null, ffmpegDirectory: null,
    defaultRoot: "/media", defaultRootName: "Home",
    libraryRoots: [{ name: "Anime shortcut", path: "/media/anime" }],
    ignoredScanFolderNames: [], useQuickHashOnUnreliableTimestamps: false, renamePreviewCompactView: false,
    maxScanWorkers: 4, maxEditWorkers: 2, maxRemuxWorkers: 1, watchFolders: ["/media/watch"],
    enableLiveWatchFolderMonitoring: false, watchDebounceMillis: 750, watchReconciliationIntervalMinutes: 30,
    watchForcePolling: false, selectedThemeName: "Dark", customThemes: [], mediaServerPathMappings: [],
    mediaServers: [{
      id: "server-1", name: "Jellyfin", type: "Jellyfin", serverUrl: "http://server", hasApiKey: true,
      isDefault: true, lastSyncedUtc: null,
      libraries: [
        { id: "tv", name: "TV", type: "tvshows", serverPath: "/data/tv", containerPath: "/media/tv", isEnabled: true },
        { id: "movies", name: "Movies", type: "movies", serverPath: "/data/movies", containerPath: "/media/movies", isEnabled: false }
      ]
    }],
    ...overrides
  };
}

describe("Library sources", () => {
  it("uses manual watch folders and enabled server libraries, not Quick Access", () => {
    expect(librarySourceOptions(settings())).toEqual([
      { name: "watch", path: "/media/watch" },
      { name: "Jellyfin — TV", path: "/media/tv" }
    ]);
  });
});
