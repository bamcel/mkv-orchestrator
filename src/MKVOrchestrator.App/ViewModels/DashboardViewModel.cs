using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Windows.Input;
using CommunityToolkit.Mvvm.ComponentModel;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.App.ViewModels;

public sealed class DashboardViewModel : ObservableObject, IDisposable
{
    private readonly MainWindowViewModel _shell;

    public DashboardViewModel(MainWindowViewModel shell)
    {
        _shell = shell;
        _shell.PropertyChanged += OnShellPropertyChanged;
    }

    public bool IsDashboardSection => _shell.IsDashboardSection;
    public bool IsBusy => _shell.IsBusy;
    public bool IsSummaryExpanded => _shell.IsSummaryExpanded;
    public bool HasDashboardFiles => _shell.HasDashboardFiles;
    public bool HasNoDashboardFiles => _shell.HasNoDashboardFiles;
    public ObservableCollection<MkvFileItem> Files => _shell.Files;
    public ObservableCollection<MkvTrackItem> SelectedTracks => _shell.SelectedTracks;
    public ObservableCollection<string> DashboardConsoleLines => _shell.DashboardConsoleLines;

    public string FolderPath
    {
        get => _shell.FolderPath;
        set => _shell.FolderPath = value;
    }

    public string IgnoredScanFolderNameText
    {
        get => _shell.IgnoredScanFolderNameText;
        set => _shell.IgnoredScanFolderNameText = value;
    }

    public MkvFileItem? SelectedFile
    {
        get => _shell.SelectedFile;
        set => _shell.SelectedFile = value;
    }

    public ICommand BrowseFolderCommand => _shell.BrowseFolderCommand;
    public ICommand ScanCommand => _shell.ScanCommand;
    public ICommand CancelCommand => _shell.CancelCommand;
    public ICommand ToggleSummaryExpandedCommand => _shell.ToggleSummaryExpandedCommand;
    public ICommand UseSelectedFileAsTemplateCommand => _shell.UseSelectedFileAsTemplateCommand;

    public void LogDashboardMessage(string message) => _shell.LogDashboardMessage(message);
    public Task ScanDroppedFolderAsync(string folderPath) => _shell.ScanDroppedFolderAsync(folderPath);
    public void RemoveDashboardFiles(IReadOnlyList<MkvFileItem> files) => _shell.RemoveDashboardFiles(files);
    public void RestoreDashboardSelectionAfterGridMutation() => _shell.RestoreDashboardSelectionAfterGridMutation();

    private void OnShellPropertyChanged(object? sender, PropertyChangedEventArgs e) => OnPropertyChanged(e.PropertyName);

    public void Dispose() => _shell.PropertyChanged -= OnShellPropertyChanged;
}
