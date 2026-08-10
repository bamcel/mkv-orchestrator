using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.Services;

public interface IUserInteractionService
{
    Task<IReadOnlyList<string>> PickFoldersAsync(string title, bool allowMultiple, string? suggestedPath = null);
    Task<string?> PickExecutableAsync(string title);
    Task CopyTextAsync(string text);
    bool OpenUrl(string url);
    Task<bool> ConfirmSkipConflictsAsync(IReadOnlyList<FileConflictResult> conflicts);
    void ShowOutput(string title, IReadOnlyList<string> lines);
    Task ShowRenameUndoAsync(
        IReadOnlyList<RenameBatchRecord> batches,
        Action clearBatches,
        Func<RenameBatchRecord, RenameBatchUndoPreview> previewUndoBatch,
        Func<RenameBatchRecord, Task<RenameBatchUndoResult>> undoBatchAsync);
}

public sealed class NullUserInteractionService : IUserInteractionService
{
    public static NullUserInteractionService Instance { get; } = new();

    private NullUserInteractionService()
    {
    }

    public Task<IReadOnlyList<string>> PickFoldersAsync(string title, bool allowMultiple, string? suggestedPath = null) =>
        Task.FromResult<IReadOnlyList<string>>(Array.Empty<string>());

    public Task<string?> PickExecutableAsync(string title) => Task.FromResult<string?>(null);
    public Task CopyTextAsync(string text) => Task.CompletedTask;
    public bool OpenUrl(string url) => false;
    public Task<bool> ConfirmSkipConflictsAsync(IReadOnlyList<FileConflictResult> conflicts) => Task.FromResult(false);
    public void ShowOutput(string title, IReadOnlyList<string> lines)
    {
    }

    public Task ShowRenameUndoAsync(
        IReadOnlyList<RenameBatchRecord> batches,
        Action clearBatches,
        Func<RenameBatchRecord, RenameBatchUndoPreview> previewUndoBatch,
        Func<RenameBatchRecord, Task<RenameBatchUndoResult>> undoBatchAsync) => Task.CompletedTask;
}
