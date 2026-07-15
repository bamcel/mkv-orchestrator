using System.Collections.Generic;
using System.Linq;
using System;
using System.Collections.ObjectModel;
using System.Text;
using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.Input;
using Avalonia.Controls;
using Avalonia.Platform.Storage;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Pipeline;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{

    [RelayCommand]
    private void ToggleSummaryExpanded()
    {
        IsSummaryExpanded = !IsSummaryExpanded;
    }

    partial void OnSelectedFileChanged(MkvFileItem? value)
    {
        UseSelectedFileAsTemplateCommand.NotifyCanExecuteChanged();
        // DataGrid.SelectedItem can briefly become null when a tab unloads/reloads.
        // Treat null as a real template clear only when the scan collection is empty.
        if (value is null)
        {
            if (Files.Count == 0)
            {
                AppState.SelectedFile = null;
                SelectedTracks.Clear();
                ClearPropEditTemplateState();
            }
            return;
        }

        AppState.SelectedFile = value;
        SelectedTracks.Clear();
        PropEditTemplateFilePath = value.FilePath;

        foreach (var track in value.Tracks)
        {
            SelectedTracks.Add(track);
        }

        LoadPropEditTemplate(value);
    }

    partial void OnFolderPathChanged(string value)
    {
        if (!_isUpdatingFolderPathDisplay)
        {
            _selectedScanFolderPaths.Clear();
        }
    }

    partial void OnRootFolderPathChanged(string value) => SaveSettingsIfReady();
    partial void OnSourceFolderStartModeChanged(string value)
    {
        SourceFolderStartMode = NormalizeSourceFolderStartMode(value);
        SaveSettingsIfReady();
    }
    partial void OnMkvToolNixDirectoryChanged(string value)
    {
        SaveSettingsIfReady();
        OnPropertyChanged(nameof(MkvMergePath));
        OnPropertyChanged(nameof(MkvPropEditPath));
        OnPropertyChanged(nameof(MkvExtractPath));
        OnPropertyChanged(nameof(MkvInfoPath));
    }

    partial void OnFfmpegDirectoryChanged(string value)
    {
        SaveSettingsIfReady();
        OnPropertyChanged(nameof(FfmpegPath));
        OnPropertyChanged(nameof(FfProbePath));
    }
    partial void OnIgnoredScanFolderNameTextChanged(string value)
    {
        // Dashboard ignored-subfolder changes are saved automatically.
        SaveSettingsIfReady();
    }
    partial void OnWatchFolderTextChanged(string value)
    {
        SaveSettingsIfReady();
        if (_isLoadingSettings) return;
        RefreshLibraryAuditWatchFolderOptions();
    }
    partial void OnEnableLiveWatchFolderMonitoringChanged(bool value)
    {
        SaveSettingsIfReady();
        if (_isLoadingSettings) return;
        _ = RestartWatchersAsync();
    }
    partial void OnTvdbApiKeyChanged(string value)
    {
        OnPropertyChanged(nameof(IsTvdbConfigured));
        OnPropertyChanged(nameof(IsSelectedRenameProviderConfigured));
        SaveSettingsIfReady();
    }
    partial void OnTvdbPinChanged(string value) => SaveSettingsIfReady();
    partial void OnTmdbApiKeyChanged(string value)
    {
        OnPropertyChanged(nameof(IsTmdbConfigured));
        OnPropertyChanged(nameof(IsSelectedRenameProviderConfigured));
        SaveSettingsIfReady();
    }
    partial void OnTvdbLanguageChanged(string value)
    {
        ClearRenameProviderCache();
        SaveSettingsIfReady();
    }

    partial void OnRenameLookupProviderChanged(string value)
    {
        ClearRenameProviderCache();
        TvdbSeriesResults.Clear();
        TvdbSeasonScopeOptions.Clear();
        SelectedTvdbSeries = null;
        RefreshDisplayedRenameTemplateOptions();
        RenameStatusText = $"Lookup provider: {NormalizeLookupProvider(value)}";
        OnPropertyChanged(nameof(IsSelectedRenameProviderConfigured));
        SaveSettingsIfReady();
    }
    partial void OnRenameTemplateChanged(string value)
    {
        if (!_isLoadingSettings)
        {
            IsRenamePreviewDirty = true;
        }

        var clean = NormalizeRenameTemplate(value);
        var matching = DisplayedRenameTemplateOptions.FirstOrDefault(x => string.Equals(x, ToDisplayRenameTemplate(clean), StringComparison.OrdinalIgnoreCase))
            ?? RenameTemplateOptions.FirstOrDefault(x => string.Equals(x, clean, StringComparison.OrdinalIgnoreCase));
        if (!string.IsNullOrWhiteSpace(matching) && !string.Equals(SelectedRenameTemplateOption, matching, StringComparison.OrdinalIgnoreCase))
        {
            SelectedRenameTemplateOption = matching;
        }
        SaveSettingsIfReady();
    }

    partial void OnSelectedRenameTemplateOptionChanged(string value)
    {
        var stored = ToStoredRenameTemplate(value);
        if (!string.IsNullOrWhiteSpace(stored) && !string.Equals(RenameTemplate, stored, StringComparison.OrdinalIgnoreCase))
        {
            RenameTemplate = stored;
        }
    }
    partial void OnMkvMergeDefaultAudioLanguagesChanged(string value) => SaveSettingsIfReady();
    partial void OnMkvMergeDefaultSubtitleLanguagesChanged(string value) => SaveSettingsIfReady();
    partial void OnMaxScanWorkersChanged(int value) => ApplyWorkerSettings();
    partial void OnMaxEditWorkersChanged(int value) => ApplyWorkerSettings();
    partial void OnMaxRemuxWorkersChanged(int value) => ApplyWorkerSettings();

    partial void OnSelectedTvdbSeriesChanged(TvdbSeriesSearchResult? value)
    {
        OnPropertyChanged(nameof(SelectedDatabaseUrl));
        OnPropertyChanged(nameof(HasSelectedDatabaseUrl));

        if (value is null) return;

        RefreshDisplayedRenameTemplateOptions();
        RenameStatusText = $"Selected {NormalizeLookupProvider(RenameLookupProvider)} result: {value.DisplayName}";
        RenameLog(RenameStatusText);
        if (!_suppressSelectedSeriesAutoLoad)
        {
            _ = LoadTvdbSeasonScopesAndPreviewAsync();
        }
    }

    [RelayCommand]
    private async Task BrowseFolder(Window window)
    {
        var options = new FolderPickerOpenOptions
        {
            Title = "Select MKV folder(s)",
            AllowMultiple = true
        };

        var startFolder = GetPreferredSourceFolderPath();
        if (!string.IsNullOrWhiteSpace(startFolder))
        {
            options.SuggestedStartLocation = await window.StorageProvider.TryGetFolderFromPathAsync(startFolder);
        }

        var folders = await window.StorageProvider.OpenFolderPickerAsync(options);
        if (folders.Count > 0)
        {
            var paths = folders
                .Select(folder => folder.Path.LocalPath)
                .Where(path => !string.IsNullOrWhiteSpace(path))
                .Distinct(CrossPlatformRuntime.PathComparer)
                .ToList();

            _selectedScanFolderPaths = paths;
            SetFolderPathDisplay(GetCommonFolderPath(paths));
            _lastBrowseFolderPath = paths.FirstOrDefault() ?? FolderPath;
            SaveSettingsIfReady();
            Log(paths.Count == 1
                ? $"Folder selected: {paths[0]}"
                : $"Folders selected: {paths.Count}; display path: {FolderPath}");
            await EnsureWatchersInitializedAsync();
        }
    }

    [RelayCommand]
    private async Task BrowseRootFolder(Window window)
    {
        var folders = await window.StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
        {
            Title = "Select startup root folder",
            AllowMultiple = false
        });
        if (folders.Count > 0)
        {
            RootFolderPath = folders[0].Path.LocalPath;
            if (string.IsNullOrWhiteSpace(FolderPath) || !Directory.Exists(FolderPath))
            {
                _selectedScanFolderPaths = new List<string> { RootFolderPath };
                SetFolderPathDisplay(RootFolderPath);
                _lastBrowseFolderPath = RootFolderPath;
            }
            SaveSettingsIfReady();
            Log($"Startup root folder saved: {RootFolderPath}");
        }
    }

    [RelayCommand]
    private async Task BrowseMkvToolNixDirectory(Window window)
    {
        var folders = await window.StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
        {
            Title = "Select MKVToolNix folder",
            AllowMultiple = false
        });

        if (folders.Count > 0)
        {
            MkvToolNixDirectory = folders[0].Path.LocalPath;
            SaveSettingsIfReady();
            Log($"MKVToolNix directory saved: {MkvToolNixDirectory}");
        }
    }

    [RelayCommand]
    private void AutoFindMkvToolNixDirectory()
    {
        var candidates = GetMkvToolNixSearchDirectories()
            .Select(CrossPlatformRuntime.NormalizeUserPath)
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Distinct(CrossPlatformRuntime.PathComparer)
            .ToList();

        foreach (var candidate in candidates)
        {
            if (!IsValidMkvToolNixDirectory(candidate)) continue;

            MkvToolNixDirectory = candidate;
            MkvToolNixAutoFindStatusText = $"Found MKVToolNix: {candidate}";
            SaveSettingsIfReady();
            Log(MkvToolNixAutoFindStatusText);
            return;
        }

        MkvToolNixAutoFindStatusText = $"MKVToolNix not found in {candidates.Count} checked location(s). Install it or use Browse.";
        Log(MkvToolNixAutoFindStatusText);
    }

    [RelayCommand]
    private async Task BrowseFfmpegDirectory(Window window)
    {
        var folders = await window.StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
        {
            Title = "Select FFmpeg bin folder",
            AllowMultiple = false
        });

        if (folders.Count > 0)
        {
            FfmpegDirectory = folders[0].Path.LocalPath;
            SaveSettingsIfReady();
            Log($"FFmpeg directory saved: {FfmpegDirectory}");
        }
    }

    [RelayCommand]
    private void AutoFindFfmpegDirectory()
    {
        var candidates = GetFfmpegSearchDirectories()
            .Select(CrossPlatformRuntime.NormalizeUserPath)
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Distinct(CrossPlatformRuntime.PathComparer)
            .ToList();

        foreach (var candidate in candidates)
        {
            if (!IsValidFfmpegDirectory(candidate)) continue;

            FfmpegDirectory = candidate;
            FfmpegAutoFindStatusText = $"Found FFmpeg: {candidate}";
            SaveSettingsIfReady();
            Log(FfmpegAutoFindStatusText);
            return;
        }

        FfmpegAutoFindStatusText = $"FFmpeg not found in {candidates.Count} checked location(s). Install it or use Browse.";
        Log(FfmpegAutoFindStatusText);
    }

    [RelayCommand]
    private void SavePresetLists()
    {
        ReplaceCollection(AudioNamePresets, ParsePresetText(AudioNamePresetText));
        ReplaceCollection(SubtitleNamePresets, ParsePresetText(SubtitleNamePresetText));
        ReplaceCollection(LanguagePresets, ParsePresetText(LanguagePresetText));
        SaveSettingsIfReady();
        RefreshTrackPresetOptions();
        Log("Preset drop-down lists saved.");
    }

    [RelayCommand]
    private void ResetPresetLists()
    {
        ReplaceCollection(AudioNamePresets, DefaultAudioNamePresets());
        ReplaceCollection(SubtitleNamePresets, DefaultSubtitleNamePresets());
        ReplaceCollection(LanguagePresets, DefaultLanguagePresets());
        SyncPresetTextFromCollections();
        SaveSettingsIfReady();
        RefreshTrackPresetOptions();
        Log("Preset drop-down lists reset to defaults.");
    }

    [RelayCommand]
    private void UpdateIgnoredScanFolders()
    {
        var normalized = NormalizeIgnoredScanFolderText(IgnoredScanFolderNameText);
        IgnoredScanFolderNameText = normalized;
        SaveSettingsIfReady();

        var names = ParseIgnoredScanFolderNames(IgnoredScanFolderNameText).ToList();
        Log(names.Count > 0
            ? $"Updated ignored subfolders: {string.Join(", ", names)}"
            : "Ignored subfolder list cleared.");
    }


    public void RemoveDashboardFiles(IReadOnlyList<MkvFileItem> filesToRemove)
    {
        var removeList = filesToRemove
            .Where(file => file is not null && Files.Contains(file))
            .Distinct()
            .ToList();

        if (removeList.Count == 0)
        {
            return;
        }

        var firstRemovedIndex = removeList
            .Select(file => Files.IndexOf(file))
            .Where(index => index >= 0)
            .DefaultIfEmpty(0)
            .Min();

        var keepCurrentSelection = SelectedFile is not null && Files.Contains(SelectedFile) && !removeList.Contains(SelectedFile);
        var retainedSelection = keepCurrentSelection ? SelectedFile : null;

        foreach (var index in removeList
                     .Select(file => Files.IndexOf(file))
                     .Where(index => index >= 0)
                     .Distinct()
                     .OrderByDescending(index => index))
        {
            Files.RemoveAt(index);
        }

        var nextSelection = retainedSelection;
        if (nextSelection is null && Files.Count > 0)
        {
            nextSelection = Files[Math.Clamp(firstRemovedIndex, 0, Files.Count - 1)];
        }

        RefreshDashboardSelection(nextSelection);

        if (Files.Count > 0)
        {
            EvaluateTrackTemplateDeviations();
            BuildDashboardMismatchReport();
            SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: false);
        }
        else
        {
            AppState.PlannedActions.Clear();
            ResetRenameMetadataContextForNewScan();
        }

        StatusText = Files.Count == 0
            ? "Removed selected file row(s). No files remain."
            : $"Removed {removeList.Count} file row(s). {Files.Count} file(s) remain.";
        Log(StatusText);
    }

    public void RestoreDashboardSelectionAfterGridMutation()
    {
        var nextSelection = SelectedFile is not null && Files.Contains(SelectedFile)
            ? SelectedFile
            : Files.FirstOrDefault();

        RefreshDashboardSelection(nextSelection);
    }

    private void RefreshDashboardSelection(MkvFileItem? nextSelection)
    {
        // Force the detail bindings to rehydrate even when the selected object is unchanged
        // after a delete operation. This keeps Media Info, Track Info, propedit templates,
        // and cross-panel AppState synchronized with the visible file collection.
        SelectedFile = null;
        AppState.SelectedFile = null;
        SelectedTracks.Clear();

        if (nextSelection is null)
        {
            ClearPropEditTemplateState();
            return;
        }

        SelectedFile = nextSelection;
        AppState.SelectedFile = nextSelection;
    }

    public async Task ScanDroppedFolderAsync(string folderPath)
    {
        if (IsBusy)
        {
            Log("Drop ignored because a scan is already running.");
            return;
        }

        if (string.IsNullOrWhiteSpace(folderPath) || !Directory.Exists(folderPath))
        {
            Log("Dropped item is not a valid folder.");
            return;
        }

        _selectedScanFolderPaths = new List<string> { folderPath };
        SetFolderPathDisplay(folderPath);
        _lastBrowseFolderPath = folderPath;
        SaveSettingsIfReady();
        Log($"Dropped folder selected: {folderPath}");
        await EnsureWatchersInitializedAsync();

        await Scan();
    }

    [RelayCommand]
    private async Task Scan()
    {
        var scanFolders = GetSelectedScanFolders();
        if (scanFolders.Count == 0)
        {
            Log("Select a valid folder first.");
            return;
        }

        if (string.IsNullOrWhiteSpace(MkvMergePath))
        {
            Log("Configure the MKVToolNix folder in Settings or ensure mkvmerge is available on PATH.");
            return;
        }

        await EnsureWatchersInitializedAsync();

        // Commit the ignored-subfolder editor and last successful source folder before starting a scan.
        IgnoredScanFolderNameText = NormalizeIgnoredScanFolderText(IgnoredScanFolderNameText);
        _lastBrowseFolderPath = scanFolders[0];
        SaveSettingsIfReady();

        _cts?.Cancel();
        _cts = new CancellationTokenSource();
        ResetRenameMetadataContextForNewScan();
        AppState.ClearScanCollections();
        SelectedFile = null;
        IsBusy = true;
        BeginGlobalOperation("scan");
        AppState.BeginScan(FolderPath);
        var scanUsesWatchCache = scanFolders.All(IsPathUnderAnyWatchFolder);
        var activeScanPipeline = scanUsesWatchCache ? _scanPipeline : _tempScanPipeline;
        if (!scanUsesWatchCache)
        {
            var pruned = PruneExpiredTempMetadataCache();
            if (pruned > 0)
            {
                Log($"Temp cache cleanup: removed {pruned} entr{(pruned == 1 ? "y" : "ies")} older than 7 days.");
            }
        }

        Log(scanFolders.Count == 1
            ? $"Scanning: {scanFolders[0]}"
            : $"Scanning {scanFolders.Count} folders from: {FolderPath}");
        Log(scanUsesWatchCache
            ? "Cache scope: watch-folder database"
            : "Cache scope: temp scan database");
        Log($"Using mkvmerge: {MkvMergePath}");
        Log($"Using ffprobe: {FfProbePath}");

        try
        {
            var ignoredFolderNames = ParseIgnoredScanFolderNames(IgnoredScanFolderNameText).ToList();
            if (ignoredFolderNames.Count > 0)
            {
                Log($"Ignoring subfolders named: {string.Join(", ", ignoredFolderNames)}");
            }

            Log($"Scan workers: {_workerSettings.CloneNormalized().MaxScanWorkers}");

            foreach (var scanFolder in scanFolders)
            {
                var request = new ScanPipelineRequest(scanFolder, MkvMergePath, FfProbePath, ignoredFolderNames, _workerSettings.CloneNormalized());
                await activeScanPipeline.ExecuteAsync(
                    request,
                    item =>
                    {
                        InsertFileSorted(item);
                        if (SelectedFile is null)
                        {
                            SelectedFile = item;
                            AppState.SelectedFile = item;
                        }

                        return Task.CompletedTask;
                    },
                    (completed, total) =>
                    {
                        AppState.UpdateScanProgress(completed, total);
                        UpdateGlobalOperation(completed, total);
                    },
                    Log,
                    _cts.Token);
            }

            EvaluateTrackTemplateDeviations();
            CompleteGlobalOperation($"Scan complete: {Files.Count} file(s)");
            BuildDashboardMismatchReport();
            SyncRenameFromDashboardSelection(preserveSearchTitle: false, writeLog: false);
            RefreshCacheLifecycleStatus();
            Log(StatusText);
        }
        catch (OperationCanceledException)
        {
            StatusText = "Scan canceled";
            Log(StatusText);
        }
        finally
        {
            AppState.CompleteOperation(StatusText);
            IsBusy = false;
        }
    }


    private IReadOnlyList<string> GetSelectedScanFolders()
    {
        var selectedFolders = _selectedScanFolderPaths
            .Where(Directory.Exists)
            .Distinct(CrossPlatformRuntime.PathComparer)
            .ToList();

        if (selectedFolders.Count > 0)
        {
            return selectedFolders;
        }

        return !string.IsNullOrWhiteSpace(FolderPath) && Directory.Exists(FolderPath)
            ? new[] { FolderPath }
            : Array.Empty<string>();
    }

    private void SetFolderPathDisplay(string value)
    {
        _isUpdatingFolderPathDisplay = true;
        try
        {
            FolderPath = value;
        }
        finally
        {
            _isUpdatingFolderPathDisplay = false;
        }
    }

    private static string GetCommonFolderPath(IReadOnlyList<string> paths)
    {
        var normalizedPaths = paths
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Select(path => Path.GetFullPath(path).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar))
            .ToList();

        if (normalizedPaths.Count == 0)
        {
            return string.Empty;
        }

        if (normalizedPaths.Count == 1)
        {
            return normalizedPaths[0];
        }

        var commonPath = normalizedPaths[0];
        foreach (var path in normalizedPaths.Skip(1))
        {
            while (!PathStartsWithFolder(path, commonPath))
            {
                var parent = Path.GetDirectoryName(commonPath);
                if (string.IsNullOrWhiteSpace(parent))
                {
                    return Path.GetPathRoot(normalizedPaths[0]) ?? normalizedPaths[0];
                }

                commonPath = parent.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            }
        }

        return commonPath;
    }

    private static bool PathStartsWithFolder(string path, string folder)
    {
        if (path.Equals(folder, StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        var folderWithSeparator = folder.EndsWith(Path.DirectorySeparatorChar) || folder.EndsWith(Path.AltDirectorySeparatorChar)
            ? folder
            : folder + Path.DirectorySeparatorChar;

        return path.StartsWith(folderWithSeparator, StringComparison.OrdinalIgnoreCase);
    }

    private void InsertFileSorted(MkvFileItem item)
    {
        var targetIndex = 0;
        while (targetIndex < Files.Count && CompareFileOrder(Files[targetIndex], item) <= 0)
        {
            targetIndex++;
        }

        Files.Insert(targetIndex, item);
    }

    private int CompareFileOrder(MkvFileItem left, MkvFileItem right)
    {
        var leftKey = GetFileSortKey(left.FilePath);
        var rightKey = GetFileSortKey(right.FilePath);

        var seasonCompare = leftKey.Season.CompareTo(rightKey.Season);
        if (seasonCompare != 0) return seasonCompare;

        var episodeCompare = leftKey.Episode.CompareTo(rightKey.Episode);
        if (episodeCompare != 0) return episodeCompare;

        var partCompare = leftKey.Part.CompareTo(rightKey.Part);
        if (partCompare != 0) return partCompare;

        return NaturalStringComparer.Instance.Compare(leftKey.RelativeName, rightKey.RelativeName);
    }

    private FileSortKey GetFileSortKey(string filePath)
    {
        return GetFileSortKey(filePath, FolderPath);
    }

    private static FileSortKey GetFileSortKey(string filePath, string rootFolder)
    {
        var fileName = Path.GetFileNameWithoutExtension(filePath);
        var relativeName = GetRelativeSortName(filePath, rootFolder);

        var episodeMatch = Regex.Match(
            fileName,
            @"(?:^|[\s._\-\[\(])S(?<season>\d{1,3})\s*E(?<episode>\d{1,4})(?:\s*[-+&]\s*E?(?<part>\d{1,4}))?",
            RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

        if (episodeMatch.Success)
        {
            return new FileSortKey(
                ParseSortNumber(episodeMatch.Groups["season"].Value, int.MaxValue),
                ParseSortNumber(episodeMatch.Groups["episode"].Value, int.MaxValue),
                ParseSortNumber(episodeMatch.Groups["part"].Value, 0),
                relativeName);
        }

        var absoluteEpisodeMatch = Regex.Match(
            fileName,
            @"(?:^|[\s._\-\[\(])(?<episode>\d{1,4})(?:v\d+)?(?:$|[\s._\-\]\)])",
            RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

        if (absoluteEpisodeMatch.Success)
        {
            return new FileSortKey(
                int.MaxValue - 1,
                ParseSortNumber(absoluteEpisodeMatch.Groups["episode"].Value, int.MaxValue),
                0,
                relativeName);
        }

        return new FileSortKey(int.MaxValue, int.MaxValue, 0, relativeName);
    }

    private static string GetRelativeSortName(string filePath, string rootFolder)
    {
        if (!string.IsNullOrWhiteSpace(rootFolder))
        {
            try
            {
                return Path.GetRelativePath(rootFolder, filePath);
            }
            catch
            {
                // Fall through to filename-only sorting if relative path parsing fails.
            }
        }

        return Path.GetFileName(filePath);
    }

    private static int ParseSortNumber(string value, int fallback)
    {
        return int.TryParse(value, out var parsed) ? parsed : fallback;
    }

    private sealed record FileSortKey(int Season, int Episode, int Part, string RelativeName);


    private bool CanUseSelectedFileAsTemplate()
    {
        return SelectedFile is not null && Files.Contains(SelectedFile);
    }

    [RelayCommand(CanExecute = nameof(CanUseSelectedFileAsTemplate))]
    private void UseSelectedFileAsTemplate()
    {
        if (SelectedFile is null || !Files.Contains(SelectedFile))
        {
            DashboardConsoleLines.Add("Select a scanned file first.");
            return;
        }

        ComparisonTemplateFile = SelectedFile;
        DashboardConsoleLines.Add($"Template file set: {SelectedFile.FileName}");
        StatusText = $"Template file set: {SelectedFile.FileName}";
        EvaluateTrackTemplateDeviations();
    }

    private void EvaluateTrackTemplateDeviations()
    {
        foreach (var file in Files)
        {
            var previousStatus = file.Status;
            file.HasTrackMismatch = false;
            file.MismatchSummary = string.Empty;
            file.Status = "Ready";
            file.ResetDifferenceHighlighting();

            if (file.Tracks.Count == 0
                || previousStatus.Contains("failed", StringComparison.OrdinalIgnoreCase)
                || previousStatus.Contains("no tracks", StringComparison.OrdinalIgnoreCase))
            {
                file.HasTrackMismatch = true;
                file.MismatchSummary = "Unable to verify media/track info.";
                file.Status = "Warning";
            }
        }

        if (Files.Count < 2) return;

        var template = ComparisonTemplateFile is not null && Files.Contains(ComparisonTemplateFile)
            ? ComparisonTemplateFile
            : Files[0];

        for (var fileIndex = 0; fileIndex < Files.Count; fileIndex++)
        {
            var file = Files[fileIndex];
            if (ReferenceEquals(file, template))
            {
                file.Status = "Template";
                continue;
            }

            var issues = new List<string>();

            AddMetadataDeviationIssues(file, template, issues);
            AddTrackFieldDeviationIssues(file, template, issues);

            file.HasTrackMismatch = issues.Count > 0;
            file.MismatchSummary = string.Join(" ", issues.Distinct(StringComparer.OrdinalIgnoreCase));
            file.Status = file.HasTrackMismatch ? "Warning" : "Ready";
        }
    }

    private static void AddMetadataDeviationIssues(MkvFileItem file, MkvFileItem template, List<string> issues)
    {
        // File name/path are intentionally ignored. Only media metadata values are compared.
        if (!SameMetadataValue(file.Resolution, template.Resolution))
        {
            file.ResolutionVisualState = VisualState.Warning;
            issues.Add("Resolution differs");
        }

        if (!SameMetadataValue(file.Codec, template.Codec))
        {
            file.CodecVisualState = VisualState.Warning;
            issues.Add("Codec differs");
        }

        if (!SameMetadataValue(file.BitDepth, template.BitDepth))
        {
            file.BitDepthVisualState = VisualState.Warning;
            issues.Add("Bit depth differs");
        }

        if (!SameMetadataValue(file.AudioSummary, template.AudioSummary))
        {
            file.AudioSummaryVisualState = VisualState.Warning;
            issues.Add("Audio summary differs");
        }

        if (!SameMetadataValue(file.SubtitleSummary, template.SubtitleSummary))
        {
            file.SubtitleSummaryVisualState = VisualState.Warning;
            issues.Add("Subtitle summary differs");
        }
    }

    private static void AddTrackFieldDeviationIssues(MkvFileItem file, MkvFileItem template, List<string> issues)
    {
        var templateTracks = GetComparableTrackItems(template);
        var fileTracks = GetComparableTrackItems(file);

        if (templateTracks.Count != fileTracks.Count)
        {
            issues.Add("Track count differs");
        }

        var max = Math.Max(templateTracks.Count, fileTracks.Count);
        for (var i = 0; i < max; i++)
        {
            if (i >= fileTracks.Count)
            {
                issues.Add($"Track {i + 1} missing");
                continue;
            }

            var actual = fileTracks[i];
            if (i >= templateTracks.Count)
            {
                actual.HighlightAllDifferenceFields();
                issues.Add($"Track {i + 1} extra");
                continue;
            }

            var expected = templateTracks[i];
            var trackLabel = $"Track {i + 1}";

            if (actual.PropEditTrackNumber != expected.PropEditTrackNumber)
            {
                actual.TrackNumberVisualState = VisualState.Warning;
                issues.Add($"{trackLabel} number differs");
            }

            if (!SameMetadataValue(actual.Type, expected.Type))
            {
                actual.TypeVisualState = VisualState.Warning;
                issues.Add($"{trackLabel} type differs");
            }

            if (!string.Equals(NormalizeTrackLanguage(actual.Language), NormalizeTrackLanguage(expected.Language), StringComparison.OrdinalIgnoreCase))
            {
                actual.LanguageVisualState = VisualState.Warning;
                issues.Add($"{trackLabel} language differs");
            }

            if (!SameMetadataValue(actual.Codec, expected.Codec))
            {
                actual.CodecVisualState = VisualState.Warning;
                issues.Add($"{trackLabel} codec differs");
            }

            if (!string.Equals(NormalizeTrackName(actual.Name), NormalizeTrackName(expected.Name), StringComparison.OrdinalIgnoreCase))
            {
                actual.NameVisualState = VisualState.Warning;
                issues.Add($"{trackLabel} name differs");
            }
        }
    }

    private static List<MkvTrackItem> GetComparableTrackItems(MkvFileItem file)
    {
        return file.Tracks
            .Where(t => IsComparableTrackType(t.Type))
            .OrderBy(t => t.PropEditTrackNumber)
            .ToList();
    }

    private static bool IsComparableTrackType(string? type)
    {
        return string.Equals(type, "audio", StringComparison.OrdinalIgnoreCase)
            || string.Equals(type, "subtitles", StringComparison.OrdinalIgnoreCase)
            || string.Equals(type, "subtitle", StringComparison.OrdinalIgnoreCase);
    }

    private static bool SameMetadataValue(string? left, string? right)
    {
        return string.Equals((left ?? string.Empty).Trim(), (right ?? string.Empty).Trim(), StringComparison.OrdinalIgnoreCase);
    }

    private static List<string> GetComparableTracks(MkvFileItem file, string type)
    {
        var tracks = file.Tracks
            .Where(t => type == "audio"
                ? string.Equals(t.Type, "audio", StringComparison.OrdinalIgnoreCase)
                : string.Equals(t.Type, "subtitles", StringComparison.OrdinalIgnoreCase) || string.Equals(t.Type, "subtitle", StringComparison.OrdinalIgnoreCase))
            .OrderBy(t => t.PropEditTrackNumber)
            .ToList();

        var signatures = new List<string>();
        for (var index = 0; index < tracks.Count; index++)
        {
            var track = tracks[index];
            var position = index + 1;
            var propEditId = track.PropEditTrackNumber;
            var language = NormalizeTrackLanguage(track.Language);
            var name = NormalizeTrackName(track.Name);

            signatures.Add($"{type}:{position}:id={propEditId}:lang={language}:name={name}");
        }

        return signatures;
    }

    private static List<string> GetDisplayTracks(MkvFileItem file, string type)
    {
        return file.Tracks
            .Where(t => type == "audio"
                ? string.Equals(t.Type, "audio", StringComparison.OrdinalIgnoreCase)
                : string.Equals(t.Type, "subtitles", StringComparison.OrdinalIgnoreCase) || string.Equals(t.Type, "subtitle", StringComparison.OrdinalIgnoreCase))
            .OrderBy(t => t.PropEditTrackNumber)
            .Select(t =>
            {
                var lang = NormalizeTrackLanguage(t.Language);
                var name = string.IsNullOrWhiteSpace(t.Name) ? "<no name>" : t.Name.Trim();
                return $"#{t.PropEditTrackNumber} {lang} | {name}";
            })
            .ToList();
    }

    private static string NormalizeTrackName(string? name)
    {
        return string.IsNullOrWhiteSpace(name)
            ? "<blank>"
            : name.Trim().ToLowerInvariant();
    }

    private static void AddTrackListDeviationIssues(List<string> issues, string label, IReadOnlyList<string> template, IReadOnlyList<string> actual)
    {
        if (template.Count != actual.Count)
        {
            issues.Add($"{label} count differs");
        }

        var max = Math.Max(template.Count, actual.Count);
        for (var i = 0; i < max; i++)
        {
            var expected = i < template.Count ? template[i] : "missing";
            var found = i < actual.Count ? actual[i] : "missing";

            if (!string.Equals(expected, found, StringComparison.OrdinalIgnoreCase))
            {
                issues.Add($"{label} track {i + 1} differs");
            }
        }
    }

    private void BuildDashboardMismatchReport()
    {
        DashboardConsoleLines.Clear();

        if (Files.Count == 0)
        {
            DashboardConsoleLines.Add("No files scanned.");
            return;
        }

        var template = Files[0];
        var mismatches = Files.Skip(1).Where(f => f.HasTrackMismatch).ToList();

        DashboardConsoleLines.Add("TRACK TEMPLATE CHECK");
        DashboardConsoleLines.Add(new string('=', 60));
        DashboardConsoleLines.Add($"Standard file: {template.FileName}");
        DashboardConsoleLines.Add($"Files scanned : {Files.Count}");
        DashboardConsoleLines.Add($"Different     : {mismatches.Count}");
        DashboardConsoleLines.Add(new string('=', 60));

        if (mismatches.Count == 0)
        {
            DashboardConsoleLines.Add("No files are different from the standard file.");
            return;
        }

        DashboardConsoleLines.Add("Files different from standard:");
        foreach (var file in mismatches)
        {
            DashboardConsoleLines.Add($"  - {file.FileName}");
        }
    }

    private static void AddIndentedTrackLines(ObservableCollection<string> lines, IReadOnlyList<string> tracks, string indent = "    ")
    {
        if (tracks.Count == 0)
        {
            lines.Add(indent + "None");
            return;
        }

        foreach (var track in tracks)
        {
            lines.Add(indent + track);
        }
    }

    private static string Truncate(string value, int maxLength)
    {
        if (string.IsNullOrEmpty(value) || value.Length <= maxLength) return value;
        return value[..Math.Max(0, maxLength - 3)] + "...";
    }

    private static string NormalizeTrackLanguage(string? language)
    {
        var value = (language ?? string.Empty).Trim().ToLowerInvariant();
        return value switch
        {
            "en" or "eng" or "english" => "eng",
            "ja" or "jpn" or "jp" or "japanese" => "jpn",
            "es" or "spa" or "spanish" => "spa",
            "fr" or "fre" or "fra" or "french" => "fre",
            "de" or "ger" or "deu" or "german" => "ger",
            "und" or "unknown" or "" => "und",
            _ => value
        };
    }
}
