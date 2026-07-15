using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services.Rename;

public sealed record RenamePlanRequest(
    IReadOnlyCollection<MediaFile> Files,
    string Template,
    bool CheckExistingFiles = true);

public sealed record RenamePlanItem(
    string SourcePath,
    string TargetPath,
    string NewFileName,
    string Status,
    bool CanApply);

public sealed record RenamePlan(IReadOnlyList<RenamePlanItem> Items)
{
    public int RenameCount => Items.Count(i => i.CanApply && !string.Equals(i.SourcePath, i.TargetPath, StringComparison.OrdinalIgnoreCase));
    public int SkipCount => Items.Count - RenameCount;
    public bool HasBlockingIssues => Items.Any(i => !i.CanApply && !string.Equals(i.Status, "No change", StringComparison.OrdinalIgnoreCase));
}

public sealed class RenamePlanner
{
    private const string DefaultTemplate = "{series} - S{season:00}E{episode:00} - {episodeTitle}";

    public RenamePlan BuildPlan(RenamePlanRequest request)
    {
        var items = request.Files.Select(file => BuildItem(file, request.Template, request.CheckExistingFiles)).ToList();
        var targetGroups = items
            .Where(i => !string.Equals(i.SourcePath, i.TargetPath, StringComparison.OrdinalIgnoreCase))
            .GroupBy(i => i.TargetPath, StringComparer.OrdinalIgnoreCase)
            .Where(g => g.Count() > 1)
            .SelectMany(g => g)
            .ToHashSet();

        if (targetGroups.Count == 0)
        {
            return new RenamePlan(items);
        }

        var rewritten = items.Select(item => targetGroups.Contains(item)
            ? item with { Status = "Skipped - duplicate target in plan", CanApply = false }
            : item).ToList();

        return new RenamePlan(rewritten);
    }

    public async Task<RenamePlan> ApplyAsync(RenamePlan plan, CancellationToken cancellationToken)
    {
        var results = new List<RenamePlanItem>(plan.Items.Count);

        foreach (var item in plan.Items)
        {
            cancellationToken.ThrowIfCancellationRequested();

            if (!item.CanApply)
            {
                results.Add(item);
                continue;
            }

            if (string.Equals(item.SourcePath, item.TargetPath, StringComparison.OrdinalIgnoreCase))
            {
                results.Add(item with { Status = "No change", CanApply = false });
                continue;
            }

            try
            {
                File.Move(item.SourcePath, item.TargetPath);
                results.Add(item with { Status = "Renamed", CanApply = false });
            }
            catch (Exception ex)
            {
                results.Add(item with { Status = "Failed - " + ex.Message, CanApply = false });
            }
        }

        await Task.CompletedTask;
        return new RenamePlan(results);
    }

    public string BuildFileName(MediaFile file, string? template)
    {
        var activeTemplate = string.IsNullOrWhiteSpace(template) ? DefaultTemplate : template.Trim();
        var value = activeTemplate
            .Replace("{series}", FirstNonBlank(file.ProviderMatch.SeriesName, file.SeriesTitle, Path.GetFileNameWithoutExtension(file.FilePath)), StringComparison.OrdinalIgnoreCase)
            .Replace("{episodeTitle}", FirstNonBlank(file.ProviderMatch.EpisodeName, file.EpisodeTitle), StringComparison.OrdinalIgnoreCase)
            .Replace("{year}", string.Empty, StringComparison.OrdinalIgnoreCase)
            .Replace("{absolute}", FormatNumber(file.AbsoluteEpisode, string.Empty), StringComparison.OrdinalIgnoreCase)
            .Replace("{season:00}", FormatNumber(file.Season, "00"), StringComparison.OrdinalIgnoreCase)
            .Replace("{episode:00}", FormatNumber(file.Episode, "00"), StringComparison.OrdinalIgnoreCase)
            .Replace("{season}", FormatNumber(file.Season, string.Empty), StringComparison.OrdinalIgnoreCase)
            .Replace("{episode}", FormatNumber(file.Episode, string.Empty), StringComparison.OrdinalIgnoreCase);

        var extension = Path.GetExtension(file.FilePath);
        return SanitizeFileName(value.Trim()) + extension;
    }

    public static string SanitizeFileName(string value)
    {
        var clean = value;
        foreach (var invalid in Path.GetInvalidFileNameChars())
        {
            clean = clean.Replace(invalid, ' ');
        }

        // Windows-reserved characters are replaced on every OS so rename results
        // stay identical between the desktop app and the Linux web container.
        foreach (var invalid in "\\/:*?\"<>|")
        {
            clean = clean.Replace(invalid, ' ');
        }

        while (clean.Contains("  ", StringComparison.Ordinal))
        {
            clean = clean.Replace("  ", " ", StringComparison.Ordinal);
        }

        return clean.Trim().TrimEnd('.');
    }

    private RenamePlanItem BuildItem(MediaFile file, string template, bool checkExistingFiles)
    {
        var newFileName = BuildFileName(file, template);
        if (string.IsNullOrWhiteSpace(newFileName) || string.Equals(newFileName, Path.GetExtension(file.FilePath), StringComparison.OrdinalIgnoreCase))
        {
            return new RenamePlanItem(file.FilePath, file.FilePath, string.Empty, "Skipped - empty target name", CanApply: false);
        }

        var directory = Path.GetDirectoryName(file.FilePath) ?? string.Empty;
        var targetPath = Path.Combine(directory, newFileName);
        if (string.Equals(file.FilePath, targetPath, StringComparison.OrdinalIgnoreCase))
        {
            return new RenamePlanItem(file.FilePath, targetPath, newFileName, "No change", CanApply: false);
        }

        if (checkExistingFiles && File.Exists(targetPath))
        {
            return new RenamePlanItem(file.FilePath, targetPath, newFileName, "Skipped - target exists", CanApply: false);
        }

        return new RenamePlanItem(file.FilePath, targetPath, newFileName, "Ready", CanApply: true);
    }

    private static string FormatNumber(int? value, string format)
    {
        if (!value.HasValue) return string.Empty;
        return string.IsNullOrWhiteSpace(format) ? value.Value.ToString() : value.Value.ToString(format);
    }

    private static string FirstNonBlank(params string?[] values)
    {
        return values.FirstOrDefault(value => !string.IsNullOrWhiteSpace(value))?.Trim() ?? string.Empty;
    }
}
