using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Linq;
using CommunityToolkit.Mvvm.ComponentModel;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services.Metadata;

namespace MKVOrchestrator.App.ViewModels;

public partial class MediaServerItemViewModel : ObservableObject
{
    private static readonly HashSet<string> TransientPropertyNames = new(StringComparer.Ordinal)
    {
        nameof(StatusText),
        nameof(IsBusy),
        nameof(LastSyncedText),
        nameof(HasLibraries)
    };

    public MediaServerItemViewModel(MediaServerSettings settings)
    {
        Id = string.IsNullOrWhiteSpace(settings.Id) ? Guid.NewGuid().ToString("N") : settings.Id;
        name = settings.Name ?? string.Empty;
        serverType = MediaServerDiscoveryService.NormalizeServerType(settings.Type);
        serverUrl = settings.ServerUrl ?? string.Empty;
        apiKey = settings.ApiKey ?? string.Empty;
        isDefault = settings.IsDefault;
        lastSyncedUtc = settings.LastSyncedUtc;
        foreach (var library in settings.Libraries ?? new List<MediaServerLibraryPath>())
        {
            AddLibrary(library);
        }
    }

    public string Id { get; }

    [ObservableProperty] private string name = string.Empty;
    [ObservableProperty] private string serverType = "Emby";
    [ObservableProperty] private string serverUrl = string.Empty;
    [ObservableProperty] private string apiKey = string.Empty;
    [ObservableProperty] private bool isDefault;
    [ObservableProperty] private DateTimeOffset? lastSyncedUtc;
    [ObservableProperty] private string statusText = string.Empty;
    [ObservableProperty] private bool isBusy;

    public ObservableCollection<MediaServerLibraryItemViewModel> Libraries { get; } = new();

    /// <summary>Raised whenever a persisted value changes, so the owner can save settings.</summary>
    public event EventHandler<string?>? SettingsChanged;

    public bool HasLibraries => Libraries.Count > 0;

    public string LastSyncedText => LastSyncedUtc is { } synced
        ? $"Last synced: {synced.ToLocalTime():yyyy-MM-dd HH:mm}"
        : "Not synced yet.";

    public MediaServerSettings ToSettings() => new()
    {
        Id = Id,
        Name = Name?.Trim() ?? string.Empty,
        Type = MediaServerDiscoveryService.NormalizeServerType(ServerType),
        ServerUrl = ServerUrl?.Trim() ?? string.Empty,
        ApiKey = ApiKey?.Trim() ?? string.Empty,
        IsDefault = IsDefault,
        LastSyncedUtc = LastSyncedUtc,
        Libraries = Libraries.Select(library => library.ToSettings()).ToList()
    };

    public void ReplaceLibraries(IEnumerable<MediaServerLibraryPath> libraries)
    {
        foreach (var library in Libraries)
        {
            library.PropertyChanged -= OnLibraryPropertyChanged;
        }

        Libraries.Clear();
        foreach (var library in libraries)
        {
            AddLibrary(library);
        }

        OnPropertyChanged(nameof(HasLibraries));
    }

    private void AddLibrary(MediaServerLibraryPath library)
    {
        var item = new MediaServerLibraryItemViewModel(library);
        item.PropertyChanged += OnLibraryPropertyChanged;
        Libraries.Add(item);
    }

    private void OnLibraryPropertyChanged(object? sender, PropertyChangedEventArgs e)
        => SettingsChanged?.Invoke(this, nameof(Libraries));

    partial void OnLastSyncedUtcChanged(DateTimeOffset? value)
        => OnPropertyChanged(nameof(LastSyncedText));

    protected override void OnPropertyChanged(PropertyChangedEventArgs e)
    {
        base.OnPropertyChanged(e);
        if (e.PropertyName is { } propertyName && !TransientPropertyNames.Contains(propertyName))
        {
            SettingsChanged?.Invoke(this, propertyName);
        }
    }
}

public partial class MediaServerLibraryItemViewModel : ObservableObject
{
    public MediaServerLibraryItemViewModel(MediaServerLibraryPath library)
    {
        Id = library.Id ?? string.Empty;
        Name = library.Name ?? string.Empty;
        LibraryType = library.Type ?? string.Empty;
        ServerPath = library.ServerPath ?? string.Empty;
        ContainerPath = library.ContainerPath ?? string.Empty;
        isEnabled = library.IsEnabled;
    }

    public string Id { get; }
    public string Name { get; }
    public string LibraryType { get; }
    public string ServerPath { get; }
    public string ContainerPath { get; }

    [ObservableProperty] private bool isEnabled;

    public string PathToolTip => $"{ServerPath} -> {ContainerPath}";

    public MediaServerLibraryPath ToSettings() => new()
    {
        Id = Id,
        Name = Name,
        Type = LibraryType,
        ServerPath = ServerPath,
        ContainerPath = ContainerPath,
        IsEnabled = IsEnabled
    };
}
