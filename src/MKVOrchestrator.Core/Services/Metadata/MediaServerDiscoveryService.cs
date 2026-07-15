using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Xml.Linq;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services.Metadata;

/// <summary>
/// Discovers library paths from Emby, Jellyfin, or Plex servers and maps the
/// server-reported paths to local/container paths via user-configured prefixes.
/// Shared by the web host and available to the desktop app.
/// </summary>
public sealed class MediaServerDiscoveryService
{
    private static readonly HttpClient Client = new() { Timeout = TimeSpan.FromSeconds(15) };

    public async Task<IReadOnlyList<MediaServerLibraryPath>> DiscoverLibrariesAsync(
        MediaServerSettings server,
        IReadOnlyList<MediaServerPathMapping> mappings,
        CancellationToken token)
    {
        var type = NormalizeServerType(server.Type);
        if (type == "Plex")
        {
            return await DiscoverPlexLibrariesAsync(server, mappings, token);
        }

        return await DiscoverEmbyLikeLibrariesAsync(server, mappings, token);
    }

    public static string NormalizeServerType(string? type)
    {
        return (type ?? string.Empty).Trim().ToLowerInvariant() switch
        {
            "jellyfin" => "Jellyfin",
            "plex" => "Plex",
            _ => "Emby"
        };
    }

    public static string MapServerPath(string serverPath, IReadOnlyList<MediaServerPathMapping> mappings)
    {
        var source = serverPath.Trim();
        foreach (var mapping in mappings.OrderByDescending(mapping => mapping.ServerPathPrefix?.Length ?? 0))
        {
            var serverPrefix = TrimPathEnd(mapping.ServerPathPrefix);
            var containerPrefix = TrimPathEnd(mapping.ContainerPathPrefix);
            if (string.IsNullOrWhiteSpace(serverPrefix) || string.IsNullOrWhiteSpace(containerPrefix)) continue;

            if (!TrimPathEnd(source).StartsWith(serverPrefix, StringComparison.OrdinalIgnoreCase)) continue;

            var suffix = source.Length > serverPrefix.Length
                ? source[serverPrefix.Length..].TrimStart('\\', '/')
                : string.Empty;
            var mapped = string.IsNullOrWhiteSpace(suffix)
                ? containerPrefix
                : $"{containerPrefix.TrimEnd('\\', '/')}/{suffix.Replace('\\', '/')}";
            return CrossPlatformRuntime.NormalizeUserPath(mapped);
        }

        return CrossPlatformRuntime.NormalizeUserPath(source);
    }

    public static string TrimPathEnd(string? path)
        => (path ?? string.Empty).Trim().TrimEnd('\\', '/');

    public static string GetDisplayPathName(string path)
    {
        var clean = TrimPathEnd(path);
        if (string.IsNullOrWhiteSpace(clean)) return "Library";
        var slash = Math.Max(clean.LastIndexOf('/'), clean.LastIndexOf('\\'));
        return slash >= 0 && slash < clean.Length - 1 ? clean[(slash + 1)..] : clean;
    }

