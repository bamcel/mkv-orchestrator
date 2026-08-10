using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Models;
using Workflows = MKVOrchestrator.App.Workflows;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{

    private sealed record ExecutionConflictCheck(ExecutionJob Job, string SourcePath, string? TargetPath, bool RenameCheck);

    private async Task<bool> ConfirmOrCancelForConflictsAsync(IEnumerable<ExecutionConflictCheck> checks, Action<string> writeLine)
    {
        var result = await _executionWorkflow.ReviewConflictsAsync(
            checks.Select(check => new Workflows.ExecutionConflictCheck(
                check.Job,
                check.SourcePath,
                check.TargetPath,
                check.RenameCheck)),
            writeLine);
        if (!string.IsNullOrWhiteSpace(result.StatusText)) ExecutionStatusText = result.StatusText;
        RefreshExecutionSummary();
        return result.Proceed;
    }

    private ExecutionSummary BeginExecutionWorkflow(string workflow, IEnumerable<ExecutionJob> jobs)
    {
        var summary = _executionWorkflow.Begin(workflow, jobs);
        ExecutionSummaryLines.Clear();
        ExecutionStatusText = $"Execution Center: {workflow} queued ({summary.Total} job(s))";
        foreach (var line in summary.ToConsoleLines()) ExecutionSummaryLines.Add(line);
        return summary;
    }

    private void RefreshExecutionSummary()
    {
        ExecutionSummaryLines.Clear();
        foreach (var line in _executionWorkflow.SummaryLines)
        {
            ExecutionSummaryLines.Add(line);
        }
    }

    private void CompleteExecutionWorkflow(string statusText)
    {
        var summary = _executionWorkflow.Complete();
        ExecutionStatusText = $"Execution Center: {statusText}";
        RefreshExecutionSummary();
        Log($"Execution Center: {summary.Workflow} complete - total {summary.Total}, completed {summary.Completed}, failed {summary.Failed}, skipped {summary.Skipped}, canceled {summary.Canceled}.");
    }

    private ExecutionJob CreateExecutionJob(string workflow, string filePath, string description)
    {
        return Workflows.ExecutionWorkflowCoordinator.CreateJob(workflow, filePath, description);
    }

    private bool TryPassFileConflictCheck(ExecutionJob job, string sourcePath, string? targetPath, bool renameCheck, Action<string> writeLine)
    {
        var passed = _executionWorkflow.TryPassFileConflictCheck(
            job,
            sourcePath,
            targetPath,
            renameCheck,
            writeLine);
        if (passed) return true;
        RefreshExecutionSummary();
        return false;
    }
}
