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
      { id: "watch:/media/watch", name: "watch", paths: ["/media/watch"] },
      {
        id: "media:server-1:tv",
        name: "TV",
        paths: ["/media/tv"],
        serverId: "server-1",
        libraryName: "TV",
        mediaType: "tvshows"
      }
    ]);
  });

  it("deduplicates equivalent manual and server paths", () => {
    expect(librarySourceOptions(settings({ watchFolders: ["/media/tv/"] }))).toEqual([
      { id: "watch:/media/tv", name: "tv", paths: ["/media/tv/"] },
      {
        id: "media:server-1:tv",
        name: "TV",
        paths: ["/media/tv"],
        serverId: "server-1",
        libraryName: "TV",
        mediaType: "tvshows"
      }
    ]);
  });

  it("groups multiple paths for the same server library", () => {
    expect(librarySourceOptions(settings({
      mediaServers: [{
        ...settings().mediaServers[0],
        libraries: [
          { id: "manga-a", name: "Manga", type: "mixed", serverPath: "/anime/manga", containerPath: "/media/anime/manga", isEnabled: true },
          { id: "manga-b", name: "Manga", type: "mixed", serverPath: "/ereader/comics", containerPath: "/media/e-reader/comics", isEnabled: true }
        ]
      }]
    }))).toEqual([
      { id: "watch:/media/watch", name: "watch", paths: ["/media/watch"] },
      {
        id: "media:server-1:manga",
        name: "Manga",
        paths: ["/media/anime/manga", "/media/e-reader/comics"],
        serverId: "server-1",
        libraryName: "Manga",
        mediaType: "mixed"
      }
    ]);
  });
});
