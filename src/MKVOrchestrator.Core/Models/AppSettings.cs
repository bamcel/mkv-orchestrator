namespace MKVOrchestrator.Core.Models;

public sealed class AppSettings
{
    public const int CurrentSettingsSchemaVersion = 1;
    public int SettingsSchemaVersion { get; set; } = CurrentSettingsSchemaVersion;
    public string MkvToolNixDirectory { get; set; } = string.Empty;
    public string MkvMergePath { get; set; } = string.Empty; // legacy compatibility
    public string MkvPropEditPath { get; set; } = string.Empty; // legacy compatibility
    public string FfmpegDirectory { get; set; } = string.Empty;
    public string FfProbePath { get; set; } = string.Empty; // legacy compatibility
    public string RootFolderPath { get; set; } = string.Empty;
    public string TvdbApiKey { get; set; } = string.Empty;
    public string TvdbPin { get; set; } = string.Empty;
    public string TvdbLanguage { get; set; } = "eng";
    public string TvdbSeasonFilter { get; set; } = "All seasons + specials";
    public string TmdbApiKey { get; set; } = string.Empty;
    public string RenameLookupProvider { get; set; } = "TVDB";
    public string RenameTemplate { get; set; } = "{series} - S{season:00}E{episode:00} - {episodeTitle}";
    public bool RenamePreviewCompactView { get; set; }
    public List<string> RenameTemplates { get; set; } = new()
    {
        "{title}",
        "{title} ({year})",
        "{series} - S{season:00}E{episode:00} - {episodeTitle}",
        "{series} ({year}) - S{season:00}E{episode:00} - {episodeTitle}",
        "S{season:00}E{episode:00} - {episodeTitle}",
        "{series} - {absolute:000} - {episodeTitle}"
    };
    public string LastFolderPath { get; set; } = string.Empty; // legacy compatibility with v1.2/v1.3 settings
    public string SourceFolderStartMode { get; set; } = "Default root folder";
    public List<string> IgnoredScanFolderNames { get; set; } = new()
    {
        "Extras",
        "OVAs",
        "Backdrops",
        "Specials",
        "Trailers",
        "Trailer",
        "Featurettes",
        "Samples",
        "Sample"
    };
    public List<string> AudioNamePresets { get; set; } = new() { "English", "Japanese", "Commentary", "Director Commentary", "Signs & Songs" };
    public List<string> SubtitleNamePresets { get; set; } = new() { "English", "English Forced", "English SDH", "Signs & Songs", "Commentary" };
    public List<string> LanguagePresets { get; set; } = new() { "eng", "jpn", "spa", "fre", "ger", "und", "en", "ja", "es", "fr", "de" };
    public string MkvMergeDefaultAudioLanguages { get; set; } = "eng,jpn";
    public string MkvMergeDefaultSubtitleLanguages { get; set; } = "eng";
    public List<string> WatchFolders { get; set; } = new();
    public bool EnableLiveWatchFolderMonitoring { get; set; }
    public List<MediaServerSettings> MediaServers { get; set; } = new();
    public List<MediaServerPathMapping> MediaServerPathMappings { get; set; } = new();
    public WorkerSettings Workers { get; set; } = WorkerSettings.Defaults;
    public string SelectedThemeName { get; set; } = "Dark";
    public List<ThemeDefinition> CustomThemes { get; set; } = new();
}

public sealed class MediaServerSettings
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = string.Empty;
    public string Type { get; set; } = "Emby";
    public string ServerUrl { get; set; } = string.Empty;
    public string ApiKey { get; set; } = string.Empty;
    public bool IsDefault { get; set; }
    public List<MediaServerLibraryPath> Libraries { get; set; } = new();
    public DateTimeOffset? LastSyncedUtc { get; set; }
}

public sealed class MediaServerLibraryPath
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = string.Empty;
    public string Type { get; set; } = string.Empty;
    public string ServerPath { get; set; } = string.Empty;
    public string ContainerPath { get; set; } = string.Empty;
    public bool IsEnabled { get; set; } = true;
}

public sealed class MediaServerPathMapping
{
    public string ServerPathPrefix { get; set; } = string.Empty;
    public string ContainerPathPrefix { get; set; } = string.Empty;
}

public sealed class ThemeDefinition
{
    public string Name { get; set; } = string.Empty;
    public Dictionary<string, string> Colors { get; set; } = new(StringComparer.OrdinalIgnoreCase);
}
