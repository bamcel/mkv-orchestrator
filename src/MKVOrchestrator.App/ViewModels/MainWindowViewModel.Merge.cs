using System;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Text;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{
    public ObservableCollection<string> MkvMergePlanLines { get; } = new();

    [ObservableProperty] private bool mergeRemoveUnwantedAudioLanguages;
    [ObservableProperty] private bool mergeRemoveUnwantedSubtitleLanguages;
    [ObservableProperty] private bool mergeRemoveUnwantedTrackIds;
    [ObservableProperty] private bool mergePreserveChapters = true;
    [ObservableProperty] private bool mergePreserveAttachments = true;
    [ObservableProperty] private bool mergeUseSafeTempReplacement = true;
    [ObservableProperty] private string mergeKeepAudioLanguages = "eng,jpn";
    [ObservableProperty] private string mergeKeepSubtitleLanguages = "eng";
    [ObservableProperty] private string mergeRemoveTrackIdsText = string.Empty;
    [ObservableProperty] private bool mergeMuxMatchingExternalSubtitles;
    [ObservableProperty] private string mergeExternalSubtitleLanguage = "eng";
    [ObservableProperty] private string mergeExternalSubtitleFormat = "srt,ass,ssa,sub,idx";
    [ObservableProperty] private bool mergePreserveExternalSubtitleFiles = true;
    [ObservableProperty] private bool mergeSkipMuxIfSubtitleAlreadyExists = true;
    [ObservableProperty] private bool mergeExtractSubtitles;
    [ObservableProperty] private string mergeExtractSubtitleLanguages = "eng";
    [ObservableProperty] private bool mergeExtractOverwriteExistingFiles;

    [RelayCommand]
    private void BuildMkvMergePreview()
    {
        var plan = BuildMkvMergePlan();
        BuildMkvMergeSummary(plan, dryRun: true, completed: 0, failed: 0);
        Log($"mkvmerge dry run: {plan.Actions.Count} planned, {plan.NoChangeFiles.Count} no change.");
    }

    [RelayCommand]
    private async Task ExecuteMkvMerge()
    {
        if (string.IsNullOrWhiteSpace(MkvMergePath))
        {
            AddMkvMergeLine("Configure the MKVToolNix folder in Settings or ensure mkvmerge is available on PATH.");
            return;
        }

        var plan = BuildMkvMergePlan();
        if (plan.Actions.Count == 0)
        {
            BuildMkvMergeSummary(plan, dryRun: false, completed: 0, failed: 0);
            return;
        }

        _cts?.Cancel();
        _cts = new CancellationTokenSource();
        IsBusy = true;
        BeginGlobalOperation("mkvmerge/subtitle tools", plan.Actions.Count);

        var completed = 0;
        var failed = 0;
        var skipped = 0;
        var jobs = plan.Actions.Select(action => CreateExecutionJob(action.ToolName, action.SourceFilePath, action.Description)).ToList();
        BeginExecutionWorkflow("mkvmerge", jobs);

        try
        {
            var conflictChecks = plan.Actions
                .Select((action, index) => new ExecutionConflictCheck(jobs[index], action.SourceFilePath, null, RenameCheck: false))
                .ToList();
            if (!await ConfirmOrCancelForConflictsAsync(conflictChecks, AddMkvMergeLine))
            {
                StatusText = "mkvmerge canceled because conflicts were detected.";
                CompleteExecutionWorkflow(StatusText);
                return;
            }

            for (var i = 0; i < plan.Actions.Count; i++)
            {
                var action = plan.Actions[i];
                var job = jobs[i];
                if (job.Status == ExecutionJobStatus.Skipped)
                {
                    skipped++;
                    var skippedFile = Files.FirstOrDefault(f => string.Equals(f.FilePath, action.SourceFilePath, StringComparison.OrdinalIgnoreCase));
                    if (skippedFile is not null) skippedFile.Status = "Skipped - locked/busy";
                    continue;
                }

                _executionQueue.MarkRunning(job);
                RefreshExecutionSummary();
                var fileName = Path.GetFileName(action.SourceFilePath);
                UpdateGlobalOperation(i + 1, plan.Actions.Count, fileName);
                AddMkvMergeLine($"EXECUTE {action.ToolName}: {fileName}");
                AddMkvMergeLine("  " + action.Description);
                AddMkvMergeLine("  Progress: 0%");

                var result = await _mkvMerge.ExecuteRemuxAsync(
                    MkvMergePath,
                    action,
                    percent => Dispatcher.UIThread.Post(() =>
                    {
                        _executionQueue.UpdateProgress(job, percent);
                        RefreshExecutionSummary();

                        var progressText = $"{action.ToolName} {i + 1}/{plan.Actions.Count}: {percent}% - {fileName}";
                        StatusText = progressText;
                        ExecutionStatusText = $"Execution Center: {progressText}";
                    }),
                    _cts.Token);

                if (result.ExitCode == 0)
                {
                    completed++;
                    AddMkvMergeLine("  SUCCESS");
                    _executionQueue.Complete(job, "SUCCESS");
                    var file = Files.FirstOrDefault(f => string.Equals(f.FilePath, action.SourceFilePath, StringComparison.OrdinalIgnoreCase));
                    if (file is not null) file.Status = "Remuxed - refresh pending";
                    await RefreshFileMediaInfoAsync(action.SourceFilePath, _cts.Token);
                }
                else
                {
                    failed++;
                    var error = string.IsNullOrWhiteSpace(result.StandardError) ? $"FAILED exit code {result.ExitCode}" : result.StandardError.Trim();
                    AddMkvMergeLine("  FAILED: " + error);
                    _executionQueue.Fail(job, error);
                    var file = Files.FirstOrDefault(f => string.Equals(f.FilePath, action.SourceFilePath, StringComparison.OrdinalIgnoreCase));
                    if (file is not null) file.Status = "Remux failed";
                }
                RefreshExecutionSummary();
            }

            EvaluateTrackTemplateDeviations();
            BuildDashboardMismatchReport();
            SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: false);
            BuildMkvMergeSummary(plan, dryRun: false, completed: completed, failed: failed);
            CompleteGlobalOperation($"mkvmerge complete: {completed} completed, {failed} failed, {skipped} skipped, {plan.NoChangeFiles.Count} no change");
            CompleteExecutionWorkflow(StatusText);
        }
        catch (OperationCanceledException)
        {
            StatusText = "mkvmerge canceled";
            AddMkvMergeLine(StatusText);
            _executionQueue.CancelPending(StatusText);
            RefreshExecutionSummary();
        }
        catch (Exception ex)
        {
            StatusText = "mkvmerge failed";
            AddMkvMergeLine("FAILED: " + ex.Message);
        }
        finally
        {
            IsBusy = false;
        }
    }

    private MkvMergeRemuxPlan BuildMkvMergePlan()
    {
        return _mkvMerge.BuildRemuxPlan(
            Files,
            MergeKeepAudioLanguages,
            MergeKeepSubtitleLanguages,
            MergeRemoveUnwantedAudioLanguages,
            MergeRemoveUnwantedSubtitleLanguages,
            MergeRemoveUnwantedTrackIds,
            MergeRemoveTrackIdsText,
            MergePreserveChapters,
            MergePreserveAttachments,
            MergeUseSafeTempReplacement,
            MergeMuxMatchingExternalSubtitles,
            MergeExternalSubtitleLanguage,
            "{tag}",
            MergeExternalSubtitleFormat,
            MergePreserveExternalSubtitleFiles,
            MergeSkipMuxIfSubtitleAlreadyExists,
            MergeExtractSubtitles,
            MergeExtractSubtitleLanguages,
            MergeExtractOverwriteExistingFiles);
    }

    private void BuildMkvMergeSummary(MkvMergeRemuxPlan plan, bool dryRun, int completed, int failed)
    {
        MkvMergePlanLines.Clear();
        var selectedCount = Files.Count(f => f.Selected);
        var mode = dryRun ? "DRY RUN" : "EXECUTION";

        AddMkvMergeLine($"mkvmerge Summary - {mode}");
        AddMkvMergeLine($"Generated: {DateTime.Now:yyyy-MM-dd HH:mm:ss}");
        AddMkvMergeLine($"Selected files: {selectedCount} | Planned remux: {plan.Actions.Count} | No Change: {plan.NoChangeFiles.Count}");
        if (!dryRun)
        {
            AddMkvMergeLine($"Completed: {completed} | Failed: {failed}");
        }
        AddMkvMergeLine(new string('=', 92));

        if (plan.NoChangeFiles.Count > 0)
        {
            AddMkvMergeLine("NO CHANGE FILES:");
            foreach (var filePath in plan.NoChangeFiles)
            {
                AddMkvMergeLine("  - " + Path.GetFileName(filePath));
            }
            AddMkvMergeLine(new string('=', 92));
        }

        for (var index = 0; index < plan.Actions.Count; index++)
        {
            var action = plan.Actions[index];
            AddMkvMergeLine($"FILE {index + 1}/{plan.Actions.Count}: {Path.GetFileName(action.SourceFilePath)}");
            AddMkvMergeLine("CHANGES:");
            foreach (var change in SplitDescription(action.Description))
            {
                AddMkvMergeLine("  - " + change);
            }
            AddMkvMergeLine("COMMAND:");
            var toolDisplay = action.ToolName.Equals("mkvextract", StringComparison.OrdinalIgnoreCase)
                ? "mkvextract"
                : MkvMergePath;
            AddMkvMergeLine("  " + FormatCommand(toolDisplay, action.Arguments));
            AddMkvMergeLine(dryRun ? "RESULT: DRY RUN | Command not executed" : "RESULT: See execution status above");
            AddMkvMergeLine(new string('=', 92));
        }
    }

    private void AddMkvMergeLine(string line)
    {
        MkvMergePlanLines.Add(line);
    }
}
