using System.Collections.Concurrent;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.WebHost;

/// <summary>
/// Short-lived in-memory episode cache so scope loading and preview building do
/// not re-fetch the full provider episode list on every request.
/// </summary>
public sealed class RenameEpisodeCache
{
    private static readonly TimeSpan Ttl = TimeSpan.FromMinutes(10);

    private sealed record CacheKey(string Provider, int SeriesId, string Language, string Format);

    private readonly ConcurrentDictionary<CacheKey, (DateTimeOffset LoadedUtc, IReadOnlyList<TvdbEpisode> Episodes)> _entries = new();

    public async Task<IReadOnlyList<TvdbEpisode>> GetOrLoadAsync(
        string provider,
        TvdbSeriesSearchResult selectedResult,
        string language,
        Func<Task<IReadOnlyList<TvdbEpisode>>> loader)
    {
        var key = new CacheKey(
            (provider ?? string.Empty).Trim().ToUpperInvariant(),
            selectedResult.Id,
            (language ?? string.Empty).Trim().ToLowerInvariant(),
            (selectedResult.Format ?? string.Empty).Trim().ToUpperInvariant());

        if (_entries.TryGetValue(key, out var entry) && DateTimeOffset.UtcNow - entry.LoadedUtc < Ttl)
        {
            return entry.Episodes;
        }

        var episodes = await loader();
        _entries[key] = (DateTimeOffset.UtcNow, episodes);
        PruneExpired();
        return episodes;
    }

    private void PruneExpired()
    {
        if (_entries.Count <= 64) return;

        foreach (var pair in _entries)
        {
            if (DateTimeOffset.UtcNow - pair.Value.LoadedUtc >= Ttl)
            {
                _entries.TryRemove(pair.Key, out _);
            }
        }
    }
}
