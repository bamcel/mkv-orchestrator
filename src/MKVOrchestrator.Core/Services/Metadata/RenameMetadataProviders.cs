using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services;

/// <summary>
/// Shared Jellyfin-style provider contract for mkvrename metadata lookup.
/// Each provider normalizes its native API into the same series/episode model so the UI can
/// search, select scopes, build previews, and execute renames without provider-specific logic.
/// </summary>
public interface IRenameMetadataProvider
{
    string Key { get; }
    string DisplayName { get; }

    Task<IReadOnlyList<TvdbSeriesSearchResult>> SearchSeriesAsync(
        string query,
        string language,
        AppSettings settings,
        CancellationToken token);

    Task<IReadOnlyList<TvdbEpisode>> GetEpisodesAsync(
        TvdbSeriesSearchResult selectedSeries,
        string language,
        AppSettings settings,
        CancellationToken token);
}

public sealed class TvdbRenameMetadataProvider : IRenameMetadataProvider
{
    private readonly TvdbService _service = new();
    public string Key => "TVDB";
    public string DisplayName => "TVDB";

    public async Task<IReadOnlyList<TvdbSeriesSearchResult>> SearchSeriesAsync(string query, string language, AppSettings settings, CancellationToken token)
    {
        var results = await _service.SearchSeriesAsync(settings.TvdbApiKey, settings.TvdbPin, query, language, token);
        return NormalizeResults(results);
    }

    public async Task<IReadOnlyList<TvdbEpisode>> GetEpisodesAsync(TvdbSeriesSearchResult selectedSeries, string language, AppSettings settings, CancellationToken token)
    {
        var episodes = await _service.GetEpisodesAsync(settings.TvdbApiKey, settings.TvdbPin, selectedSeries, language, "All seasons + specials", token);
        return NormalizeEpisodes(episodes);
    }

    private IReadOnlyList<TvdbSeriesSearchResult> NormalizeResults(IReadOnlyList<TvdbSeriesSearchResult> results)
    {
        foreach (var result in results) result.Provider = Key;
        return results;
    }

    private IReadOnlyList<TvdbEpisode> NormalizeEpisodes(IReadOnlyList<TvdbEpisode> episodes)
    {
        foreach (var episode in episodes) episode.Provider = Key;
        return episodes;
    }
}

public sealed class TmdbRenameMetadataProvider : IRenameMetadataProvider
{
    private readonly TmdbService _service = new();
    public string Key => "TMDB";
    public string DisplayName => "TMDB";

    public async Task<IReadOnlyList<TvdbSeriesSearchResult>> SearchSeriesAsync(string query, string language, AppSettings settings, CancellationToken token)
    {
        var results = await _service.SearchSeriesAsync(settings.TmdbApiKey, query, language, token);
        foreach (var result in results) result.Provider = Key;
        return results;
    }

    public async Task<IReadOnlyList<TvdbEpisode>> GetEpisodesAsync(TvdbSeriesSearchResult selectedSeries, string language, AppSettings settings, CancellationToken token)
    {
        var episodes = await _service.GetEpisodesAsync(settings.TmdbApiKey, selectedSeries, language, token);
        foreach (var episode in episodes) episode.Provider = Key;
        return episodes;
    }
}
