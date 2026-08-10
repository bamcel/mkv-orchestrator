using System.Collections.ObjectModel;
using MKVOrchestrator.App.Services;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.Workflows;

public sealed record ExecutionConflictCheck(ExecutionJob Job, string SourcePath, string? TargetPath, bool RenameCheck);

public sealed record ConflictReviewResult(bool Proceed, string StatusText);

public sealed class ExecutionWorkflowCoordinator(
    ExecutionQueueService queue,
    FileConflictService conflicts,
    IUserInteractionService userInteraction)
{
    public ExecutionQueueService Queue { get; } = queue;
    public ObservableCollection<ExecutionJob> Jobs => Queue.Jobs;
    public IEnumerable<string> SummaryLines => Queue.CurrentSummary.ToConsoleLines();

    public async Task<ConflictReviewResult> ReviewConflictsAsync(
        IEnumerable<ExecutionConflictCheck> checks,
        Action<string> writeLine)
    {
        var blocked = checks
            .Select(check => new
            {
                Check = check,
                Conflict = check.RenameCheck && !string.IsNullOrWhiteSpace(check.TargetPath)
                    ? conflicts.CheckRenameTarget(check.SourcePath, check.TargetPath!)
                    : conflicts.CheckReadableWritable(check.SourcePath, requireWrite: true)
            })
            .Where(item => !item.Conflict.CanProceed)
            .ToList();

        if (blocked.Count == 0) return new ConflictReviewResult(true, string.Empty);

        writeLine($"WARNING: {blocked.Count} file conflict(s) detected before execution.");
        foreach (var item in blocked.Take(10))
        {
            writeLine($"  CONFLICT: {Path.GetFileName(item.Check.SourcePath)} - {item.Conflict.Reason}");
        }

        if (blocked.Count > 10) writeLine($"  ... plus {blocked.Count - 10} more conflict(s).");

        if (!await userInteraction.ConfirmSkipConflictsAsync(blocked.Select(item => item.Conflict).ToList()))
        {
            Queue.CancelPending("Canceled by user after conflict warning.");
            writeLine("RUN CANCELED: conflict warning was not accepted.");
            return new ConflictReviewResult(false, "Execution Center: canceled because conflicts were detected");
        }

        foreach (var item in blocked)
        {
            Queue.Skip(item.Check.Job, item.Conflict.Reason);
            writeLine($"SKIPPED CONFLICT: {Path.GetFileName(item.Check.SourcePath)} - {item.Conflict.Reason}");
        }

        return new ConflictReviewResult(true, $"Execution Center: skipping {blocked.Count} conflict(s)");
    }

    public ExecutionSummary Begin(string workflow, IEnumerable<ExecutionJob> jobs) =>
        Queue.BeginWorkflow(workflow, jobs.ToList());

    public ExecutionSummary Complete() => Queue.CompleteWorkflow();

    public static ExecutionJob CreateJob(string workflow, string filePath, string description) => new()
    {
        Workflow = workflow,
        FilePath = filePath,
        Description = description
    };

    public bool TryPassFileConflictCheck(
        ExecutionJob job,
        string sourcePath,
        string? targetPath,
        bool renameCheck,
        Action<string> writeLine)
    {
        var conflict = renameCheck && !string.IsNullOrWhiteSpace(targetPath)
            ? conflicts.CheckRenameTarget(sourcePath, targetPath)
            : conflicts.CheckReadableWritable(sourcePath, requireWrite: true);

        if (conflict.CanProceed) return true;

        Queue.Skip(job, conflict.Reason);
        writeLine($"SKIPPED: {Path.GetFileName(sourcePath)} - {conflict.Reason}");
        return false;
    }
}
