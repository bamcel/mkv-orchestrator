using System.Text.RegularExpressions;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services.Rename;

/// <summary>
/// Single assembly point for rename destination filenames. The desktop app and the
/// web host must both flow through this builder so template token behavior stays
/// identical across surfaces.
/// </summary>
public static class RenameFileNameBuilder
{
    public const string DefaultSeriesTemplate = "{series} - S{season:00}E{episode:00} - {episodeTitle}";
    public const string DefaultMovieTemplate = "{title} ({year})";

    public static string Build(
        string sourcePath,
        string title,
        int? year,
        TvdbEpisode episode,
        string? template,
        bool isMovie)
    {
        var activeTemplate = string.IsNullOrWhiteSpace(template)
            ? DefaultSeriesTemplate
            : template.Trim();
        if (isMovie && activeTemplate.Equals(DefaultSeriesTemplate, StringComparison.OrdinalIgnoreCase))
        {
            activeTemplate = DefaultMovieTemplate;
        }

        // Synthetic ordering token: season block of 1000 plus episode number. Specials
        // (season 0) are clamped to season 1 so the value never goes negative.
        var absolute = ((Math.Max(episode.SeasonNumber, 1) - 1) * 1000) + episode.EpisodeNumber;
        var value = activeTemplate
            .Replace("{title}", title, StringComparison.OrdinalIgnoreCase)
            .Replace("{series}", title, StringComparison.OrdinalIgnoreCase)
            .Replace("{year}", year?.ToString() ?? string.Empty, StringComparison.OrdinalIgnoreCase)
            .Replace("{episodeTitle}", episode.Name, StringComparison.OrdinalIgnoreCase)
            .Replace("{season:00}", episode.SeasonNumber.ToString("00"), StringComparison.OrdinalIgnoreCase)
            .Replace("{episode:00}", episode.EpisodeNumber.ToString("00"), StringComparison.OrdinalIgnoreCase)
            .Replace("{season}", episode.SeasonNumber.ToString(), StringComparison.OrdinalIgnoreCase)
            .Replace("{episode}", episode.EpisodeNumber.ToString(), StringComparison.OrdinalIgnoreCase)
            .Replace("{absolute:000}", absolute.ToString("000"), StringComparison.OrdinalIgnoreCase)
            .Replace("{absolute}", absolute.ToString(), StringComparison.OrdinalIgnoreCase);

        return SanitizeFileName(value.Trim()) + Path.GetExtension(sourcePath);
    }

    public static string SanitizeFileName(string value)
    {
        foreach (var invalid in Path.GetInvalidFileNameChars())
        {
            value = value.Replace(invalid, '-');
        }

        // Windows-reserved characters are replaced on every OS so rename results
        // stay identical between the desktop app and the Linux web container.
        foreach (var invalid in "\\/:*?\"<>|")
        {
            value = value.Replace(invalid, '-');
        }

        value = Regex.Replace(value, @"\s+", " ").Trim();
        value = Regex.Replace(value, @"\s+-\s+", " - ").Trim();
        return value.TrimEnd('.', ' ');
    }
}
