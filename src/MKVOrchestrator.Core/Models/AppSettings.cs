namespace MKVOrchestrator.Core.Models;

public sealed class AppSettings
{
    public const int CurrentSettingsSchemaVersion = 1;
    public int SettingsSchemaVersion { get; set; } = CurrentSettingsSchemaVersion;
    public string MkvToolNixDirectory { get; set; } = string.Empty;
    public string MkvMergePath { get; set; } = string.Empty; // legacy compatibility
    public string MkvPropEditPath { get; set; } = string.Empty; // legacy compatibility
    public string FfProbePath { get; set; } = string.Empty;
    public string RootFolderPath { get; set; } = string.Empty;
    public string TvdbApiKey { get; set; } = string.Empty;
    public string TvdbPin { get; set; } = string.Empty;
    public string TvdbLanguage { get; set; } = "eng";
    public string TvdbSeasonFilter { get; set; } = "All seasons + specials";
    public string TmdbApiKey { get; set; } = string.Empty;
    public string RenameLookupProvider { get; set; } = "TVDB";
    public string RenameTemplate { get; set; } = "{series} - S{season:00}E{episode:00} - {episodeTitle}";
    public List<string> RenameTemplates { get; set; } = new()
    {
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
    public WorkerSettings Workers { get; set; } = WorkerSettings.Defaults;
    public string SelectedThemeName { get; set; } = "Midnight";
    public List<ThemeDefinition> CustomThemes { get; set; } = new();
}

public sealed class ThemeDefinition
{
    public string Name { get; set; } = string.Empty;
    public Dictionary<string, string> Colors { get; set; } = new(StringComparer.OrdinalIgnoreCase);
}
