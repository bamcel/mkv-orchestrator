using System.Diagnostics;
using Avalonia.Controls;
using Avalonia.Platform.Storage;
using MKVOrchestrator.App.Views;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.Services;

public sealed class AvaloniaUserInteractionService(Window owner) : IUserInteractionService
{
    public async Task<IReadOnlyList<string>> PickFoldersAsync(string title, bool allowMultiple, string? suggestedPath = null)
    {
        var options = new FolderPickerOpenOptions
        {
            Title = title,
            AllowMultiple = allowMultiple
        };

        if (!string.IsNullOrWhiteSpace(suggestedPath))
        {
            options.SuggestedStartLocation = await owner.StorageProvider.TryGetFolderFromPathAsync(suggestedPath);
        }

        var folders = await owner.StorageProvider.OpenFolderPickerAsync(options);
        return folders
            .Select(folder => folder.Path.LocalPath)
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .ToList();
    }

    public async Task<string?> PickExecutableAsync(string title)
    {
        var files = await owner.StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = title,
            AllowMultiple = false,
            FileTypeFilter =
            [
                new FilePickerFileType("Executable files") { Patterns = ["*.exe"] },
                FilePickerFileTypes.All
            ]
        });

        return files.Count > 0 ? files[0].Path.LocalPath : null;
    }

    public Task CopyTextAsync(string text) => owner.Clipboard?.SetTextAsync(text) ?? Task.CompletedTask;

    public bool OpenUrl(string url)
    {
        try
        {
            Process.Start(new ProcessStartInfo { FileName = url, UseShellExecute = true });
            return true;
        }
        catch
        {
            return false;
        }
    }

    public async Task<bool> ConfirmSkipConflictsAsync(IReadOnlyList<FileConflictResult> conflicts)
    {
        var dialog = new ExecutionConflictDialog(conflicts);
        return await dialog.ShowDialog<bool?>(owner) == true;
    }

    public void ShowOutput(string title, IReadOnlyList<string> lines)
    {
        var dialog = new OutputWindow(title, lines);
        _ = dialog.ShowDialog(owner);
    }

    public Task ShowRenameUndoAsync(
        IReadOnlyList<RenameBatchRecord> batches,
        Action clearBatches,
        Func<RenameBatchRecord, RenameBatchUndoPreview> previewUndoBatch,
        Func<RenameBatchRecord, Task<RenameBatchUndoResult>> undoBatchAsync)
    {
        var dialog = new RenameUndoBatchDialog(batches, clearBatches, previewUndoBatch, undoBatchAsync);
        return dialog.ShowDialog(owner);
    }
}
