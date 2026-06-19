using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services;

public sealed class TvdbService
{
    private const string BaseUrl = "https://api4.thetvdb.com/v4";
    private static readonly HttpClient Client = new() { Timeout = TimeSpan.FromSeconds(30) };

    public async Task<IReadOnlyList<TvdbSeriesSearchResult>> SearchSeriesAsync(string apiKey, string pin, string query, string preferredLanguage, CancellationToken token)
    {
        if (string.IsNullOrWhiteSpace(query)) return Array.Empty<TvdbSeriesSearchResult>();

        var language = NormalizeLanguage(preferredLanguage);
        var bearer = await LoginAsync(apiKey, pin, token);
        var results = new List<TvdbSeriesSearchResult>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var type in new[] { "series", "movie" })
        {
            var typeResults = await SearchByTypeAsync(bearer, type, query, language, token);
            foreach (var result in typeResults)
            {
                if (seen.Add($"{result.Format}:{result.Id}"))
                {
                    results.Add(result);
                }
            }
        }

        return results;
    }

    private static async Task<IReadOnlyList<TvdbSeriesSearchResult>> SearchByTypeAsync(string bearer, string type, string query, string language, CancellationToken token)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, $"{BaseUrl}/search?type={Uri.EscapeDataString(type)}&query={Uri.EscapeDataString(query.Trim())}&language={language}");
        request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", bearer);
        ApplyPreferredLanguage(request, language);

        using var response = await Client.SendAsync(request, token);
        var json = await response.Content.ReadAsStringAsync(token);
        response.EnsureSuccessStatusCode();

        using var document = JsonDocument.Parse(json);
        if (!document.RootElement.TryGetProperty("data", out var data) || data.ValueKind != JsonValueKind.Array)
        {
            return Array.Empty<TvdbSeriesSearchResult>();
        }

        var results = new List<TvdbSeriesSearchResult>();
        foreach (var item in data.EnumerateArray())
        {
            var id = ReadInt(item, "tvdb_id");
            if (id == 0) id = ReadInt(item, "id");
            var name = ReadLocalizedString(item, "name", language);
            if (string.IsNullOrWhiteSpace(name)) name = ReadLocalizedString(item, "seriesName", language);
            if (string.IsNullOrWhiteSpace(name)) name = ReadLocalizedString(item, "title", language);
            if (id == 0 || string.IsNullOrWhiteSpace(name)) continue;

            results.Add(new TvdbSeriesSearchResult
            {
                Id = id,
                Name = name,
                Year = ReadString(item, "year"),
                Overview = ReadLocalizedString(item, "overview", language),
                Provider = "TVDB",
                Format = type.Equals("movie", StringComparison.OrdinalIgnoreCase) ? "Movie" : "TV",
                DatabaseUrl = BuildDatabaseUrl(type, id)
            });
        }

        return results;
    }

    private static string BuildDatabaseUrl(string type, int id)
    {
        var path = type.Equals("movie", StringComparison.OrdinalIgnoreCase) ? "movie" : "series";
        return $"https://thetvdb.com/dereferrer/{path}/{id}";
    }

    public async Task<IReadOnlyList<TvdbEpisode>> GetEpisodesAsync(string apiKey, string pin, TvdbSeriesSearchResult selectedResult, string preferredLanguage, string seasonFilter, CancellationToken token)
    {
        var language = NormalizeLanguage(preferredLanguage);
        var bearer = await LoginAsync(apiKey, pin, token);
        if (selectedResult.Format.Equals("Movie", StringComparison.OrdinalIgnoreCase))
        {
            return await GetMovieAsEpisodeAsync(bearer, selectedResult, language, token);
        }

        var seriesId = selectedResult.Id;
        var episodes = new List<TvdbEpisode>();
        var seenEpisodeIds = new HashSet<int>();
        var page = 0;
        var filter = ParseSeasonFilter(seasonFilter);

        while (true)
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, $"{BaseUrl}/series/{seriesId}/episodes/default?page={page}&language={language}");
            request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", bearer);
            ApplyPreferredLanguage(request, language);
            using var response = await Client.SendAsync(request, token);
            var json = await response.Content.ReadAsStringAsync(token);
            response.EnsureSuccessStatusCode();

            using var document = JsonDocument.Parse(json);
            if (!document.RootElement.TryGetProperty("data", out var data)) break;
            if (!data.TryGetProperty("episodes", out var episodeArray) || episodeArray.ValueKind != JsonValueKind.Array) break;

            var pageCount = 0;
            var newEpisodeCount = 0;
            foreach (var item in episodeArray.EnumerateArray())
            {
                var season = ReadInt(item, "seasonNumber");
                var number = ReadInt(item, "number");
                var name = ReadLocalizedString(item, "name", language);
                var id = ReadInt(item, "id");
                if (id > 0)
                {
                    var translatedName = await GetEpisodeNameTranslationAsync(bearer, id, language, token);
                    if (!string.IsNullOrWhiteSpace(translatedName)) name = translatedName;
                }

                if (number <= 0 || string.IsNullOrWhiteSpace(name)) continue;
                if (!ShouldIncludeSeason(season, filter)) continue;
                if (id > 0 && !seenEpisodeIds.Add(id)) continue;

                episodes.Add(new TvdbEpisode
                {
                    Id = id,
                    SeasonNumber = season,
                    EpisodeNumber = number,
                    Name = name
                });
                pageCount++;
                newEpisodeCount++;
            }

            if (episodeArray.GetArrayLength() == 0) break;
            if (newEpisodeCount == 0 && !HasNextPage(data, page)) break;
            if (!HasNextPage(data, page) && episodeArray.GetArrayLength() == 0) break;
            page++;
            if (page > 100) break;
        }

        return episodes;
    }

    private static async Task<IReadOnlyList<TvdbEpisode>> GetMovieAsEpisodeAsync(string bearer, TvdbSeriesSearchResult selectedMovie, string language, CancellationToken token)
    {
        var title = selectedMovie.Name;
        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, $"{BaseUrl}/movies/{selectedMovie.Id}/extended?meta=translations&short=true");
            request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", bearer);
            ApplyPreferredLanguage(request, language);

            using var response = await Client.SendAsync(request, token);
            if (response.IsSuccessStatusCode)
            {
                var json = await response.Content.ReadAsStringAsync(token);
                using var document = JsonDocument.Parse(json);
                if (document.RootElement.TryGetProperty("data", out var data))
                {
                    var detailTitle = ReadLocalizedString(data, "name", language);
                    if (string.IsNullOrWhiteSpace(detailTitle)) detailTitle = ReadLocalizedString(data, "title", language);
                    if (!string.IsNullOrWhiteSpace(detailTitle)) title = detailTitle;
                }
            }
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch
        {
            // Movie detail lookup is best effort. Search results already contain the title.
        }

        return new[]
        {
            new TvdbEpisode
            {
                Id = selectedMovie.Id,
                SeasonNumber = 1,
                EpisodeNumber = 1,
                Name = title,
                Provider = "TVDB",
                ScopeName = "Movie"
            }
        };
    }

    private sealed record SeasonFilter(bool IncludeRegularSeasons, bool IncludeSpecials, int? SpecificSeason);

    private static SeasonFilter ParseSeasonFilter(string? value)
    {
        var filter = (value ?? string.Empty).Trim();
        if (string.IsNullOrWhiteSpace(filter)) return new SeasonFilter(true, true, null);
        if (filter.Equals("Specials only", StringComparison.OrdinalIgnoreCase)) return new SeasonFilter(false, true, 0);
        if (filter.Equals("All seasons", StringComparison.OrdinalIgnoreCase)) return new SeasonFilter(true, false, null);
        if (filter.Equals("All seasons + specials", StringComparison.OrdinalIgnoreCase)) return new SeasonFilter(true, true, null);
        var match = System.Text.RegularExpressions.Regex.Match(filter, @"Season\s+(\d+)", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        if (match.Success && int.TryParse(match.Groups[1].Value, out var season)) return new SeasonFilter(true, false, season);
        if (int.TryParse(filter, out var numericSeason)) return new SeasonFilter(numericSeason > 0, numericSeason == 0, numericSeason);
        return new SeasonFilter(true, true, null);
    }

    private static bool ShouldIncludeSeason(int season, SeasonFilter filter)
    {
        if (filter.SpecificSeason.HasValue) return season == filter.SpecificSeason.Value;
        if (season == 0) return filter.IncludeSpecials;
        return season > 0 && filter.IncludeRegularSeasons;
    }

    private static async Task<string> GetEpisodeNameTranslationAsync(string bearer, int episodeId, string preferredLanguage, CancellationToken token)
    {
        foreach (var language in BuildLanguageFallbacks(preferredLanguage))
        {
            try
            {
                using var request = new HttpRequestMessage(HttpMethod.Get, $"{BaseUrl}/episodes/{episodeId}/translations/{language}");
                request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", bearer);
                ApplyPreferredLanguage(request, language);

                using var response = await Client.SendAsync(request, token);
                if (!response.IsSuccessStatusCode) continue;

                var json = await response.Content.ReadAsStringAsync(token);
                using var document = JsonDocument.Parse(json);
                if (!document.RootElement.TryGetProperty("data", out var data)) continue;

                var name = ReadString(data, "name");
                if (string.IsNullOrWhiteSpace(name)) name = ReadString(data, "episodeName");
                if (string.IsNullOrWhiteSpace(name)) name = ReadString(data, "title");
                if (!string.IsNullOrWhiteSpace(name)) return name;
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch
            {
                // Translation lookup is best effort. Keep the episode-list value if it fails.
            }
        }

        return string.Empty;
    }

    private static async Task<string> LoginAsync(string apiKey, string pin, CancellationToken token)
    {
        if (string.IsNullOrWhiteSpace(apiKey))
        {
            throw new InvalidOperationException("Enter a TVDB API key in Settings before searching.");
        }

        var payload = new Dictionary<string, string> { ["apikey"] = apiKey };
        if (!string.IsNullOrWhiteSpace(pin)) payload["pin"] = pin;
        var json = JsonSerializer.Serialize(payload);
        using var content = new StringContent(json, Encoding.UTF8, "application/json");
        using var response = await Client.PostAsync($"{BaseUrl}/login", content, token);
        var responseJson = await response.Content.ReadAsStringAsync(token);
        response.EnsureSuccessStatusCode();

        using var document = JsonDocument.Parse(responseJson);
        if (document.RootElement.TryGetProperty("data", out var data)
            && data.TryGetProperty("token", out var tokenElement))
        {
            return tokenElement.GetString() ?? string.Empty;
        }

        throw new InvalidOperationException("TVDB login succeeded but no token was returned.");
    }

    private static bool HasNextPage(JsonElement data, int currentPage)
    {
        if (data.TryGetProperty("links", out var links)
            && links.TryGetProperty("next", out var next)
            && next.ValueKind != JsonValueKind.Null)
        {
            return true;
        }

        if (data.TryGetProperty("page", out var pageElement) && data.TryGetProperty("pages", out var pagesElement))
        {
            return pageElement.GetInt32() < pagesElement.GetInt32() - 1;
        }

        return currentPage == 0;
    }

    private static void ApplyPreferredLanguage(HttpRequestMessage request, string language)
    {
        language = NormalizeLanguage(language);
        request.Headers.TryAddWithoutValidation("Accept-Language", language);
        request.Headers.TryAddWithoutValidation("Content-Language", language);
    }

    private static string NormalizeLanguage(string? language)
    {
        language = (language ?? string.Empty).Trim();
        return string.IsNullOrWhiteSpace(language) ? "eng" : language;
    }

    private static IEnumerable<string> BuildLanguageFallbacks(string preferredLanguage)
    {
        var language = NormalizeLanguage(preferredLanguage);
        yield return language;
        if (string.Equals(language, "eng", StringComparison.OrdinalIgnoreCase)) yield return "en";
        else if (string.Equals(language, "en", StringComparison.OrdinalIgnoreCase)) yield return "eng";
    }

    private static string ReadLocalizedString(JsonElement element, string property, string preferredLanguage)
    {
        var language = NormalizeLanguage(preferredLanguage);
        if (element.TryGetProperty("translations", out var translations))
        {
            if (translations.ValueKind == JsonValueKind.Object)
            {
                foreach (var candidate in BuildLanguageFallbacks(language))
                {
                    if (translations.TryGetProperty(candidate, out var localized) && localized.ValueKind != JsonValueKind.Null)
                    {
                        var value = localized.ToString();
                        if (!string.IsNullOrWhiteSpace(value)) return value;
                    }
                }
            }
            else if (translations.ValueKind == JsonValueKind.Array)
            {
                var candidates = BuildLanguageFallbacks(language).ToList();
                foreach (var translation in translations.EnumerateArray())
                {
                    var translationLanguage = ReadString(translation, "language");
                    if (!candidates.Any(candidate => string.Equals(candidate, translationLanguage, StringComparison.OrdinalIgnoreCase))) continue;

                    var translated = ReadString(translation, property);
                    if (string.IsNullOrWhiteSpace(translated)) translated = ReadString(translation, "name");
                    if (string.IsNullOrWhiteSpace(translated)) translated = ReadString(translation, "overview");
                    if (!string.IsNullOrWhiteSpace(translated)) return translated;
                }
            }
        }

        return ReadString(element, property);
    }

    private static string ReadString(JsonElement element, string property)
    {
        return element.TryGetProperty(property, out var value) && value.ValueKind != JsonValueKind.Null
            ? value.ToString()
            : string.Empty;
    }

    private static int ReadInt(JsonElement element, string property)
    {
        if (!element.TryGetProperty(property, out var value)) return 0;
        return value.ValueKind switch
        {
            JsonValueKind.Number => value.GetInt32(),
            JsonValueKind.String => int.TryParse(value.GetString(), out var parsed) ? parsed : 0,
            _ => 0
        };
    }
}
