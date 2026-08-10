using System.Collections.Generic;
using System.IO;
using System.Linq;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.App.ViewModels;
using MKVOrchestrator.App.Views.Shared;

namespace MKVOrchestrator.App.Views.Dashboard;

public partial class DashboardPanel : UserControl
{
    public DashboardPanel()
    {
        InitializeComponent();
    }


    private void Dashboard_DragOver(object? sender, DragEventArgs e)
    {
        e.DragEffects = DragDropEffects.None;

        if (DataContext is DashboardViewModel { IsBusy: false } && DragEventContainsFolder(e))
        {
            e.DragEffects = DragDropEffects.Copy;
        }

        e.Handled = true;
    }

    private async void Dashboard_Drop(object? sender, DragEventArgs e)
    {
        e.Handled = true;

        if (DataContext is not DashboardViewModel vm)
        {
            return;
        }

        if (vm.IsBusy)
        {
            vm.LogDashboardMessage("Drop ignored because a scan is already running.");
            return;
        }

        var folderPath = GetDroppedFolderPath(e);
        if (string.IsNullOrWhiteSpace(folderPath))
        {
            vm.LogDashboardMessage("Drop a folder onto File Info to scan it.");
            return;
        }

        await vm.ScanDroppedFolderAsync(folderPath);
    }

    private static bool DragEventContainsFolder(DragEventArgs e) =>
        !string.IsNullOrWhiteSpace(GetDroppedFolderPath(e));

    private static string? GetDroppedFolderPath(DragEventArgs e)
    {
        var files = e.Data.GetFiles();
        if (files is null)
        {
            return null;
        }

        foreach (var item in files)
        {
            var path = item.Path.LocalPath;
            if (!string.IsNullOrWhiteSpace(path) && Directory.Exists(path))
            {
                return path;
            }
        }

        return null;
    }

    private void FileGrid_KeyDown(object? sender, KeyEventArgs e)
    {
        if (DataContext is not DashboardViewModel vm)
        {
            return;
        }

        if (e.Key == Key.Delete)
        {
            var selectedFiles = GetSelectedFilesForDelete();
            if (selectedFiles.Count == 0)
            {
                return;
            }

            vm.RemoveDashboardFiles(selectedFiles);
            var nextSelection = vm.SelectedFile;
            ResetFileGridVisualSelection(nextSelection);

            Dispatcher.UIThread.Post(() =>
            {
                ResetFileGridVisualSelection(nextSelection);
                vm.RestoreDashboardSelectionAfterGridMutation();
            });

            e.Handled = true;
            return;
        }

        FileGridSelectionHelper.ToggleSelectedRowsOnSpace(sender, e);
    }


    private void ResetFileGridVisualSelection(MkvFileItem? nextSelection)
    {
        // Keep the Avalonia DataGrid visual selection in sync with the view-model
        // after rows are removed. Extended selection can retain stale selected cells/rows
        // unless the visual selection collection is explicitly reset.
        FileInfoGrid.SelectedItems.Clear();
        FileInfoGrid.SelectedItem = null;

        if (nextSelection is null)
        {
            return;
        }

        FileInfoGrid.SelectedItem = nextSelection;
        FileInfoGrid.SelectedItems.Add(nextSelection);
    }

    private List<MkvFileItem> GetSelectedFilesForDelete()
    {
        var selectedFiles = FileInfoGrid.SelectedItems
            .OfType<MkvFileItem>()
            .Distinct()
            .ToList();

        if (selectedFiles.Count == 0 && FileInfoGrid.SelectedItem is MkvFileItem currentFile)
        {
            selectedFiles.Add(currentFile);
        }

        return selectedFiles;
    }

    private void UseCheckBox_Click(object? sender, RoutedEventArgs e) =>
        FileGridSelectionHelper.ApplyClickedUseValueToSelectedRows(sender, e, FileInfoGrid);
}
