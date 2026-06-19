using System.Collections.Generic;
using System.Linq;
using System;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.Text;
using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using MKVOrchestrator.App.Services;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Pipeline;
using MKVOrchestrator.Core.Services.State;
using MKVOrchestrator.Core.Services.Cache;
using MKVOrchestrator.Core.Services.Library;
using MKVOrchestrator.Core.Services.Audit;
using MKVOrchestrator.Core.Services.Operations;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel : ObservableObject
{
    private readonly MkvScannerService _scanner = new(new MetadataCacheDatabase("metadata_cache.db"));
    private readonly MkvScannerService _tempScanner = new(new MetadataCacheDatabase("metadata_cache_temp.db"));
    private readonly MetadataCacheServiceAdapter _mediaCache;
    private readonly MetadataCacheServiceAdapter _tempMediaCache;
    private readonly MediaLibraryService _mediaLibrary;
    private readonly MediaLibraryService _tempMediaLibrary;
    private readonly LibraryAuditService _libraryAudit;
    private readonly MkvMergeService _mkvMerge = new();
    private readonly ScanPipeline _scanPipeline;
    private readonly ScanPipeline _tempScanPipeline;
    public AppStateService AppState { get; } = new();
    private readonly ActionPlanner _planner = new();
    private readonly MkvPropEditService _propEdit = new();
    private readonly MkvPropEditCommandBuilder _propEditCommandBuilder = new();
    private readonly GlobalOperationStatusService _operationStatus = new();
    private readonly AppSettingsService _settingsService = new();
    private readonly ExecutionQueueService _executionQueue = new();
    private readonly FileConflictService _fileConflict = new();
    private readonly RenameBatchHistoryService _renameBatchHistory = new();
    private readonly Dictionary<string, IRenameMetadataProvider> _renameMetadataProviders = new(StringComparer.OrdinalIgnoreCase)
    {
        ["TVDB"] = new TvdbRenameMetadataProvider(),
        ["TMDB"] = new TmdbRenameMetadataProvider()
    };
    private WorkerSettings _workerSettings = WorkerSettings.Defaults;
    private bool _isLoadingSettings;
    private string _lastBrowseFolderPath = string.Empty;
    private List<string> _selectedScanFolderPaths = new();
    private bool _isUpdatingFolderPathDisplay;
    private CancellationTokenSource? _cts;
    private CancellationTokenSource? _cacheCts;
    private CancellationTokenSource? _auditCts;
    private List<TvdbEpisode> _cachedTvdbEpisodes = new();
    private int? _cachedTvdbSeriesId;
    private string _cachedTvdbLanguage = string.Empty;
    private string _cachedLookupProvider = string.Empty;
    private bool _suppressSelectedSeriesAutoLoad;
    private bool _suppressScopeSelectionCascade;
    private readonly List<FileSystemWatcher> _watchers = new();
    private readonly Dictionary<string, CancellationTokenSource> _watchDebounce = new(CrossPlatformRuntime.PathComparer);
    private readonly object _watchGate = new();
    private readonly SemaphoreSlim _watcherInitGate = new(1, 1);
    private bool _watchersInitialized;

    public MainWindowViewModel()
    {
        _mediaCache = new MetadataCacheServiceAdapter(_scanner.Cache);
        _tempMediaCache = new MetadataCacheServiceAdapter(_tempScanner.Cache);
        _mediaLibrary = new MediaLibraryService(new MkvScannerServiceAdapter(_scanner), _mediaCache);
        _tempMediaLibrary = new MediaLibraryService(new MkvScannerServiceAdapter(_tempScanner), _tempMediaCache);
        _libraryAudit = new LibraryAuditService(_mediaCache);
        _scanPipeline = new ScanPipeline(_mediaLibrary);
        _tempScanPipeline = new ScanPipeline(_tempMediaLibrary);
        AppState.DashboardFileSelectionChanged += (_, _) => SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: false);
        AppState.Files.CollectionChanged += OnDashboardFilesChanged;
        LoadSettings();
        PruneExpiredTempMetadataCache();
    }

    public ObservableCollection<MkvFileItem> Files => AppState.Files;
    public bool HasDashboardFiles => Files.Count > 0;
    public bool HasNoDashboardFiles => Files.Count == 0;
    public ObservableCollection<MkvTrackItem> SelectedTracks => AppState.SelectedTracks;
    public string MkvMergePath => ResolveMkvToolPath("mkvmerge.exe", "mkvmerge");
    public string MkvPropEditPath => ResolveMkvToolPath("mkvpropedit.exe", "mkvpropedit");
    public string MkvExtractPath => ResolveMkvToolPath("mkvextract.exe", "mkvextract");
    public string MkvInfoPath => ResolveMkvToolPath("mkvinfo.exe", "mkvinfo");
    public string FfmpegPath => ResolveFfmpegToolPath("ffmpeg.exe", "ffmpeg");
    public string FfProbePath => ResolveFfmpegToolPath("ffprobe.exe", "ffprobe");
    public ObservableCollection<string> PlannedActions => AppState.PlannedActions;
    public ObservableCollection<string> ConsoleLines => AppState.ConsoleLines;
    public ObservableCollection<string> DashboardConsoleLines => AppState.DashboardConsoleLines;
    public ObservableCollection<RenamePreviewItem> RenameItems => AppState.RenameItems;
    public ObservableCollection<TvdbSeriesSearchResult> TvdbSeriesResults { get; } = new();
    public ObservableCollection<string> RenameLookupProviderOptions { get; } = new() { "TVDB", "TMDB" };
    public ObservableCollection<string> SourceFolderStartModeOptions { get; } = new()
    {
        "Default root folder",
        "Remember last directory"
    };
    public ObservableCollection<string> ThemeOptions { get; } = new();
    private List<ThemeDefinition> _customThemes = new();
    public ObservableCollection<string> RenameConsoleLines { get; } = new();
    public ObservableCollection<string> LibraryAuditWatchFolderOptions { get; } = new();
    public ObservableCollection<LibraryAuditSeasonItem> LibraryAuditItems { get; } = new();
    public ObservableCollection<LibraryAuditSeasonItem> DisplayedLibraryAuditItems { get; } = new();
    public ObservableCollection<LibraryAuditIssueLine> SelectedLibraryAuditIssueLines { get; } = new();
    public ObservableCollection<ExecutionJob> ExecutionJobs => _executionQueue.Jobs;
    public ObservableCollection<string> ExecutionSummaryLines { get; } = new();
    public Action<string, IReadOnlyList<string>>? ShowOutputWindow { get; set; }
    public Func<IReadOnlyList<RenameBatchRecord>, Action, Func<RenameBatchRecord, RenameBatchUndoPreview>, Func<RenameBatchRecord, Task<RenameBatchUndoResult>>, Task>? ShowRenameUndoWindowAsync { get; set; }
    public ObservableCollection<string> RenameTemplateOptions { get; } = new();
    public ObservableCollection<string> DisplayedRenameTemplateOptions { get; } = new();
    public bool IsTvdbConfigured => !string.IsNullOrWhiteSpace(TvdbApiKey);
    public bool IsTmdbConfigured => !string.IsNullOrWhiteSpace(TmdbApiKey);
    public bool IsSelectedRenameProviderConfigured => IsRenameProviderConfigured(RenameLookupProvider);
    public IBrush PreviewButtonForeground => IsRenamePreviewDirty ? Brushes.Orange : Brushes.White;
    public string RenamePreviewViewButtonText => IsRenamePreviewCompactView ? "Detailed View" : "Compact View";
    public string SelectedDatabaseUrl => SelectedTvdbSeries?.DatabaseUrl ?? string.Empty;
    public bool HasSelectedDatabaseUrl => !string.IsNullOrWhiteSpace(SelectedDatabaseUrl);

    public ObservableCollection<TvdbSeasonScopeOption> TvdbSeasonScopeOptions { get; } = new();

    public ObservableCollection<PropEditTrackConfig> PropEditAudioTracks { get; } = new();
    public ObservableCollection<PropEditTrackConfig> PropEditSubtitleTracks { get; } = new();
    public ObservableCollection<string> DefaultAudioOptions { get; } = new();
    public ObservableCollection<string> DefaultSubtitleOptions { get; } = new();
    public ObservableCollection<string> ForcedAudioOptions { get; } = new();
    public ObservableCollection<string> ForcedSubtitleOptions { get; } = new();
    public ObservableCollection<string> AudioNamePresets { get; } = new();
    public ObservableCollection<string> SubtitleNamePresets { get; } = new();
    public ObservableCollection<string> LanguagePresets { get; } = new();



    partial void OnIsRenamePreviewDirtyChanged(bool value)
    {
        OnPropertyChanged(nameof(PreviewButtonForeground));
    }

    partial void OnIsRenamePreviewCompactViewChanged(bool value)
    {
        OnPropertyChanged(nameof(RenamePreviewViewButtonText));
        SaveSettingsIfReady();
    }

    private void OnDashboardFilesChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        OnPropertyChanged(nameof(HasDashboardFiles));
        OnPropertyChanged(nameof(HasNoDashboardFiles));
    }

    [ObservableProperty] private string folderPath = string.Empty;
    [ObservableProperty] private string currentSection = "Dashboard";
    [ObservableProperty] private string rootFolderPath = string.Empty;
    [ObservableProperty] private string sourceFolderStartMode = "Default root folder";
    [ObservableProperty] private string mkvToolNixDirectory = string.Empty;
    [ObservableProperty] private string ffmpegDirectory = string.Empty;
    [ObservableProperty] private MkvFileItem? selectedFile;
    [ObservableProperty] private MkvFileItem? comparisonTemplateFile;
    [ObservableProperty] private bool isBusy;
    [ObservableProperty] private bool isSummaryExpanded;
    [ObservableProperty] private bool isRenameSummaryExpanded;
    [ObservableProperty] private bool isMergeSummaryExpanded;
    [ObservableProperty] private bool isPropEditSummaryExpanded;
    [ObservableProperty] private bool isLibraryAuditSummaryExpanded;
    [ObservableProperty] private bool showLibraryWarningsOnly;
    public string LibraryWarningFilterButtonText => ShowLibraryWarningsOnly ? "Show All" : "Show Warnings";
    [ObservableProperty] private bool removeContainerTitle = true;
    [ObservableProperty] private bool removeVideoTrackTitles = true;
    [ObservableProperty] private bool removeAudioTrackTitles;
    [ObservableProperty] private bool removeSubtitleTrackTitles;
    [ObservableProperty] private string audioLanguage = string.Empty;
    [ObservableProperty] private string subtitleLanguage = string.Empty;
    [ObservableProperty] private string statusText = "Ready";
    [ObservableProperty] private string executionStatusText = "Execution Center: Ready";
    [ObservableProperty] private string globalProgressText = "Ready";
    [ObservableProperty] private double globalProgressValue;
    [ObservableProperty] private bool isGlobalProgressIndeterminate;
    [ObservableProperty] private string audioNamePresetText = string.Empty;
    [ObservableProperty] private string subtitleNamePresetText = string.Empty;
    [ObservableProperty] private string languagePresetText = string.Empty;
    [ObservableProperty] private string ignoredScanFolderNameText = string.Empty;
    [ObservableProperty] private string watchFolderText = string.Empty;
    [ObservableProperty] private bool enableLiveWatchFolderMonitoring;
    [ObservableProperty] private string cacheStatusText = "Cache database ready";
    [ObservableProperty] private string cacheWatchEntryCountText = "0";
    [ObservableProperty] private string cacheTempEntryCountText = "0";
    [ObservableProperty] private string cacheDatabaseSizeText = "0 B";
    [ObservableProperty] private string cacheLastCleanupText = "Not yet this session";
    [ObservableProperty] private string selectedThemeName = "Midnight";
    [ObservableProperty] private string customThemeName = string.Empty;
    [ObservableProperty] private string themeEditorText = string.Empty;
    [ObservableProperty] private string themeStatusText = "Select a theme, edit its JSON, then reload to apply.";
    [ObservableProperty] private string mkvToolNixAutoFindStatusText = "Auto-find checks common install folders and PATH.";
    [ObservableProperty] private string ffmpegAutoFindStatusText = "Auto-find checks common install folders and PATH.";
    [ObservableProperty] private bool isCacheBusy;
    [ObservableProperty] private int maxScanWorkers = WorkerSettings.Defaults.MaxScanWorkers;
    [ObservableProperty] private int maxEditWorkers = WorkerSettings.Defaults.MaxEditWorkers;
    [ObservableProperty] private int maxRemuxWorkers = WorkerSettings.Defaults.MaxRemuxWorkers;
    [ObservableProperty] private string selectedLibraryAuditWatchFolder = string.Empty;
    [ObservableProperty] private LibraryAuditSeasonItem? selectedLibraryAuditItem;
    [ObservableProperty] private string libraryAuditStatusText = "Select a watch folder and build the library overview.";
    [ObservableProperty] private string libraryAuditDetailSummary = "Build an overview to see library totals here.";
    [ObservableProperty] private bool isLibraryAuditBusy;
    [ObservableProperty] private string mkvMergeDefaultAudioLanguages = "eng,jpn";
    [ObservableProperty] private string mkvMergeDefaultSubtitleLanguages = "eng";

    [ObservableProperty] private string tvdbApiKey = string.Empty;
    [ObservableProperty] private string tvdbPin = string.Empty;
    [ObservableProperty] private string tvdbLanguage = "eng";
    [ObservableProperty] private string tmdbApiKey = string.Empty;
    [ObservableProperty] private string apiProviderStatusText = "API keys are stored locally. Configure the provider you want to use.";
    [ObservableProperty] private string renameLookupProvider = "TVDB";
    [ObservableProperty] private string renameSearchTitle = string.Empty;
    [ObservableProperty] private string renameTemplate = "{series} - S{season:00}E{episode:00} - {episodeTitle}";
    [ObservableProperty] private string selectedRenameTemplateOption = string.Empty;
    [ObservableProperty] private TvdbSeriesSearchResult? selectedTvdbSeries;
    [ObservableProperty] private string renameStatusText = "Ready";
    [ObservableProperty] private bool isRenamePreviewDirty;
    [ObservableProperty] private bool isRenamePreviewCompactView;

    [ObservableProperty] private string episodeScopeSummary = string.Empty;

    [ObservableProperty] private bool propKeepContainerTitle = true;
    [ObservableProperty] private bool propContainerTitleFromFile;
    [ObservableProperty] private bool propContainerTitleCustom;
    [ObservableProperty] private bool propRemoveContainerTitle;
    [ObservableProperty] private string propCustomContainerTitle = string.Empty;

    [ObservableProperty] private bool propKeepVideoTitle = true;
    [ObservableProperty] private bool propVideoTitleFromFile;
    [ObservableProperty] private bool propVideoTitleCustom;
    [ObservableProperty] private bool propRemoveVideoTitle;
    [ObservableProperty] private string propCustomVideoTitle = string.Empty;

    [ObservableProperty] private string selectedDefaultAudio = "Keep existing";
    [ObservableProperty] private string selectedForcedAudio = "Keep existing";
    [ObservableProperty] private string selectedDefaultSubtitle = "Keep existing";
    [ObservableProperty] private string selectedForcedSubtitle = "Keep existing";
    [ObservableProperty] private string propEditTemplateFilePath = string.Empty;
    [ObservableProperty] private string propEditTemplateFileName = string.Empty;

    partial void OnPropEditTemplateFilePathChanged(string value)
    {
        PropEditTemplateFileName = string.IsNullOrWhiteSpace(value)
            ? string.Empty
            : System.IO.Path.GetFileName(value);
    }

    public bool IsDashboardSection => CurrentSection == "Dashboard";
    public bool IsRenameSection => CurrentSection == "Rename";
    public bool IsMergeSection => CurrentSection == "Mux / Remux";
    public bool IsPropertiesSection => CurrentSection == "Properties";
    public bool IsLibraryAuditSection => CurrentSection == "Library";
    public bool IsSettingsSection => CurrentSection == "Settings";
    public bool IsLogsSection => CurrentSection == "Logs";

    public string CurrentSectionDescription => CurrentSection switch
    {
        "Dashboard" => "Scan folders, compare media properties, and inspect tracks.",
        "Rename" => "Match files to provider metadata and preview safe destination names.",
        "Mux / Remux" => "Plan muxing, remux operations, language keeps, subtitle tools, and track removal.",
        "Properties" => "Edit container, track title, language, default, and forced flags.",
        "Library" => "Browse cached watch-folder coverage, season groups, and items that may need attention.",
        "Settings" => "Manage tool paths, providers, presets, cache, and application behavior.",
        "Logs" => "Review application and operation output.",
        _ => "Ready"
    };

    partial void OnCurrentSectionChanged(string value)
    {
        OnPropertyChanged(nameof(IsDashboardSection));
        OnPropertyChanged(nameof(IsRenameSection));
        OnPropertyChanged(nameof(IsMergeSection));
        OnPropertyChanged(nameof(IsPropertiesSection));
        OnPropertyChanged(nameof(IsLibraryAuditSection));
        OnPropertyChanged(nameof(IsSettingsSection));
        OnPropertyChanged(nameof(IsLogsSection));
        OnPropertyChanged(nameof(CurrentSectionDescription));
    }

    [RelayCommand]
    private void SelectSection(string? section)
    {
        if (!string.IsNullOrWhiteSpace(section))
        {
            CurrentSection = section;
        }
    }

    [RelayCommand]
    private void ToggleRenameSummaryExpanded()
    {
        IsRenameSummaryExpanded = !IsRenameSummaryExpanded;
    }

    [RelayCommand]
    private void ToggleRenamePreviewView()
    {
        IsRenamePreviewCompactView = !IsRenamePreviewCompactView;
    }

    [RelayCommand]
    private async Task OpenRenameUndoBatch()
    {
        if (ShowRenameUndoWindowAsync is null)
        {
            RenameLog("Undo Batch window is not available.");
            return;
        }

        var batches = _renameBatchHistory.Load();
        await ShowRenameUndoWindowAsync(batches, ClearRenameBatchHistory, PreviewRenameBatchUndo, UndoRenameBatchAsync);
    }

    private void ClearRenameBatchHistory()
    {
        _renameBatchHistory.Clear();
        RenameLog("Cleared rename undo batch history.");
    }

    [RelayCommand]
    private void ToggleMergeSummaryExpanded()
    {
        IsMergeSummaryExpanded = !IsMergeSummaryExpanded;
    }

    [RelayCommand]
    private void TogglePropEditSummaryExpanded()
    {
        IsPropEditSummaryExpanded = !IsPropEditSummaryExpanded;
    }

    [RelayCommand]
    private void ToggleLibraryAuditSummaryExpanded()
    {
        IsLibraryAuditSummaryExpanded = !IsLibraryAuditSummaryExpanded;
    }
}
