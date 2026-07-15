using System.Collections.Generic;
using System.Linq;
using System;
using System.Collections.ObjectModel;
using System.Text;
using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.Input;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using Avalonia.Threading;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Library;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{
    [RelayCommand]
    private void DryRun()
    {
        PlannedActions.Clear();
        var plan = BuildCurrentPlan();

        if (plan.Actions.Count == 0 && plan.SkippedFiles.Count == 0 && plan.NoChangeFiles.Count == 0)
        {
            AddSummaryLine("No selected files to evaluate.");
            Log("Dry run: no selected files to evaluate.");
            return;
        }

        BuildPropEditSummary(plan, dryRun: true, resultByFile: null);
        Log($"Dry run summary: planned {plan.Actions.Count}, skipped {plan.SkippedFiles.Count}, no change {plan.NoChangeFiles.Count}.");
    }

    [RelayCommand]
    private async Task Apply()
    {
        if (string.IsNullOrWhiteSpace(MkvPropEditPath))
        {
            Log("Select mkvpropedit.exe or enter mkvpropedit if it is in PATH.");
            return;
        }

        var plan = BuildCurrentPlan();
        if (plan.Actions.Count == 0)
        {
            BuildPropEditSummary(plan, dryRun: false, resultByFile: null);
            Log($"Apply skipped: no executable changes. Skipped {plan.SkippedFiles.Count}, no change {plan.NoChangeFiles.Count}.");
            return;
        }

        _cts?.Cancel();
        _cts = new CancellationTokenSource();
        IsBusy = true;
        BeginGlobalOperation("mkvpropedit", plan.Actions.Count);
        Log($"mkvpropedit executing: {plan.Actions.Count} planned file(s).");
        Log($"Using mkvpropedit: {MkvPropEditPath}");

        try
        {
            PlannedActions.Clear();
            var changedFiles = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            var resultByFile = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var completed = 0;
            var failed = 0;
            var skipped = 0;
            var jobs = plan.Actions.Select(action => CreateExecutionJob("mkvpropedit", action.FilePath, action.Description)).ToList();
            BeginExecutionWorkflow("mkvpropedit", jobs);

            var conflictChecks = plan.Actions
                .Select((action, index) => new ExecutionConflictCheck(jobs[index], action.FilePath, null, RenameCheck: false))
                .ToList();
            if (!await ConfirmOrCancelForConflictsAsync(conflictChecks, Log))
            {
                StatusText = "Apply canceled because conflicts were detected.";
                CompleteExecutionWorkflow(StatusText);
                return;
            }

            var editWorkers = _workerSettings.CloneNormalized().MaxEditWorkers;
            Log($"mkvpropedit workers: {editWorkers}");
            var resultGate = new object();
            var completedCounter = 0;
            var failedCounter = 0;
            var skippedCounter = 0;
            var processedCounter = 0;

            await Parallel.ForEachAsync(
                plan.Actions.Select((action, index) => new { action, job = jobs[index], index }),
                new ParallelOptions
                {
                    CancellationToken = _cts.Token,
                    MaxDegreeOfParallelism = editWorkers
                },
                async (work, token) =>
                {
                    var action = work.action;
                    var job = work.job;
                    if (job.Status == ExecutionJobStatus.Skipped)
                    {
                        Interlocked.Increment(ref skippedCounter);
                        lock (resultGate)
                        {
                            resultByFile[action.FilePath] = "SKIPPED: " + job.Result;
                        }
                        return;
                    }

                    var fileName = Path.GetFileName(action.FilePath);
                    await Dispatcher.UIThread.InvokeAsync(() =>
                    {
                        _executionQueue.MarkRunning(job);
                        RefreshExecutionSummary();
                        Log($"APPLY: {fileName}");
                        Log("  " + action.Description);
                    });

                    var result = await _propEdit.ExecuteAsync(MkvPropEditPath, action, token);
                    var processed = Interlocked.Increment(ref processedCounter);

                    if (result.ExitCode == 0)
                    {
                        Interlocked.Increment(ref completedCounter);
                        lock (resultGate)
                        {
                            resultByFile[action.FilePath] = "SUCCESS";
                            changedFiles.Add(action.FilePath);
                        }

                        await Dispatcher.UIThread.InvokeAsync(() =>
                        {
                            UpdateGlobalOperation(processed, plan.Actions.Count, fileName);
                            Log($"  SUCCESS: {fileName}");
                            _executionQueue.Complete(job, "SUCCESS");
                            var file = Files.FirstOrDefault(f => f.FilePath == action.FilePath);
                            if (file is not null) file.Status = "Updated - refresh pending";
                            RefreshExecutionSummary();
                        });
                    }
                    else
                    {
                        Interlocked.Increment(ref failedCounter);
                        var error = string.IsNullOrWhiteSpace(result.StandardError) ? $"FAILED exit code {result.ExitCode}" : result.StandardError.Trim();
                        lock (resultGate)
                        {
                            resultByFile[action.FilePath] = error;
                        }

                        await Dispatcher.UIThread.InvokeAsync(() =>
                        {
                            UpdateGlobalOperation(processed, plan.Actions.Count, fileName);
                            Log($"  FAILED: {fileName} - {error}");
                            _executionQueue.Fail(job, error);
                            var file = Files.FirstOrDefault(f => f.FilePath == action.FilePath);
                            if (file is not null) file.Status = "Failed";
                            RefreshExecutionSummary();
                        });
                    }
                });

            completed = completedCounter;
            failed = failedCounter;
            skipped = skippedCounter;

            BuildPropEditSummary(plan, dryRun: false, resultByFile);
            AddSummaryLine(string.Empty);
            AddSummaryLine($"Completed: {completed} | Failed: {failed} | Skipped: {plan.SkippedFiles.Count + skipped} | No Change: {plan.NoChangeFiles.Count} | Planned: {plan.Actions.Count}");

            if (changedFiles.Count > 0)
            {
                Log($"Refreshing media info for {changedFiles.Count} changed file(s)...");
                foreach (var filePath in changedFiles)
                {
                    await RefreshFileMediaInfoAsync(filePath, _cts.Token);
                }
                EvaluateTrackTemplateDeviations();
                BuildDashboardMismatchReport();
                Log("Media info refresh complete.");
            }

            CompleteGlobalOperation($"mkvpropedit complete: {completed} completed, {failed} failed, {plan.SkippedFiles.Count + skipped} skipped, {plan.NoChangeFiles.Count} no change");
            Log(StatusText);
            CompleteExecutionWorkflow(StatusText);
        }
        catch (OperationCanceledException)
        {
            StatusText = "Apply canceled";
            Log(StatusText);
            _executionQueue.CancelPending(StatusText);
            RefreshExecutionSummary();
        }
        finally
        {
            IsBusy = false;
        }
    }

    private async Task RefreshFileMediaInfoAsync(string filePath, CancellationToken token)
    {
        var index = -1;
        for (var i = 0; i < Files.Count; i++)
        {
            if (string.Equals(Files[i].FilePath, filePath, StringComparison.OrdinalIgnoreCase))
            {
                index = i;
                break;
            }
        }

        if (index < 0) return;

        var wasSelected = ReferenceEquals(SelectedFile, Files[index])
            || string.Equals(SelectedFile?.FilePath, filePath, StringComparison.OrdinalIgnoreCase);
        var wasChecked = Files[index].Selected;
        var previous = Files[index];

        try
        {
            var refreshedMedia = await GetMediaLibraryForPath(filePath).ScanFileAsync(filePath, MkvMergePath, FfProbePath, token, forceRefresh: true);
            var refreshed = MkvFileItem.FromMediaFile(refreshedMedia);
            PreserveTechnicalMetadataIfMissing(previous, refreshed);
            UpsertMediaCacheForPath(refreshed.ToMediaFile());
            refreshed.Selected = wasChecked;
            refreshed.Status = "Updated / refreshed";
            Files[index] = refreshed;

            if (wasSelected)
            {
                SelectedFile = refreshed;
            }

            Log($"  Refreshed: {refreshed.FileName} [{refreshed.Codec} | {refreshed.Resolution} | {refreshed.BitDepth}]");
        }
        catch (Exception ex)
        {
            Files[index].Status = "Updated / refresh failed";
            Log($"  Refresh failed for {Path.GetFileName(filePath)}: {ex.Message}");
        }
    }

    private static void PreserveTechnicalMetadataIfMissing(MkvFileItem previous, MkvFileItem refreshed)
    {
        if (IsMissingMediaValue(refreshed.Resolution)) refreshed.Resolution = previous.Resolution;
        if (IsMissingMediaValue(refreshed.Codec)) refreshed.Codec = previous.Codec;
        if (IsMissingMediaValue(refreshed.BitDepth)) refreshed.BitDepth = previous.BitDepth;
        if (IsMissingMediaValue(refreshed.Hdr)) refreshed.Hdr = previous.Hdr;
        if (IsMissingMediaValue(refreshed.VideoSummary)) refreshed.VideoSummary = previous.VideoSummary;
        if (IsMissingMediaValue(refreshed.AudioSummary)) refreshed.AudioSummary = previous.AudioSummary;
        if (IsMissingMediaValue(refreshed.SubtitleSummary)) refreshed.SubtitleSummary = previous.SubtitleSummary;
        if (IsMissingMediaValue(refreshed.AttachmentSummary)) refreshed.AttachmentSummary = previous.AttachmentSummary;

        refreshed.CanonicalMedia.Metadata.Resolution = refreshed.Resolution;
        refreshed.CanonicalMedia.Metadata.Codec = refreshed.Codec;
        refreshed.CanonicalMedia.Metadata.BitDepth = refreshed.BitDepth;
        refreshed.CanonicalMedia.Metadata.Hdr = refreshed.Hdr;
        refreshed.CanonicalMedia.Metadata.VideoSummary = refreshed.VideoSummary;
        refreshed.CanonicalMedia.Metadata.AudioSummary = refreshed.AudioSummary;
        refreshed.CanonicalMedia.Metadata.SubtitleSummary = refreshed.SubtitleSummary;
        refreshed.CanonicalMedia.Metadata.AttachmentSummary = refreshed.AttachmentSummary;
    }

    private static bool IsMissingMediaValue(string? value)
        => string.IsNullOrWhiteSpace(value)
           || value.Equals("Unknown", StringComparison.OrdinalIgnoreCase)
           || value.Equals("unknown", StringComparison.OrdinalIgnoreCase);

    private MediaLibraryService GetMediaLibraryForPath(string filePath)
        => IsPathUnderAnyWatchFolder(filePath) ? _mediaLibrary : _tempMediaLibrary;

    private void UpsertMediaCacheForPath(MediaFile media)
    {
        if (IsPathUnderAnyWatchFolder(media.FilePath)) _mediaCache.Upsert(media);
        else _tempMediaCache.Upsert(media);
    }

    private void BuildPropEditSummary(PropEditPlan plan, bool dryRun, IReadOnlyDictionary<string, string>? resultByFile)
    {
        PlannedActions.Clear();

        var selectedCount = Files.Count(f => f.Selected);
        var mode = dryRun ? "DRY RUN" : "APPLY";

        AddSummaryLine($"mkvpropedit Summary - {mode}");
        AddSummaryLine($"Generated: {DateTime.Now:yyyy-MM-dd HH:mm:ss}");
        AddSummaryLine($"Selected files: {selectedCount} | Planned edits: {plan.Actions.Count} | Skipped: {plan.SkippedFiles.Count} | No Change: {plan.NoChangeFiles.Count}");
        AddSummaryLine(new string('=', 92));

        if (plan.SkippedFiles.Count > 0)
        {
            AddSummaryLine("SKIPPED FILES:");
            foreach (var skipped in plan.SkippedFiles)
            {
                AddSummaryLine($"  - {Path.GetFileName(skipped.FilePath)}");
                AddSummaryLine($"    Reason: {skipped.Reason}");
            }
            AddSummaryLine(new string('=', 92));
        }

        if (plan.NoChangeFiles.Count > 0)
        {
            AddSummaryLine("NO CHANGE FILES:");
            foreach (var noChange in plan.NoChangeFiles)
            {
                AddSummaryLine($"  - {Path.GetFileName(noChange.FilePath)}");
                AddSummaryLine($"    Reason: {noChange.Reason}");
            }
            AddSummaryLine(new string('=', 92));
        }

        for (var index = 0; index < plan.Actions.Count; index++)
        {
            var action = plan.Actions[index];
            var file = Files.FirstOrDefault(f => string.Equals(f.FilePath, action.FilePath, StringComparison.OrdinalIgnoreCase));

            AddSummaryLine($"FILE {index + 1}/{plan.Actions.Count}: {Path.GetFileName(action.FilePath)}");
            AddSummaryLine($"PATH: {action.FilePath}");
            AddSummaryLine(string.Empty);
            AddSummaryLine("CHANGES:");

            foreach (var change in SplitDescription(action.Description))
            {
                AddSummaryLine("  - " + change);
            }

            AddSummaryLine(string.Empty);
            AddSummaryLine("COMMAND:");
            AddSummaryLine("  " + FormatCommand(MkvPropEditPath, action.Arguments));

            var result = dryRun
                ? "DRY RUN | Command not executed"
                : resultByFile is not null && resultByFile.TryGetValue(action.FilePath, out var value)
                    ? value
                    : "Result unavailable";
            AddSummaryLine($"RESULT: {result}");

            if (file is not null)
            {
                AddSummaryLine($"STATUS: {file.Status}");
            }

            AddSummaryLine(new string('=', 92));
        }
    }

    private void AddSummaryLine(string line)
    {
        PlannedActions.Add(line);
    }

    private static IEnumerable<string> SplitDescription(string description)
    {
        return description
            .Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(s => string.IsNullOrWhiteSpace(s) ? "No description" : s);
    }

    private static string FormatCommand(string toolPath, IEnumerable<string> arguments)
    {
        var builder = new StringBuilder();
        builder.Append(QuoteArgument(toolPath));

        foreach (var argument in arguments)
        {
            builder.Append(' ');
            builder.Append(QuoteArgument(argument));
        }

        return builder.ToString();
    }

    private static string QuoteArgument(string value)
    {
        if (string.IsNullOrEmpty(value)) return "\"\"";
        if (value.Any(char.IsWhiteSpace) || value.Contains('\'') || value.Contains('"'))
        {
            return "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
        }
        return value;
    }

    [RelayCommand]
    private void Cancel()
    {
        _cts?.Cancel();
        _cacheCts?.Cancel();
        _auditCts?.Cancel();
        Log("Cancel requested.");
    }

    private async Task<string?> PickExecutableAsync(Window window, string title)
    {
        var files = await window.StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = title,
            AllowMultiple = false,
            FileTypeFilter = new[]
            {
                new FilePickerFileType("Executable files") { Patterns = new[] { "*.exe" } },
                FilePickerFileTypes.All
            }
        });

        return files.Count > 0 ? files[0].Path.LocalPath : null;
    }

    private PropEditPlan BuildCurrentPlan()
    {
        return BuildPropEditPlan();
    }

    private List<PlannedAction> BuildCurrentActions()
    {
        return BuildCurrentPlan().Actions;
    }

    public void EnsurePropEditState()
    {
        // Rehydrate PropEdit state when the panel is revisited. The visual control can
        // be recreated by the TabControl, but the selected template belongs to the
        // shared ViewModel state and should survive tab changes.
        var template = SelectedFile ?? AppState.SelectedFile ?? Files.FirstOrDefault(f => f.Selected) ?? Files.FirstOrDefault();
        if (template is null) return;

        if (!ReferenceEquals(SelectedFile, template))
        {
            SelectedFile = template;
            return;
        }

        if (string.IsNullOrWhiteSpace(PropEditTemplateFilePath))
        {
            PropEditTemplateFilePath = template.FilePath;
        }

        if (PropEditAudioTracks.Count == 0 && PropEditSubtitleTracks.Count == 0)
        {
            LoadPropEditTemplate(template);
        }
    }

    private void ClearPropEditTemplateState()
    {
        PropEditTemplateFilePath = string.Empty;
        PropEditAudioTracks.Clear();
        PropEditSubtitleTracks.Clear();
        DefaultAudioOptions.Clear();
        ForcedAudioOptions.Clear();
        DefaultSubtitleOptions.Clear();
        ForcedSubtitleOptions.Clear();
        SelectedDefaultAudio = "Keep existing";
        SelectedForcedAudio = "Keep existing";
        SelectedDefaultSubtitle = "Keep existing";
        SelectedForcedSubtitle = "Keep existing";
    }

    private void LoadPropEditTemplate(MkvFileItem file)
    {
        PropEditAudioTracks.Clear();
        PropEditSubtitleTracks.Clear();
        DefaultAudioOptions.Clear();
        ForcedAudioOptions.Clear();
        DefaultSubtitleOptions.Clear();
        ForcedSubtitleOptions.Clear();

        DefaultAudioOptions.Add("Keep existing");
        DefaultSubtitleOptions.Add("Keep existing");
        SelectedDefaultAudio = "Keep existing";
        SelectedForcedAudio = "Keep existing";
        SelectedDefaultSubtitle = "Keep existing";
        SelectedForcedSubtitle = "Keep existing";

        PropCustomContainerTitle = file.ContainerTitle;
        PropCustomVideoTitle = Path.GetFileNameWithoutExtension(file.FilePath);

        var audioIndex = 1;
        var subtitleIndex = 1;

        foreach (var track in file.Tracks)
        {
            if (track.Type == "audio")
            {
                var item = new PropEditTrackConfig
                {
                    TrackNumber = track.PropEditTrackNumber,
                    TrackLabel = $"Audio {audioIndex}",
                    Type = "audio",
                    CurrentName = track.Name,
                    CurrentLanguage = track.Language,
                    CurrentDefault = track.Default,
                    EditedName = track.Name,
                    EditedLanguage = track.Language
                };
                ApplyPresetOptions(item, AudioNamePresets, LanguagePresets);
                PropEditAudioTracks.Add(item);
                DefaultAudioOptions.Add(item.TrackLabel);
                if (track.Default) SelectedDefaultAudio = item.TrackLabel;
                audioIndex++;
            }
            else if (track.Type == "subtitles")
            {
                var item = new PropEditTrackConfig
                {
                    TrackNumber = track.PropEditTrackNumber,
                    TrackLabel = $"Subtitle {subtitleIndex}",
                    Type = "subtitles",
                    CurrentName = track.Name,
                    CurrentLanguage = track.Language,
                    CurrentDefault = track.Default,
                    EditedName = track.Name,
                    EditedLanguage = track.Language
                };
                ApplyPresetOptions(item, SubtitleNamePresets, LanguagePresets);
                PropEditSubtitleTracks.Add(item);
                DefaultSubtitleOptions.Add(item.TrackLabel);
                if (track.Default) SelectedDefaultSubtitle = item.TrackLabel;
                subtitleIndex++;
            }
        }
    }

    private PropEditPlan BuildPropEditPlan()
    {
        var plan = new PropEditPlan();
        var template = SelectedFile;
        if (template is null)
        {
            plan.SkippedFiles.Add(new PropEditSkippedFile("No template selected", "Select a template file first."));
            return plan;
        }

        var selectedFiles = Files.Where(f => f.Selected).ToList();
        if (selectedFiles.Count == 0)
        {
            return plan;
        }

        foreach (var file in selectedFiles)
        {
            if (!CrossPlatformRuntime.IsMkvPath(file.FilePath))
            {
                plan.SkippedFiles.Add(new PropEditSkippedFile(file.FilePath, "Track property edits are only available for MKV files."));
                continue;
            }

            if (!HasCompatibleTrackLayout(template, file, out var layoutError))
            {
                plan.SkippedFiles.Add(new PropEditSkippedFile(file.FilePath, layoutError));
                continue;
            }

            var build = _propEditCommandBuilder.Build(new MkvPropEditCommandBuildRequest(
                file,
                PropEditAudioTracks.ToList(),
                PropEditSubtitleTracks.ToList(),
                SelectedDefaultAudio,
                SelectedForcedAudio,
                SelectedDefaultSubtitle,
                SelectedForcedSubtitle,
                PropContainerTitleFromFile,
                PropContainerTitleCustom,
                PropRemoveContainerTitle,
                PropCustomContainerTitle,
                PropVideoTitleFromFile,
                PropVideoTitleCustom,
                PropRemoveVideoTitle,
                PropCustomVideoTitle));

            var args = build.Arguments.ToList();
            var descriptions = build.Descriptions.ToList();

            if (descriptions.Count > 0)
            {
                var action = new PlannedAction
                {
                    FilePath = file.FilePath,
                    Description = string.Join("; ", descriptions)
                };
                action.Arguments.AddRange(args);
                plan.Actions.Add(action);
            }
            else
            {
                plan.NoChangeFiles.Add(new PropEditNoChangeFile(file.FilePath, "All selected settings already match this file."));
            }
        }

        return plan;
    }

    private static bool HasCompatibleTrackLayout(MkvFileItem template, MkvFileItem file, out string error)
    {
        error = string.Empty;
        foreach (var type in new[] { "video", "audio", "subtitles" })
        {
            var normalizedType = MkvTrackSelector.NormalizeTrackType(type);
            var templateCount = template.Tracks.Count(t => MkvTrackSelector.NormalizeTrackType(t.Type) == normalizedType);
            var fileCount = file.Tracks.Count(t => MkvTrackSelector.NormalizeTrackType(t.Type) == normalizedType);
            if (templateCount != fileCount)
            {
                error = $"track layout mismatch for {type}: template has {templateCount}, file has {fileCount}";
                return false;
            }
        }
        return true;
    }
}
