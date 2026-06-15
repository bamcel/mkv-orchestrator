using System.Collections.Generic;
using System.Linq;
using System;
using System.Collections.ObjectModel;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.Input;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using MKVOrchestrator.App.Services;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{
    private static readonly JsonSerializerOptions ThemeJsonOptions = new()
    {
        WriteIndented = true
    };

    private void LoadSettings()
    {
        _isLoadingSettings = true;
        var settings = _settingsService.Load();
        _workerSettings = (settings.Workers ?? WorkerSettings.Defaults).CloneNormalized();
        MaxScanWorkers = _workerSettings.MaxScanWorkers;
        MaxEditWorkers = _workerSettings.MaxEditWorkers;
        MaxRemuxWorkers = _workerSettings.MaxRemuxWorkers;
        MkvToolNixDirectory = !string.IsNullOrWhiteSpace(settings.MkvToolNixDirectory)
            ? settings.MkvToolNixDirectory
            : InferMkvToolNixDirectory(settings.MkvMergePath, settings.MkvPropEditPath);
        FfProbePath = string.IsNullOrWhiteSpace(settings.FfProbePath) ? CrossPlatformRuntime.GetToolDisplayName("ffprobe.exe", "ffprobe") : settings.FfProbePath;
        TvdbApiKey = settings.TvdbApiKey ?? string.Empty;
        TvdbPin = settings.TvdbPin ?? string.Empty;
        TmdbApiKey = settings.TmdbApiKey ?? string.Empty;
        TvdbLanguage = string.IsNullOrWhiteSpace(settings.TvdbLanguage) ? "eng" : settings.TvdbLanguage.Trim();
        RenameLookupProvider = string.IsNullOrWhiteSpace(settings.RenameLookupProvider) ? "TVDB" : NormalizeLookupProvider(settings.RenameLookupProvider);
        RenameTemplate = string.IsNullOrWhiteSpace(settings.RenameTemplate) ? DefaultRenameTemplates()[0] : settings.RenameTemplate.Trim();
        ReplaceCollection(RenameTemplateOptions, BuildRenameTemplateList(settings.RenameTemplates, RenameTemplate));
        SelectedRenameTemplateOption = RenameTemplateOptions.FirstOrDefault(x => string.Equals(x, RenameTemplate, StringComparison.OrdinalIgnoreCase)) ?? RenameTemplateOptions.FirstOrDefault() ?? string.Empty;
        RootFolderPath = !string.IsNullOrWhiteSpace(settings.RootFolderPath)
            ? settings.RootFolderPath
            : settings.LastFolderPath ?? string.Empty;
        SourceFolderStartMode = NormalizeSourceFolderStartMode(settings.SourceFolderStartMode);
        MkvMergeDefaultAudioLanguages = NormalizeLanguageListText(settings.MkvMergeDefaultAudioLanguages, "eng,jpn");
        MkvMergeDefaultSubtitleLanguages = NormalizeLanguageListText(settings.MkvMergeDefaultSubtitleLanguages, "eng");
        MergeKeepAudioLanguages = MkvMergeDefaultAudioLanguages;
        MergeKeepSubtitleLanguages = MkvMergeDefaultSubtitleLanguages;
        WatchFolderText = string.Join(Environment.NewLine, settings.WatchFolders ?? new List<string>());
        EnableLiveWatchFolderMonitoring = settings.EnableLiveWatchFolderMonitoring;
        LoadThemeSettings(settings);
        _lastBrowseFolderPath = settings.LastFolderPath ?? string.Empty;

        // Startup performance: do not validate the preferred folder synchronously here.
        // UNC/network paths can block for several seconds during app launch.
        var preferredSourceFolder = GetPreferredSourceFolderPath();
        FolderPath = preferredSourceFolder ?? string.Empty;
        if (string.IsNullOrWhiteSpace(_lastBrowseFolderPath) && !string.IsNullOrWhiteSpace(FolderPath))
        {
            _lastBrowseFolderPath = FolderPath;
        }
        ReplaceCollection(AudioNamePresets, settings.AudioNamePresets.Count > 0 ? settings.AudioNamePresets : DefaultAudioNamePresets());
        ReplaceCollection(SubtitleNamePresets, settings.SubtitleNamePresets.Count > 0 ? settings.SubtitleNamePresets : DefaultSubtitleNamePresets());
        IgnoredScanFolderNameText = string.Join(Environment.NewLine,
            BuildIgnoredScanFolderList(settings.IgnoredScanFolderNames));
        ReplaceCollection(LanguagePresets, settings.LanguagePresets.Count > 0 ? settings.LanguagePresets : DefaultLanguagePresets());
        SyncPresetTextFromCollections();
        _isLoadingSettings = false;
        
        IsRenamePreviewDirty = false;
Log($"Settings loaded from {_settingsService.SettingsPath}");
        RefreshCacheLifecycleStatus();
        CacheStatusText = EnableLiveWatchFolderMonitoring
            ? "Live watcher pending startup initialization."
            : $"Live watcher disabled. Watch entries: {CacheWatchEntryCountText}. Temp entries: {CacheTempEntryCountText}.";
    }

    public async Task InitializeAfterUiReadyAsync()
    {
        // Let the main window render before touching watch folders or network-backed paths.
        await Task.Delay(150);

        RefreshLibraryAuditWatchFolderOptions();
        await EnsureWatchersInitializedAsync();
    }

    private string ResolveMkvToolPath(string windowsName, string unixName)
    {
        var directory = CrossPlatformRuntime.NormalizeUserPath(MkvToolNixDirectory);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            var candidate = Path.Combine(directory, CrossPlatformRuntime.IsWindows ? windowsName : unixName);
            if (File.Exists(candidate))
            {
                return candidate;
            }
        }

        return CrossPlatformRuntime.GetToolDisplayName(windowsName, unixName);
    }

    private static bool IsValidMkvToolNixDirectory(string directory)
    {
        directory = CrossPlatformRuntime.NormalizeUserPath(directory);
        if (string.IsNullOrWhiteSpace(directory) || !Directory.Exists(directory)) return false;

        var required = CrossPlatformRuntime.IsWindows
            ? new[] { "mkvmerge.exe", "mkvpropedit.exe", "mkvextract.exe", "mkvinfo.exe" }
            : new[] { "mkvmerge", "mkvpropedit", "mkvextract", "mkvinfo" };

        return required.All(tool => File.Exists(Path.Combine(directory, tool)));
    }

    private static IEnumerable<string> GetMkvToolNixSearchDirectories()
    {
        if (CrossPlatformRuntime.IsWindows)
        {
            yield return @"C:\Program Files\MKVToolNix";
            yield return @"C:\Program Files (x86)\MKVToolNix";
            var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            if (!string.IsNullOrWhiteSpace(localAppData)) yield return Path.Combine(localAppData, "Programs", "MKVToolNix");
        }
        else if (CrossPlatformRuntime.IsMacOS)
        {
            yield return "/Applications/MKVToolNix.app/Contents/MacOS";
            yield return "/opt/homebrew/bin";
            yield return "/usr/local/bin";
            yield return "/usr/bin";
        }
        else
        {
            yield return "/usr/bin";
            yield return "/usr/local/bin";
            yield return "/snap/bin";
            yield return "/app/bin";
        }

        var pathTool = CrossPlatformRuntime.FindExecutableOnPath(CrossPlatformRuntime.GetToolDisplayName("mkvmerge.exe", "mkvmerge"));
        var pathDirectory = string.IsNullOrWhiteSpace(pathTool) ? string.Empty : Path.GetDirectoryName(pathTool);
        if (!string.IsNullOrWhiteSpace(pathDirectory)) yield return pathDirectory;
    }

    private static string FormatByteSize(long bytes)
    {
        string[] units = { "B", "KB", "MB", "GB" };
        var value = (double)Math.Max(0, bytes);
        var unit = 0;
        while (value >= 1024 && unit < units.Length - 1)
        {
            value /= 1024;
            unit++;
        }

        return unit == 0 ? $"{bytes} {units[unit]}" : $"{value:0.0} {units[unit]}";
    }

    private long GetCacheDatabaseSizeBytes()
    {
        static long SizeOf(string path) => File.Exists(path) ? new FileInfo(path).Length : 0;
        return SizeOf(_scanner.Cache.DatabasePath) + SizeOf(_tempScanner.Cache.DatabasePath);
    }

    private void RefreshCacheLifecycleStatus()
    {
        CacheWatchEntryCountText = _mediaCache.CountEntries().ToString();
        CacheTempEntryCountText = _tempMediaCache.CountEntries().ToString();
        CacheDatabaseSizeText = FormatByteSize(GetCacheDatabaseSizeBytes());
    }

    private static string InferMkvToolNixDirectory(params string[] toolPaths)
    {
        foreach (var toolPath in toolPaths)
        {
            var normalized = CrossPlatformRuntime.NormalizeUserPath(toolPath);
            if (string.IsNullOrWhiteSpace(normalized))
            {
                continue;
            }

            if (Path.IsPathFullyQualified(normalized) || normalized.Contains(Path.DirectorySeparatorChar) || normalized.Contains(Path.AltDirectorySeparatorChar))
            {
                var folder = Path.GetDirectoryName(normalized);
                if (!string.IsNullOrWhiteSpace(folder) && Directory.Exists(folder))
                {
                    return folder;
                }
            }
        }

        return string.Empty;
    }

    private void SaveSettingsIfReady()
    {
        if (_isLoadingSettings) return;
        _settingsService.Save(new AppSettings
        {
            MkvToolNixDirectory = CrossPlatformRuntime.NormalizeUserPath(MkvToolNixDirectory),
            FfProbePath = string.IsNullOrWhiteSpace(FfProbePath) ? CrossPlatformRuntime.GetToolDisplayName("ffprobe.exe", "ffprobe") : CrossPlatformRuntime.NormalizeUserPath(FfProbePath),
            RootFolderPath = RootFolderPath?.Trim() ?? string.Empty,
            LastFolderPath = _lastBrowseFolderPath?.Trim() ?? string.Empty,
            SourceFolderStartMode = NormalizeSourceFolderStartMode(SourceFolderStartMode),
            TvdbApiKey = TvdbApiKey?.Trim() ?? string.Empty,
            TvdbPin = TvdbPin?.Trim() ?? string.Empty,
            TmdbApiKey = TmdbApiKey?.Trim() ?? string.Empty,
            TvdbLanguage = string.IsNullOrWhiteSpace(TvdbLanguage) ? "eng" : TvdbLanguage.Trim(),
            RenameLookupProvider = NormalizeLookupProvider(RenameLookupProvider),
            RenameTemplate = string.IsNullOrWhiteSpace(RenameTemplate) ? DefaultRenameTemplates()[0] : RenameTemplate.Trim(),
            RenameTemplates = BuildRenameTemplateList(RenameTemplateOptions, RenameTemplate).ToList(),
            IgnoredScanFolderNames = ParseIgnoredScanFolderNames(IgnoredScanFolderNameText).ToList(),
            AudioNamePresets = AudioNamePresets.ToList(),
            SubtitleNamePresets = SubtitleNamePresets.ToList(),
            LanguagePresets = LanguagePresets.ToList(),
            MkvMergeDefaultAudioLanguages = NormalizeLanguageListText(MkvMergeDefaultAudioLanguages, "eng,jpn"),
            MkvMergeDefaultSubtitleLanguages = NormalizeLanguageListText(MkvMergeDefaultSubtitleLanguages, "eng"),
            WatchFolders = ParseWatchFolderText(WatchFolderText).ToList(),
            EnableLiveWatchFolderMonitoring = EnableLiveWatchFolderMonitoring,
            Workers = _workerSettings.CloneNormalized(),
            SelectedThemeName = string.IsNullOrWhiteSpace(SelectedThemeName) ? "Midnight" : SelectedThemeName,
            CustomThemes = _customThemes.Select(ThemeService.Clone).ToList()
        });
    }

    partial void OnSelectedThemeNameChanged(string value)
    {
        if (_isLoadingSettings) return;
        LoadSelectedThemeIntoEditor();
        SaveSettingsIfReady();
    }

    [RelayCommand]
    private void ReloadTheme()
    {
        try
        {
            var theme = ParseThemeEditor();
            if (string.IsNullOrWhiteSpace(theme.Name))
            {
                theme.Name = string.IsNullOrWhiteSpace(SelectedThemeName) ? "Custom Theme" : SelectedThemeName;
            }

            ThemeService.Apply(theme);
            if (ThemeOptions.Any(option => string.Equals(option, theme.Name, StringComparison.OrdinalIgnoreCase)))
            {
                SelectedThemeName = ThemeOptions.First(option => string.Equals(option, theme.Name, StringComparison.OrdinalIgnoreCase));
                ThemeStatusText = $"Theme reloaded: {theme.Name}";
            }
            else
            {
                ThemeStatusText = $"Theme reloaded from editor: {theme.Name}. Save it as a custom theme to keep it.";
            }

            SaveSettingsIfReady();
        }
        catch (Exception ex)
        {
            ThemeStatusText = $"Theme reload failed: {ex.Message}";
            Log(ThemeStatusText);
        }
    }

    [RelayCommand]
    private void SaveCustomTheme()
    {
        try
        {
            var theme = ParseThemeEditor();
            var name = string.IsNullOrWhiteSpace(CustomThemeName) ? theme.Name : CustomThemeName.Trim();
            if (string.IsNullOrWhiteSpace(name))
            {
                ThemeStatusText = "Enter a custom theme name before saving.";
                return;
            }

            if (ThemeService.IsBuiltInTheme(name))
            {
                ThemeStatusText = "Built-in theme names cannot be overwritten. Enter a new custom name.";
                return;
            }

            theme.Name = name;
            var existing = _customThemes.FindIndex(t => string.Equals(t.Name, name, StringComparison.OrdinalIgnoreCase));
            if (existing >= 0) _customThemes[existing] = theme;
            else _customThemes.Add(theme);

            RefreshThemeOptions(name);
            ThemeEditorText = SerializeTheme(theme);
            ThemeService.Apply(theme);
            ThemeStatusText = $"Saved and applied custom theme: {name}";
            SaveSettingsIfReady();
        }
        catch (Exception ex)
        {
            ThemeStatusText = $"Save custom theme failed: {ex.Message}";
            Log(ThemeStatusText);
        }
    }

    [RelayCommand]
    private void RemoveCustomTheme()
    {
        var selected = SelectedThemeName?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(selected) || ThemeService.IsBuiltInTheme(selected))
        {
            ThemeStatusText = "Only custom themes can be removed.";
            return;
        }

        var removed = _customThemes.RemoveAll(t => string.Equals(t.Name, selected, StringComparison.OrdinalIgnoreCase));
        if (removed == 0)
        {
            ThemeStatusText = "Selected custom theme was not found.";
            return;
        }

        RefreshThemeOptions("Midnight");
        LoadSelectedThemeIntoEditor();
        ThemeService.Apply(ThemeService.GetTheme(SelectedThemeName, _customThemes));
        ThemeStatusText = $"Removed custom theme: {selected}";
        SaveSettingsIfReady();
    }

    private void LoadThemeSettings(AppSettings settings)
    {
        _customThemes = (settings.CustomThemes ?? new List<ThemeDefinition>())
            .Where(theme => !string.IsNullOrWhiteSpace(theme.Name) && !ThemeService.IsBuiltInTheme(theme.Name))
            .Select(ThemeService.Clone)
            .ToList();

        RefreshThemeOptions(string.IsNullOrWhiteSpace(settings.SelectedThemeName) ? "Midnight" : settings.SelectedThemeName);
        LoadSelectedThemeIntoEditor();
        ThemeService.Apply(ThemeService.GetTheme(SelectedThemeName, _customThemes));
    }

    private void RefreshThemeOptions(string preferredTheme)
    {
        ThemeOptions.Clear();
        foreach (var theme in ThemeService.BuiltInThemes)
        {
            ThemeOptions.Add(theme.Name);
        }

        foreach (var theme in _customThemes.OrderBy(t => t.Name, StringComparer.OrdinalIgnoreCase))
        {
            ThemeOptions.Add(theme.Name);
        }

        SelectedThemeName = ThemeOptions.FirstOrDefault(t => string.Equals(t, preferredTheme, StringComparison.OrdinalIgnoreCase))
            ?? ThemeOptions.FirstOrDefault()
            ?? "Midnight";
    }

    private void LoadSelectedThemeIntoEditor()
    {
        var theme = ThemeService.GetTheme(SelectedThemeName, _customThemes);
        ThemeEditorText = SerializeTheme(theme);
        CustomThemeName = ThemeService.IsBuiltInTheme(theme.Name) ? theme.Name + " Custom" : theme.Name;
        ThemeStatusText = ThemeService.IsBuiltInTheme(theme.Name)
            ? "Built-in theme loaded. Edit JSON and save as a custom theme to keep changes."
            : "Custom theme loaded. Edit JSON, save, then reload to apply changes.";
    }

    private ThemeDefinition ParseThemeEditor()
    {
        var theme = JsonSerializer.Deserialize<ThemeDefinition>(ThemeEditorText, ThemeJsonOptions)
            ?? throw new InvalidOperationException("Theme JSON is empty.");
        if (theme.Colors.Count == 0)
        {
            throw new InvalidOperationException("Theme JSON must include a Colors object.");
        }

        foreach (var color in theme.Colors)
        {
            if (!Color.TryParse(color.Value, out _))
            {
                throw new InvalidOperationException($"{color.Key} is not a valid color value.");
            }
        }

        return theme;
    }

    private static string SerializeTheme(ThemeDefinition theme)
        => JsonSerializer.Serialize(ThemeService.Clone(theme), ThemeJsonOptions);

    private void ApplyWorkerSettings()
    {
        _workerSettings = new WorkerSettings
        {
            MaxScanWorkers = MaxScanWorkers,
            MaxEditWorkers = MaxEditWorkers,
            MaxRemuxWorkers = MaxRemuxWorkers
        }.Normalize();

        MaxScanWorkers = _workerSettings.MaxScanWorkers;
        MaxEditWorkers = _workerSettings.MaxEditWorkers;
        MaxRemuxWorkers = _workerSettings.MaxRemuxWorkers;
        SaveSettingsIfReady();
    }



    [RelayCommand]
    private void SaveRenameTemplate()
    {
        var clean = NormalizeRenameTemplate(RenameTemplate);
        if (string.IsNullOrWhiteSpace(clean))
        {
            return;
        }

        var existing = RenameTemplateOptions.FirstOrDefault(x => string.Equals(x, clean, StringComparison.OrdinalIgnoreCase));
        if (string.IsNullOrWhiteSpace(existing))
        {
            RenameTemplateOptions.Add(clean);
            SelectedRenameTemplateOption = clean;
            Log($"Added rename template: {clean}");
        }
        else
        {
            SelectedRenameTemplateOption = existing;
            Log($"Rename template already exists: {existing}");
        }

        RenameTemplate = clean;
        SaveSettingsIfReady();
    }

    [RelayCommand]
    private void RemoveRenameTemplate()
    {
        var selected = NormalizeRenameTemplate(SelectedRenameTemplateOption);
        if (string.IsNullOrWhiteSpace(selected))
        {
            selected = NormalizeRenameTemplate(RenameTemplate);
        }

        var existing = RenameTemplateOptions.FirstOrDefault(x => string.Equals(x, selected, StringComparison.OrdinalIgnoreCase));
        if (string.IsNullOrWhiteSpace(existing))
        {
            return;
        }

        RenameTemplateOptions.Remove(existing);
        Log($"Removed rename template: {existing}");

        if (RenameTemplateOptions.Count == 0)
        {
            foreach (var template in DefaultRenameTemplates())
            {
                RenameTemplateOptions.Add(template);
            }
        }

        SelectedRenameTemplateOption = RenameTemplateOptions.FirstOrDefault() ?? string.Empty;
        RenameTemplate = SelectedRenameTemplateOption;
        SaveSettingsIfReady();
    }

    [RelayCommand]
    private void RestoreDefaultRenameTemplates()
    {
        ReplaceCollection(RenameTemplateOptions, DefaultRenameTemplates());
        SelectedRenameTemplateOption = RenameTemplateOptions.FirstOrDefault() ?? string.Empty;
        RenameTemplate = SelectedRenameTemplateOption;
        Log("Restored default rename templates.");
        SaveSettingsIfReady();
    }


    private static IEnumerable<string> ParseWatchFolderText(string? text)
    {
        return (text ?? string.Empty)
            .Split(new[] { '\r', '\n', '|' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(CrossPlatformRuntime.NormalizeUserPath)
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Distinct(CrossPlatformRuntime.PathComparer);
    }

    private static string NormalizeLanguageListText(string? value, string fallback)
    {
        var normalized = string.Join(",", (value ?? string.Empty)
            .Split(new[] { '\r', '\n', ',', ';', '|', ' ' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(x => !string.IsNullOrWhiteSpace(x))
            .Distinct(StringComparer.OrdinalIgnoreCase));

        return string.IsNullOrWhiteSpace(normalized) ? fallback : normalized;
    }

    private string GetPreferredSourceFolderPath()
    {
        return NormalizeSourceFolderStartMode(SourceFolderStartMode) == "Remember last directory"
            ? GetBestExistingFolder(_lastBrowseFolderPath, FolderPath, RootFolderPath)
            : GetBestExistingFolder(RootFolderPath, _lastBrowseFolderPath, FolderPath);
    }

    private static string NormalizeSourceFolderStartMode(string? value)
    {
        return string.Equals(value, "Remember last directory", StringComparison.OrdinalIgnoreCase)
            ? "Remember last directory"
            : "Default root folder";
    }

    private static string NormalizeIgnoredScanFolderText(string? text)
    {
        return string.Join(Environment.NewLine, ParseIgnoredScanFolderNames(text));
    }

    private static IEnumerable<string> DefaultIgnoredScanFolderNames()
    {
        return new[] { "Extras", "OVAs", "Backdrops", "Specials", "Trailers", "Trailer", "Featurettes", "Samples", "Sample" };
    }

    private static IEnumerable<string> BuildIgnoredScanFolderList(IEnumerable<string>? configuredNames)
    {
        var configured = configuredNames?.ToList() ?? new List<string>();
        var names = configured.Count > 0
            ? configured.Concat(DefaultIgnoredScanFolderNames())
            : DefaultIgnoredScanFolderNames();

        return names
            .Where(name => !string.IsNullOrWhiteSpace(name))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(name => name, StringComparer.OrdinalIgnoreCase);
    }

    private static IEnumerable<string> ParseIgnoredScanFolderNames(string? text)
    {
        return (text ?? string.Empty)
            .Split(new[] { '\r', '\n', ',', ';', '|' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(name => !string.IsNullOrWhiteSpace(name))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(name => name, StringComparer.OrdinalIgnoreCase);
    }


    private static string GetBestExistingFolder(params string?[] candidates)
    {
        foreach (var candidate in candidates)
        {
            if (!string.IsNullOrWhiteSpace(candidate) && Directory.Exists(candidate))
            {
                return candidate;
            }
        }

        return string.Empty;
    }

    private static List<string> DefaultRenameTemplates() => new()
    {
        "{series} - S{season:00}E{episode:00} - {episodeTitle}",
        "{series} ({year}) - S{season:00}E{episode:00} - {episodeTitle}",
        "S{season:00}E{episode:00} - {episodeTitle}",
        "{series} - {absolute:000} - {episodeTitle}"
    };

    private static string NormalizeRenameTemplate(string? template) => (template ?? string.Empty).Trim();

    private static IEnumerable<string> BuildRenameTemplateList(IEnumerable<string>? templates, string? activeTemplate = null)
    {
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var template in templates ?? Enumerable.Empty<string>())
        {
            var clean = NormalizeRenameTemplate(template);
            if (!string.IsNullOrWhiteSpace(clean) && seen.Add(clean))
            {
                yield return clean;
            }
        }

        var active = NormalizeRenameTemplate(activeTemplate);
        if (!string.IsNullOrWhiteSpace(active) && seen.Add(active))
        {
            yield return active;
        }

        if (seen.Count == 0)
        {
            foreach (var template in DefaultRenameTemplates())
            {
                if (seen.Add(template)) yield return template;
            }
        }
    }

    private void RefreshTrackPresetOptions()
    {
        foreach (var track in PropEditAudioTracks) ApplyPresetOptions(track, AudioNamePresets, LanguagePresets);
        foreach (var track in PropEditSubtitleTracks) ApplyPresetOptions(track, SubtitleNamePresets, LanguagePresets);
    }

    private static void ApplyPresetOptions(PropEditTrackConfig item, IEnumerable<string> namePresets, IEnumerable<string> languagePresets)
    {
        item.NameOptions.Clear();
        item.LanguageOptions.Clear();

        foreach (var value in BuildOptions(namePresets, item.EditedName, item.CurrentName))
        {
            item.NameOptions.Add(value);
        }

        if (!string.IsNullOrWhiteSpace(item.EditedName)
            && item.NameOptions.Any(v => string.Equals(v, item.EditedName, StringComparison.OrdinalIgnoreCase)))
        {
            item.SelectedNamePreset = item.NameOptions.First(v => string.Equals(v, item.EditedName, StringComparison.OrdinalIgnoreCase));
        }
        else
        {
            item.SelectedNamePreset = string.Empty;
        }

        foreach (var value in BuildOptions(languagePresets, item.EditedLanguage, item.CurrentLanguage))
        {
            item.LanguageOptions.Add(value);
        }
    }

    private static IEnumerable<string> BuildOptions(IEnumerable<string> configuredValues, params string?[] priorityValues)
    {
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var value in priorityValues.Concat(configuredValues))
        {
            var clean = value?.Trim();
            if (string.IsNullOrWhiteSpace(clean)) continue;
            if (seen.Add(clean)) yield return clean;
        }
    }

    private void SyncPresetTextFromCollections()
    {
        AudioNamePresetText = string.Join(Environment.NewLine, AudioNamePresets);
        SubtitleNamePresetText = string.Join(Environment.NewLine, SubtitleNamePresets);
        LanguagePresetText = string.Join(Environment.NewLine, LanguagePresets);
    }

    private static List<string> ParsePresetText(string text)
    {
        return text
            .Split(new[] { '\r', '\n', ',', ';' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(x => !string.IsNullOrWhiteSpace(x))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private static void ReplaceCollection(ObservableCollection<string> collection, IEnumerable<string> values)
    {
        collection.Clear();
        foreach (var value in values.Where(v => !string.IsNullOrWhiteSpace(v)).Distinct(StringComparer.OrdinalIgnoreCase))
        {
            collection.Add(value.Trim());
        }
    }

    private static List<string> DefaultAudioNamePresets() => new()
    {
        "English",
        "Japanese",
        "English Commentary",
        "Japanese Commentary",
        "Director Commentary"
    };

    private static List<string> DefaultSubtitleNamePresets() => new()
    {
        "English",
        "English Forced",
        "English SDH",
        "Signs & Songs",
        "Commentary"
    };

    private static List<string> DefaultLanguagePresets() => new()
    {
        "eng",
        "jpn",
        "spa",
        "fre",
        "ger",
        "ita",
        "por",
        "kor",
        "chi",
        "und",
        "en",
        "ja",
        "es",
        "fr",
        "de"
    };

    private sealed class PropEditPlan
    {
        public List<PlannedAction> Actions { get; } = new();
        public List<PropEditSkippedFile> SkippedFiles { get; } = new();
        public List<PropEditNoChangeFile> NoChangeFiles { get; } = new();
    }

    private sealed record PropEditSkippedFile(string FilePath, string Reason);

    private sealed record PropEditNoChangeFile(string FilePath, string Reason);
}
