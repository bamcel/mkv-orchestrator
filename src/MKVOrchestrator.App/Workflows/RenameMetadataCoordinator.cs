using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.Workflows;

public sealed class RenameMetadataCoordinator(IReadOnlyDictionary<string, IRenameMetadataProvider> providers)
{
    private List<TvdbEpisode> _cachedEpisodes = new();
    private int? _cachedSeriesId;
    private string _cachedLanguage = string.Empty;
    private string _cachedProvider = string.Empty;

    public IRenameMetadataProvider GetProvider(string? provider)
    {
        var key = string.Equals(provider, "TMDB", StringComparison.OrdinalIgnoreCase) ? "TMDB" : "TVDB";
        return providers.TryGetValue(key, out var value) ? value : providers["TVDB"];
    }

    public bool TryGetCached(int seriesId, string language, string provider, out IReadOnlyList<TvdbEpisode> episodes)
    {
        if (_cachedSeriesId == seriesId
            && string.Equals(_cachedLanguage, language, StringComparison.OrdinalIgnoreCase)
            && string.Equals(_cachedProvider, provider, StringComparison.OrdinalIgnoreCase)
            && _cachedEpisodes.Count > 0)
        {
            episodes = _cachedEpisodes;
            return true;
        }

        episodes = Array.Empty<TvdbEpisode>();
        return false;
    }

    public IReadOnlyList<TvdbEpisode> Store(
        int seriesId,
        string language,
        string provider,
        IEnumerable<TvdbEpisode> episodes)
    {
        _cachedSeriesId = seriesId;
        _cachedLanguage = language;
        _cachedProvider = provider;
        _cachedEpisodes = episodes.ToList();
        return _cachedEpisodes;
    }

    public void Clear()
    {
        _cachedEpisodes.Clear();
        _cachedSeriesId = null;
        _cachedLanguage = string.Empty;
        _cachedProvider = string.Empty;
    }
}
