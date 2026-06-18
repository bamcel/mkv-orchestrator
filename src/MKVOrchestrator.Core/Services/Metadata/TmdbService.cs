using System.Net.Http;
using System.Text.Json;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services;

public sealed class TmdbService
{
    private const string BaseUrl = "https://api.themoviedb.org/3";
    private static readonly HttpClient Client = new() { Timeout = TimeSpan.FromSeconds(30) };

    public async Task<IReadOnlyList<TvdbSeriesSearchResult>> SearchSeriesAsync(string apiKey, string query, string language, CancellationToken token)
    {
        if (string.IsNullOrWhiteSpace(apiKey)) throw new InvalidOperationException("Enter a TMDB API key in Settings before searching TMDB.");
        if (string.IsNullOrWhiteSpace(query)) return Array.Empty<TvdbSeriesSearchResult>();

        var lang = NormalizeTmdbLanguage(language);
        var url = $"{BaseUrl}/search/multi?api_key={Uri.EscapeDataString(apiKey.Trim())}&query={Uri.EscapeDataString(query.Trim())}&language={Uri.EscapeDataString(lang)}&include_adult=false";
        using var response = await Client.GetAsync(url, token);
        var json = await response.Content.ReadAsStringAsync(token);
        response.EnsureSuccessStatusCode();

        using var document = JsonDocument.Parse(json);
        if (!document.RootElement.TryGetProperty("results", out var resultsElement) || resultsElement.ValueKind != JsonValueKind.Array)
        {
            return Array.Empty<TvdbSeriesSearchResult>();
        }

        var results = new List<TvdbSeriesSearchResult>();
        foreach (var item in resultsElement.EnumerateArray())
        {
            var mediaType = ReadString(item, "media_type");
            if (!mediaType.Equals("tv", StringComparison.OrdinalIgnoreCase)
                && !mediaType.Equals("movie", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            var id = ReadInt(item, "id");
            var name = mediaType.Equals("movie", StringComparison.OrdinalIgnoreCase)
                ? ReadString(item, "title")
                : ReadString(item, "name");
            if (id <= 0 || string.IsNullOrWhiteSpace(name)) continue;

            var date = mediaType.Equals("movie", StringComparison.OrdinalIgnoreCase)
                ? ReadString(item, "release_date")
                : ReadString(item, "first_air_date");
            var year = date.Length >= 4 ? date[..4] : string.Empty;
            results.Add(new TvdbSeriesSearchResult
            {
                Id = id,
                Name = name,
                Year = year,
                Overview = ReadString(item, "overview"),
                Provider = "TMDB",
                Format = mediaType.Equals("movie", StringComparison.OrdinalIgnoreCase) ? "Movie" : "TV"
            });
        }
        return results;
    }

    public async Task<IReadOnlyList<TvdbEpisode>> GetEpisodesAsync(string apiKey, TvdbSeriesSearchResult selectedResult, string language, CancellationToken token)
    {
        if (string.IsNullOrWhiteSpace(apiKey)) throw new InvalidOperationException("Enter a TMDB API key in Settings before loading TMDB episodes.");
        if (selectedResult.Format.Equals("Movie", StringComparison.OrdinalIgnoreCase))
        {
            return await GetMovieAsEpisodeAsync(apiKey, selectedResult, language, token);
        }

        var lang = NormalizeTmdbLanguage(language);
        var seriesId = selectedResult.Id;
        var detailsUrl = $"{BaseUrl}/tv/{seriesId}?api_key={Uri.EscapeDataString(apiKey.Trim())}&language={Uri.EscapeDataString(lang)}";
        using var detailsResponse = await Client.GetAsync(detailsUrl, token);
        var detailsJson = await detailsResponse.Content.ReadAsStringAsync(token);
        detailsResponse.EnsureSuccessStatusCode();

        using var detailsDocument = JsonDocument.Parse(detailsJson);
        if (!detailsDocument.RootElement.TryGetProperty("seasons", out var seasonsElement) || seasonsElement.ValueKind != JsonValueKind.Array)
        {
            return Array.Empty<TvdbEpisode>();
        }

        var seasonNumbers = seasonsElement.EnumerateArray()
            .Select(s => ReadInt(s, "season_number"))
            .Distinct()
            .OrderBy(n => n)
            .ToList();

        var episodes = new List<TvdbEpisode>();
        foreach (var seasonNumber in seasonNumbers)
        {
            var seasonUrl = $"{BaseUrl}/tv/{seriesId}/season/{seasonNumber}?api_key={Uri.EscapeDataString(apiKey.Trim())}&language={Uri.EscapeDataString(lang)}";
            using var seasonResponse = await Client.GetAsync(seasonUrl, token);
            if (!seasonResponse.IsSuccessStatusCode) continue;

            var seasonJson = await seasonResponse.Content.ReadAsStringAsync(token);
            using var seasonDocument = JsonDocument.Parse(seasonJson);
            if (!seasonDocument.RootElement.TryGetProperty("episodes", out var episodeElement) || episodeElement.ValueKind != JsonValueKind.Array) continue;

            foreach (var ep in episodeElement.EnumerateArray())
            {
                var episodeNumber = ReadInt(ep, "episode_number");
                if (episodeNumber <= 0) continue;
                var id = ReadInt(ep, "id");
                episodes.Add(new TvdbEpisode
                {
                    Id = id > 0 ? id : (seriesId * 1000000) + (seasonNumber * 10000) + episodeNumber,
                    SeasonNumber = seasonNumber,
                    EpisodeNumber = episodeNumber,
                    Name = ReadString(ep, "name"),
                    Provider = "TMDB"
                });
            }
        }

        return episodes;
    }

    private async Task<IReadOnlyList<TvdbEpisode>> GetMovieAsEpisodeAsync(string apiKey, TvdbSeriesSearchResult selectedMovie, string language, CancellationToken token)
    {
        var lang = NormalizeTmdbLanguage(language);
        var movieUrl = $"{BaseUrl}/movie/{selectedMovie.Id}?api_key={Uri.EscapeDataString(apiKey.Trim())}&language={Uri.EscapeDataString(lang)}";
        using var movieResponse = await Client.GetAsync(movieUrl, token);
        var movieJson = await movieResponse.Content.ReadAsStringAsync(token);
        movieResponse.EnsureSuccessStatusCode();

        using var movieDocument = JsonDocument.Parse(movieJson);
        var title = ReadString(movieDocument.RootElement, "title");
        if (string.IsNullOrWhiteSpace(title)) title = selectedMovie.Name;

        return new[]
        {
            new TvdbEpisode
            {
                Id = selectedMovie.Id,
                SeasonNumber = 1,
                EpisodeNumber = 1,
                Name = title,
                Provider = "TMDB",
                ScopeName = "Movie"
            }
        };
    }

    private static string NormalizeTmdbLanguage(string language)
    {
        var value = (language ?? string.Empty).Trim().ToLowerInvariant();
        return value switch
        {
            "eng" or "en" => "en-US",
            "jpn" or "ja" => "ja-JP",
            "spa" or "es" => "es-ES",
            "fre" or "fra" or "fr" => "fr-FR",
            "ger" or "deu" or "de" => "de-DE",
            "ita" or "it" => "it-IT",
            "por" or "pt" => "pt-PT",
            "kor" or "ko" => "ko-KR",
            "chi" or "zh" => "zh-CN",
            _ => string.IsNullOrWhiteSpace(value) ? "en-US" : value
        };
    }

    private static int ReadInt(JsonElement element, string property)
    {
        if (!element.TryGetProperty(property, out var value)) return 0;
        if (value.ValueKind == JsonValueKind.Number && value.TryGetInt32(out var number)) return number;
        if (value.ValueKind == JsonValueKind.String && int.TryParse(value.GetString(), out number)) return number;
        return 0;
    }

    private static string ReadString(JsonElement element, string property)
    {
        if (!element.TryGetProperty(property, out var value) || value.ValueKind == JsonValueKind.Null) return string.Empty;
        return value.ValueKind == JsonValueKind.String ? value.GetString() ?? string.Empty : value.ToString();
    }
}
