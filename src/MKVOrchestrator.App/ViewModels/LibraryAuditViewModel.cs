using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Windows.Input;
using CommunityToolkit.Mvvm.ComponentModel;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.App.ViewModels;

/// <summary>
/// Panel-scoped binding surface. Behavior remains delegated to the shell during the
/// incremental migration, while the view no longer depends on the full shell contract.
/// </summary>
public sealed class LibraryAuditViewModel : ObservableObject, IDisposable
{
    private readonly MainWindowViewModel _shell;

    public LibraryAuditViewModel(MainWindowViewModel shell)
    {
        _shell = shell;
        _shell.PropertyChanged += OnShellPropertyChanged;
    }

    public bool IsLibraryAuditSection => _shell.IsLibraryAuditSection;
    public ObservableCollection<string> LibraryAuditWatchFolderOptions => _shell.LibraryAuditWatchFolderOptions;
    public string SelectedLibraryAuditWatchFolder
    {
        get => _shell.SelectedLibraryAuditWatchFolder;
        set => _shell.SelectedLibraryAuditWatchFolder = value;
    }

    public ObservableCollection<LibraryAuditSeasonItem> DisplayedLibraryAuditItems => _shell.DisplayedLibraryAuditItems;
    public LibraryAuditSeasonItem? SelectedLibraryAuditItem
    {
        get => _shell.SelectedLibraryAuditItem;
        set => _shell.SelectedLibraryAuditItem = value;
    }

    public ObservableCollection<LibraryAuditIssueLine> SelectedLibraryAuditIssueLines => _shell.SelectedLibraryAuditIssueLines;
    public bool IsLibraryAuditBusy => _shell.IsLibraryAuditBusy;
    public bool IsLibraryAuditSummaryExpanded => _shell.IsLibraryAuditSummaryExpanded;
    public string LibraryWarningFilterButtonText => _shell.LibraryWarningFilterButtonText;
    public string LibraryAuditDetailSummary => _shell.LibraryAuditDetailSummary;
    public string LibraryAuditStatusText => _shell.LibraryAuditStatusText;

    public ICommand RefreshLibraryAuditWatchFoldersCommand => _shell.RefreshLibraryAuditWatchFoldersCommand;
    public ICommand RunLibraryAuditCommand => _shell.RunLibraryAuditCommand;
    public ICommand SendSelectedAuditIssuesToDashboardCommand => _shell.SendSelectedAuditIssuesToDashboardCommand;
    public ICommand ToggleLibraryWarningsOnlyCommand => _shell.ToggleLibraryWarningsOnlyCommand;
    public ICommand ToggleLibraryAuditSummaryExpandedCommand => _shell.ToggleLibraryAuditSummaryExpandedCommand;
    public ICommand CancelCommand => _shell.CancelCommand;

    private void OnShellPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        OnPropertyChanged(e.PropertyName);
    }

    public void Dispose()
    {
        _shell.PropertyChanged -= OnShellPropertyChanged;
    }
}