    public static string CreateStableLibraryId(string serverId, string path)
    {
        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes($"{serverId}:{path}".ToLowerInvariant()));
        return Convert.ToHexString(bytes)[..16].ToLowerInvariant();
    }

    private async Task<IReadOnlyList<MediaServerLibraryPath>> DiscoverEmbyLikeLibrariesAsync(
        MediaServerSettings server,
        IReadOnlyList<MediaServerPathMapping> mappings,
        CancellationToken token)
    {
        var endpoints = new[]
        {
            "Library/VirtualFolders",
            "emby/Library/VirtualFolders"
        };

        foreach (var endpoint in endpoints)
        {
            var json = await TryGetTextAsync(server, endpoint, usePlexToken: false, token);
            if (string.IsNullOrWhiteSpace(json)) continue;

            using var document = JsonDocument.Parse(json);
            var libraries = ParseEmbyVirtualFolders(document.RootElement, server, mappings);
            if (libraries.Count > 0)
            {
                return libraries;
            }
        }

        var pathEndpoints = new[]
        {
            "Library/PhysicalPaths",
            "emby/Library/PhysicalPaths"
        };

        foreach (var endpoint in pathEndpoints)
        {
            var json = await TryGetTextAsync(server, endpoint, usePlexToken: false, token);
            if (string.IsNullOrWhiteSpace(json)) continue;

            using var document = JsonDocument.Parse(json);
            var libraries = ParsePhysicalPaths(document.RootElement, server, mappings);
            if (libraries.Count > 0)
            {
                return libraries;
            }
        }

        throw new InvalidOperationException("Connected to the server, but no library paths were returned. Check the API key permissions and server type.");
    }

    private async Task<IReadOnlyList<MediaServerLibraryPath>> DiscoverPlexLibrariesAsync(
        MediaServerSettings server,
        IReadOnlyList<MediaServerPathMapping> mappings,
        CancellationToken token)
    {
        var xml = await TryGetTextAsync(server, "library/sections", usePlexToken: true, token);
        if (string.IsNullOrWhiteSpace(xml))
        {
            throw new InvalidOperationException("Plex did not return library sections. Check the server URL and token.");
        }

        var document = XDocument.Parse(xml);
        var rows = new List<MediaServerLibraryPath>();
        foreach (var directory in document.Descendants("Directory"))
        {
            var name = directory.Attribute("title")?.Value ?? "Library";
            var type = directory.Attribute("type")?.Value ?? "library";
            foreach (var location in directory.Elements("Location"))
            {
                var serverPath = location.Attribute("path")?.Value ?? string.Empty;
                AddDiscoveredLibrary(rows, server, mappings, name, type, serverPath);
            }
        }

        return DistinctLibraries(rows);
    }

    private static async Task<string?> TryGetTextAsync(MediaServerSettings server, string relativePath, bool usePlexToken, CancellationToken token)
    {
        var url = BuildServerUrl(server.ServerUrl, relativePath, server.ApiKey, usePlexToken);
        using var request = new HttpRequestMessage(HttpMethod.Get, url);
        if (!string.IsNullOrWhiteSpace(server.ApiKey) && !usePlexToken)
        {
            request.Headers.TryAddWithoutValidation("X-Emby-Token", server.ApiKey.Trim());
            request.Headers.TryAddWithoutValidation("X-MediaBrowser-Token", server.ApiKey.Trim());
        }

        using var response = await Client.SendAsync(request, token);
        if (!response.IsSuccessStatusCode)
        {
            return null;
        }

        return await response.Content.ReadAsStringAsync(token);
    }

    private static string BuildServerUrl(string serverUrl, string relativePath, string apiKey, bool usePlexToken)
    {
        var trimmedServer = (serverUrl ?? string.Empty).Trim().TrimEnd('/');
        if (string.IsNullOrWhiteSpace(trimmedServer))
        {
            throw new InvalidOperationException("Enter a server URL.");
        }

        var relative = relativePath.TrimStart('/');
        var url = $"{trimmedServer}/{relative}";

        // Emby/Jellyfin authenticate through headers; only Plex requires its token in the query.
        if (usePlexToken && !string.IsNullOrWhiteSpace(apiKey))
        {
            var separator = relative.Contains('?') ? '&' : '?';
            url += $"{separator}X-Plex-Token={Uri.EscapeDataString(apiKey.Trim())}";
        }

        return url;
    }

    private static IReadOnlyList<MediaServerLibraryPath> ParseEmbyVirtualFolders(
        JsonElement root,
        MediaServerSettings server,
        IReadOnlyList<MediaServerPathMapping> mappings)
    {
        var rows = new List<MediaServerLibraryPath>();
        if (root.ValueKind != JsonValueKind.Array) return rows;

        foreach (var item in root.EnumerateArray())
        {
            var name = GetJsonString(item, "Name") ?? GetJsonString(item, "name") ?? "Library";
            var type = GetJsonString(item, "CollectionType") ?? GetJsonString(item, "collectionType") ?? string.Empty;
            var locations = GetJsonStringArray(item, "Locations")
                .Concat(GetJsonStringArray(item, "locations"))
                .DefaultIfEmpty(GetJsonString(item, "Path") ?? GetJsonString(item, "path") ?? string.Empty);

            foreach (var serverPath in locations)
            {
                AddDiscoveredLibrary(rows, server, mappings, name, type, serverPath);
            }
        }

        return DistinctLibraries(rows);
    }

    private static IReadOnlyList<MediaServerLibraryPath> ParsePhysicalPaths(
        JsonElement root,
        MediaServerSettings server,
        IReadOnlyList<MediaServerPathMapping> mappings)
    {
        var rows = new List<MediaServerLibraryPath>();
        if (root.ValueKind != JsonValueKind.Array) return rows;

        foreach (var item in root.EnumerateArray())
        {
            var serverPath = item.ValueKind == JsonValueKind.String
                ? item.GetString() ?? string.Empty
                : GetJsonString(item, "Path") ?? GetJsonString(item, "path") ?? string.Empty;
            AddDiscoveredLibrary(rows, server, mappings, GetDisplayPathName(serverPath), string.Empty, serverPath);
        }

        return DistinctLibraries(rows);
    }

    private static IEnumerable<string> GetJsonStringArray(JsonElement item, string propertyName)
    {
        if (!item.TryGetProperty(propertyName, out var property) || property.ValueKind != JsonValueKind.Array)
        {
            return Array.Empty<string>();
        }

        return property.EnumerateArray()
            .Where(value => value.ValueKind == JsonValueKind.String)
            .Select(value => value.GetString() ?? string.Empty)
            .Where(value => !string.IsNullOrWhiteSpace(value));
    }

    private static string? GetJsonString(JsonElement item, string propertyName)
    {
        if (!item.TryGetProperty(propertyName, out var property) || property.ValueKind != JsonValueKind.String)
        {
            return null;
        }

        return property.GetString();
    }

    private static void AddDiscoveredLibrary(
        List<MediaServerLibraryPath> rows,
        MediaServerSettings server,
        IReadOnlyList<MediaServerPathMapping> mappings,
        string name,
        string type,
        string serverPath)
    {
        if (string.IsNullOrWhiteSpace(serverPath)) return;

        var containerPath = MapServerPath(serverPath, mappings);
        rows.Add(new MediaServerLibraryPath
        {
            Id = CreateStableLibraryId(server.Id, serverPath),
            Name = string.IsNullOrWhiteSpace(name) ? GetDisplayPathName(serverPath) : name.Trim(),
            Type = type.Trim(),
            ServerPath = serverPath.Trim(),
            ContainerPath = containerPath,
            IsEnabled = true
        });
    }

    private static IReadOnlyList<MediaServerLibraryPath> DistinctLibraries(IEnumerable<MediaServerLibraryPath> libraries)
    {
        return libraries
            .Where(library => !string.IsNullOrWhiteSpace(library.ContainerPath))
            .GroupBy(library => library.ContainerPath, StringComparer.OrdinalIgnoreCase)
            .Select(group => group.First())
            .OrderBy(library => library.Name, StringComparer.OrdinalIgnoreCase)
            .ThenBy(library => library.ContainerPath, StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }
}
