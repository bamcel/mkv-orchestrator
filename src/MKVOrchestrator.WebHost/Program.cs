using System.Diagnostics;
using System.Collections.Concurrent;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Cache;
using MKVOrchestrator.Core.Services.Metadata;
using MKVOrchestrator.Core.Services.Rename;
using MKVOrchestrator.WebHost;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddSingleton(_ => new MkvScannerService(new MetadataCacheDatabase("metadata_cache_web.db")));
builder.Services.AddSingleton<MkvMergeService>();
builder.Services.AddSingleton<MkvPropEditCommandBuilder>();
builder.Services.AddSingleton<MkvPropEditService>();
builder.Services.AddSingleton<AppSettingsService>();
builder.Services.AddSingleton<RenameBatchHistoryService>();
builder.Services.AddSingleton<FileConflictService>();
builder.Services.AddSingleton<MediaServerDiscoveryService>();
builder.Services.AddSingleton<ScanJobStore>();
builder.Services.AddSingleton<OperationJobStore>();
builder.Services.AddSingleton<CurrentScanStore>();
builder.Services.AddSingleton<OperationLogStore>();
builder.Services.AddSingleton<RenameEpisodeCache>();
builder.Services.AddSingleton<WatchFolderMonitorService>();
builder.Services.AddHostedService(provider => provider.GetRequiredService<WatchFolderMonitorService>());
builder.Services.AddRouting(options => options.LowercaseUrls = true);

var app = builder.Build();

var mediaRoot = ResolveMediaRoot();
var sourceRoots = ResolveSourceRoots(mediaRoot);
Directory.CreateDirectory(CrossPlatformRuntime.AppDataDirectory);
if (!Directory.Exists(mediaRoot))
{
    Directory.CreateDirectory(mediaRoot);
}

// Prune cache rows that have not been refreshed in 30 days so the web cache
// database does not grow without bound across container restarts.
try
{
    app.Services.GetRequiredService<MkvScannerService>().Cache.RemoveOlderThan(DateTime.UtcNow.AddDays(-30));
}
catch
{
    // Cache pruning is best effort; a locked or corrupt cache should not block startup.
}

// Optional HTTP basic auth. When MKVO_AUTH_USERNAME and MKVO_AUTH_PASSWORD are
// both set, every request except the container healthcheck requires credentials.
var authUsername = Environment.GetEnvironmentVariable("MKVO_AUTH_USERNAME");
var authPassword = Environment.GetEnvironmentVariable("MKVO_AUTH_PASSWORD");
if (!string.IsNullOrWhiteSpace(authUsername) && !string.IsNullOrWhiteSpace(authPassword))
{
    app.Use(async (context, next) =>
    {
        if (context.Request.Path.StartsWithSegments("/api/health"))
        {
            await next();
            return;
        }

        if (IsBasicAuthAuthorized(context.Request.Headers.Authorization.ToString(), authUsername, authPassword))
        {
            await next();
            return;
        }

        context.Response.StatusCode = StatusCodes.Status401Unauthorized;
        context.Response.Headers.WWWAuthenticate = "Basic realm=\"MKV Orchestrator\", charset=\"UTF-8\"";
    });
}

app.UseDefaultFiles();
app.UseStaticFiles();

// Bundled tool paths and versions do not change while the container is running,
// so resolve them once instead of spawning six processes on every status call.
var cachedToolStatuses = new Lazy<ToolStatus[]>(() => new[]
{
    ToolStatus.Create("mkvmerge", "mkvmerge.exe", "mkvmerge"),
    ToolStatus.Create("mkvpropedit", "mkvpropedit.exe", "mkvpropedit"),
    ToolStatus.Create("mkvextract", "mkvextract.exe", "mkvextract"),
    ToolStatus.Create("mkvinfo", "mkvinfo.exe", "mkvinfo"),
    ToolStatus.Create("ffmpeg", "ffmpeg.exe", "ffmpeg"),
    ToolStatus.Create("ffprobe", "ffprobe.exe", "ffprobe")
}, LazyThreadSafetyMode.ExecutionAndPublication);

app.MapGet("/api/status", (AppSettingsService settingsService) =>
{
    return Results.Ok(new AppStatusResponse(
        Name: "MKV Orchestrator Web",
        Version: typeof(MkvScannerService).Assembly.GetName().Version?.ToString() ?? "dev",
        MediaRoot: mediaRoot,
        ConfigRoot: CrossPlatformRuntime.AppDataDirectory,
        SourceRoots: BuildStatusSourceRoots(sourceRoots, BuildSettingsSnapshot(settingsService)),
        Tools: cachedToolStatuses.Value));
});

app.MapGet("/api/filesystem", (string? path, AppSettingsService settingsService) =>
{
    var target = string.IsNullOrWhiteSpace(path)
        ? mediaRoot
        : CrossPlatformRuntime.NormalizeUserPath(path);

    var allowedRoots = BuildAllowedBrowseRoots(mediaRoot, sourceRoots, settingsService.Load());
    if (!IsPathUnderAllowedRoots(target, allowedRoots))
    {
        return Results.Json(new ApiError("Browsing is limited to configured media source roots."), statusCode: StatusCodes.Status403Forbidden);
    }

    if (!Directory.Exists(target))
    {
        return Results.NotFound(new ApiError($"Directory not found: {target}"));
    }

    try
    {
        var directory = new DirectoryInfo(target);
        var parent = directory.Parent?.FullName;
        if (parent is not null && !IsPathUnderAllowedRoots(parent, allowedRoots))
        {
            parent = null;
        }

        var entries = directory.EnumerateFileSystemInfos()
            .Where(info => IsVisibleBrowseEntry(info))
            .OrderByDescending(info => info is DirectoryInfo)
            .ThenBy(info => info.Name, StringComparer.OrdinalIgnoreCase)
            .Select(info => new FileSystemEntry(
                Name: info.Name,
                Path: info.FullName,
                Kind: info is DirectoryInfo ? "folder" : "file",
                SizeBytes: info is FileInfo file ? file.Length : null,
                ModifiedUtc: info.LastWriteTimeUtc))
            .ToArray();

        return Results.Ok(new FileSystemResponse(directory.FullName, parent, entries));
    }
    catch (Exception ex)
    {
        return Results.Problem(ex.Message, statusCode: StatusCodes.Status500InternalServerError);
    }
});

app.MapPost("/api/scans", (ScanRequest request, MkvScannerService scanner, ScanJobStore jobs, CurrentScanStore currentScan) =>
{
    var job = jobs.Create();
    _ = Task.Run(() => RunScanJobAsync(job, request, scanner, mediaRoot, currentScan, job.Token));
    return Results.Accepted($"/api/scans/{job.Id}", job.ToResponse());
});

app.MapGet("/api/scans/{id}", (string id, ScanJobStore jobs) =>
{
    return jobs.TryGet(id, out var job)
        ? Results.Ok(job.ToResponse())
        : Results.NotFound(new ApiError($"Scan job not found: {id}"));
});

app.MapPost("/api/scans/{id}/cancel", (string id, ScanJobStore jobs) =>
{
    if (!jobs.TryGet(id, out var job))
    {
        return Results.NotFound(new ApiError($"Scan job not found: {id}"));
    }

    job.Cancel();
    return Results.Ok(job.ToResponse());
});

app.MapGet("/api/files/current", (CurrentScanStore currentScan) => Results.Ok(currentScan.ToResponse()));

app.MapDelete("/api/files/current", (CurrentScanStore currentScan, OperationLogStore logs) =>
{
    currentScan.Clear();
    logs.Add("Library", "Library scan cache cleared", "The current web scan cache was cleared.");
    return Results.Ok(currentScan.ToResponse());
});

app.MapGet("/api/settings", (AppSettingsService settingsService) =>
{
    var settings = BuildSettingsSnapshot(settingsService);
    return Results.Ok(WebSettingsResponse.From(settings));
});

app.MapPut("/api/settings", (WebSettingsRequest request, AppSettingsService settingsService, WatchFolderMonitorService watchMonitor) =>
{
    var settings = settingsService.Load();
    settings.TvdbApiKey = request.TvdbApiKey?.Trim() ?? settings.TvdbApiKey;
    settings.TvdbPin = request.TvdbPin?.Trim() ?? settings.TvdbPin;
    settings.TmdbApiKey = request.TmdbApiKey?.Trim() ?? settings.TmdbApiKey;
    settings.TvdbLanguage = string.IsNullOrWhiteSpace(request.TvdbLanguage) ? "eng" : request.TvdbLanguage.Trim();
    settings.RenameLookupProvider = NormalizeLookupProvider(request.RenameLookupProvider);
    settings.RenameTemplate = string.IsNullOrWhiteSpace(request.RenameTemplate)
        ? settings.RenameTemplate
        : request.RenameTemplate.Trim();
    if (request.RenameTemplates is not null)
    {
        settings.RenameTemplates = request.RenameTemplates
            .Select(template => template?.Trim() ?? string.Empty)
            .Where(template => !string.IsNullOrWhiteSpace(template))
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .DefaultIfEmpty(settings.RenameTemplate)
            .ToList();
    }
    if (request.AudioNamePresets is not null)
    {
        settings.AudioNamePresets = NormalizeStringList(request.AudioNamePresets, settings.AudioNamePresets);
    }
    if (request.SubtitleNamePresets is not null)
    {
        settings.SubtitleNamePresets = NormalizeStringList(request.SubtitleNamePresets, settings.SubtitleNamePresets);
    }
    if (request.LanguagePresets is not null)
    {
        settings.LanguagePresets = NormalizeStringList(request.LanguagePresets, settings.LanguagePresets);
    }
    if (request.MkvMergeDefaultAudioLanguages is not null)
    {
        settings.MkvMergeDefaultAudioLanguages = string.IsNullOrWhiteSpace(request.MkvMergeDefaultAudioLanguages)
            ? "eng,jpn"
            : request.MkvMergeDefaultAudioLanguages.Trim();
    }
    if (request.MkvMergeDefaultSubtitleLanguages is not null)
    {
        settings.MkvMergeDefaultSubtitleLanguages = string.IsNullOrWhiteSpace(request.MkvMergeDefaultSubtitleLanguages)
            ? "eng"
            : request.MkvMergeDefaultSubtitleLanguages.Trim();
    }
    if (request.WatchFolders is not null)
    {
        settings.WatchFolders = NormalizeStringList(request.WatchFolders, Array.Empty<string>());
    }
    if (request.EnableLiveWatchFolderMonitoring is not null)
    {
        settings.EnableLiveWatchFolderMonitoring = request.EnableLiveWatchFolderMonitoring.Value;
    }
    if (request.MediaServers is not null)
    {
        settings.MediaServers = NormalizeMediaServers(request.MediaServers, settings.MediaServers);
    }
    if (request.MediaServerPathMappings is not null)
    {
        settings.MediaServerPathMappings = NormalizeMediaServerPathMappings(request.MediaServerPathMappings);
    }

    settingsService.Save(settings);
    _ = watchMonitor.RestartAsync();
    return Results.Ok(WebSettingsResponse.From(BuildSettingsSnapshot(settingsService)));
});

app.MapPost("/api/media-servers/test", async (MediaServerConnectionRequest request, AppSettingsService settingsService, MediaServerDiscoveryService discovery, CancellationToken token) =>
{
    var settings = settingsService.Load();
    var server = ResolveMediaServerRequest(request, settings);

    if (string.IsNullOrWhiteSpace(server.ServerUrl))
    {
        return Results.BadRequest(new ApiError("Enter a server URL."));
    }

    try
    {
        var libraries = await discovery.DiscoverLibrariesAsync(server, settings.MediaServerPathMappings, token);
        var status = libraries.Count == 0
            ? "Connection succeeded, but no library paths were returned."
            : $"Connection succeeded. Found {libraries.Count} library path(s).";
        return Results.Ok(new MediaServerTestResponse(true, status, libraries.Count));
    }
    catch (Exception ex)
    {
        return Results.Ok(new MediaServerTestResponse(false, ex.Message, 0));
    }
});

app.MapPost("/api/media-servers/{id}/sync", async (string id, AppSettingsService settingsService, MediaServerDiscoveryService discovery, OperationLogStore logs, CancellationToken token) =>
{
    var settings = settingsService.Load();
    var server = settings.MediaServers.FirstOrDefault(item => item.Id.Equals(id, StringComparison.OrdinalIgnoreCase));
    if (server is null)
    {
        return Results.NotFound(new ApiError($"Media server not found: {id}"));
    }

    try
    {
        server.Libraries = (await discovery.DiscoverLibrariesAsync(server, settings.MediaServerPathMappings, token)).ToList();
        server.LastSyncedUtc = DateTimeOffset.UtcNow;
        settingsService.Save(settings);

        var responseServer = WebMediaServerResponse.From(server);
        var status = server.Libraries.Count == 0
            ? $"Sync complete for {server.Name}. No library paths were returned."
            : $"Sync complete for {server.Name}: {server.Libraries.Count} library path(s).";
        logs.Add("Library", $"Media server synced: {server.Name}", status);
        return Results.Ok(new MediaServerSyncResponse(responseServer, responseServer.Libraries, status));
    }
    catch (Exception ex)
    {
        return Results.BadRequest(new ApiError(ex.Message));
    }
});

app.MapPost("/api/rename/search", async (RenameSearchRequest request, AppSettingsService settingsService, CancellationToken token) =>
{
    var query = request.Query?.Trim() ?? string.Empty;
    if (string.IsNullOrWhiteSpace(query))
    {
        return Results.BadRequest(new ApiError("Enter a title to search."));
    }

    var settings = BuildSettingsSnapshot(settingsService, request.Provider, request.Language);
    var provider = GetRenameMetadataProvider(settings.RenameLookupProvider);
    var results = await provider.SearchSeriesAsync(query, settings.TvdbLanguage, settings, token);
    foreach (var result in results)
    {
        result.Provider = provider.Key;
    }

    return Results.Ok(new RenameSearchResponse(results));
});

app.MapPost("/api/rename/test-provider", async (RenameProviderTestRequest request, AppSettingsService settingsService, CancellationToken token) =>
{
    var settings = BuildSettingsSnapshot(settingsService, request.Provider, request.Language);
    var providerName = settings.RenameLookupProvider;
    var provider = GetRenameMetadataProvider(providerName);

    try
    {
        var results = await provider.SearchSeriesAsync("test", settings.TvdbLanguage, settings, token);
        return Results.Ok(new RenameProviderTestResponse(true, $"{providerName} API connection successful. Results returned: {results.Count}"));
    }
    catch (Exception ex)
    {
        return Results.Ok(new RenameProviderTestResponse(false, $"{providerName} API connection failed: {ex.Message}"));
    }
});

app.MapPost("/api/rename/scopes", async (RenameScopesRequest request, AppSettingsService settingsService, RenameEpisodeCache episodeCache, CancellationToken token) =>
{
    if (request.SelectedResult is null)
    {
        return Results.BadRequest(new ApiError("Select a database result first."));
    }

    var settings = BuildSettingsSnapshot(settingsService, request.Provider, request.Language);
    var episodes = await LoadRenameEpisodesAsync(request.SelectedResult, settings, episodeCache, token);
    return Results.Ok(new RenameScopesResponse(BuildRenameScopeOptions(episodes, request.SelectedResult)));
});

app.MapPost("/api/rename/preview", async (RenamePreviewRequest request, AppSettingsService settingsService, RenameEpisodeCache episodeCache, CancellationToken token) =>
{
    if (request.SelectedResult is null)
    {
        return Results.BadRequest(new ApiError("Select a database result first."));
    }

    var sourceFiles = BuildRenameSourceFiles(request.Files ?? Array.Empty<MediaFileRow>());
    if (sourceFiles.Count == 0)
    {
        return Results.BadRequest(new ApiError("Scan and select files before building a rename preview."));
    }

    var settings = BuildSettingsSnapshot(settingsService, request.Provider, request.Language, request.Template);
    var allEpisodes = await LoadRenameEpisodesAsync(request.SelectedResult, settings, episodeCache, token);
    var scopeOptions = BuildRenameScopeOptions(allEpisodes, request.SelectedResult);
    var selectedScopes = NormalizeRenameScopeKeys(request.ScopeKeys, request.ScopeKey, scopeOptions);
    var episodes = FilterRenameEpisodes(allEpisodes, selectedScopes, request.SelectedResult);
    var preview = BuildRenamePreviewRows(sourceFiles, episodes, request.SelectedResult, settings.RenameTemplate);

    return Results.Ok(new RenamePreviewResponse(
        Items: preview.Items,
        Summary: preview.Summary,
        Scopes: scopeOptions,
        Status: preview.Status));
});

app.MapPost("/api/rename/apply", (RenameApplyRequest request, OperationLogStore logs, RenameBatchHistoryService renameBatches) =>
{
    var requestedRows = request.Items ?? Array.Empty<RenamePreviewRow>();
    var rows = ApplyRenameRows(requestedRows);
    var renamed = rows.Count(row => row.Status.Equals("Renamed", StringComparison.OrdinalIgnoreCase));
    var skipped = rows.Count - renamed;
    var batchEntries = rows
        .Select((row, index) => new { Row = row, Original = index < requestedRows.Count ? requestedRows[index] : null })
        .Where(item => item.Original is not null && item.Row.Status.Equals("Renamed", StringComparison.OrdinalIgnoreCase))
        .Select(item => new RenameBatchEntry
        {
            OriginalPath = item.Original!.SourcePath,
            RenamedPath = item.Row.SourcePath
        })
        .ToList();

    if (batchEntries.Count > 0)
    {
        renameBatches.RecordBatch(new RenameBatchRecord
        {
            CreatedAt = DateTime.Now,
            Provider = NormalizeLookupProvider(request.Provider),
            Template = request.Template?.Trim() ?? string.Empty,
            TotalFiles = batchEntries.Count,
            Entries = batchEntries
        });
    }

    var summary = BuildRenameSummary(rows, renamed, skipped, dryRun: false);
    logs.Add("Rename", $"Rename complete: {renamed} renamed, {skipped} skipped", summary);
    return Results.Ok(new RenameApplyResponse(rows, summary, $"Rename complete: {renamed} renamed, {skipped} skipped"));
});

app.MapGet("/api/rename/batches", (RenameBatchHistoryService renameBatches) =>
{
    return Results.Ok(new RenameBatchListResponse(renameBatches.Load()));
});

app.MapGet("/api/rename/batches/{id}/preview", (string id, RenameBatchHistoryService renameBatches) =>
{
    var batch = FindRenameBatch(renameBatches, id);
    if (batch is null)
    {
        return Results.NotFound(new ApiError($"Rename batch not found: {id}"));
    }

    return Results.Ok(RenameBatchUndoPreviewResponse.From(renameBatches.PreviewUndoBatch(batch)));
});

app.MapPost("/api/rename/batches/{id}/undo", (string id, RenameBatchHistoryService renameBatches, OperationLogStore logs) =>
{
    var batch = FindRenameBatch(renameBatches, id);
    if (batch is null)
    {
        return Results.NotFound(new ApiError($"Rename batch not found: {id}"));
    }

    var result = renameBatches.UndoBatch(batch);
    var restored = BuildRestoredMoves(batch);
    var detail = string.Join(Environment.NewLine, result.Lines);
    logs.Add("Rename", $"Undo batch complete: {result.Renamed} restored, {result.Skipped} skipped", detail);
    return Results.Ok(new RenameBatchUndoResponse(result.Renamed, result.Skipped, result.Lines, restored));
});

app.MapDelete("/api/rename/batches", (RenameBatchHistoryService renameBatches) =>
{
    renameBatches.Clear();
    return Results.Ok(new RenameBatchListResponse(Array.Empty<RenameBatchRecord>()));
});

app.MapPost("/api/mux/preview", (MuxPreviewRequest request, MkvMergeService muxService) =>
{
    var plan = BuildMuxPlan(request, muxService);
    return Results.Ok(BuildMuxPreviewResponse(plan, dryRun: true, completed: 0, failed: 0));
});

app.MapPost("/api/mux/apply", (MuxPreviewRequest request, MkvMergeService muxService, MkvScannerService scanner, CurrentScanStore currentScan, OperationLogStore logs, OperationJobStore operations, FileConflictService fileConflicts) =>
{
    var plan = BuildMuxPlan(request, muxService);
    var job = operations.Create("mux", plan.Actions.Count);
    _ = Task.Run(() => RunMuxJobAsync(job, plan, muxService, scanner, currentScan, logs, fileConflicts));
    return Results.Accepted($"/api/operations/{job.Id}", job.ToResponse());
});

app.MapGet("/api/operations/{id}", (string id, OperationJobStore operations) =>
{
    return operations.TryGet(id, out var job)
        ? Results.Ok(job.ToResponse())
        : Results.NotFound(new ApiError($"Operation not found: {id}"));
});

app.MapPost("/api/operations/{id}/cancel", (string id, OperationJobStore operations) =>
{
    if (!operations.TryGet(id, out var job))
    {
        return Results.NotFound(new ApiError($"Operation not found: {id}"));
    }

    job.Cancel();
    return Results.Ok(job.ToResponse());
});

app.MapPost("/api/propedit/template", (PropEditTemplateRequest request) =>
{
    var files = request.Files?.Select(row => ToMkvFileItem(row)).ToList() ?? new List<MkvFileItem>();
    var template = FindTemplate(files, request.TemplatePath);
    if (template is null)
    {
        return Results.BadRequest(new ApiError("Select a scanned MKV template file first."));
    }

    return Results.Ok(BuildPropEditTemplateResponse(template));
});

app.MapPost("/api/propedit/preview", (PropEditPreviewRequest request, MkvPropEditCommandBuilder builder) =>
{
    var plan = BuildPropEditPlan(request, builder);
    return Results.Ok(BuildPropEditPreviewResponse(plan, dryRun: true, completed: 0, failed: 0));
});

app.MapPost("/api/propedit/apply", (PropEditPreviewRequest request, MkvPropEditCommandBuilder builder, MkvPropEditService propEdit, MkvScannerService scanner, CurrentScanStore currentScan, OperationLogStore logs, OperationJobStore operations, FileConflictService fileConflicts) =>
{
    var plan = BuildPropEditPlan(request, builder);
    var job = operations.Create("propedit", plan.Actions.Count);
    _ = Task.Run(() => RunPropEditJobAsync(job, plan, propEdit, scanner, currentScan, logs, fileConflicts));
    return Results.Accepted($"/api/operations/{job.Id}", job.ToResponse());
});

app.MapPost("/api/library/audit", (LibraryAuditRequest request) =>
{
    var rows = (request.Files ?? Array.Empty<MediaFileRow>())
        .Where(row => !string.IsNullOrWhiteSpace(row.Path))
        .OrderBy(row => row.Path, StringComparer.OrdinalIgnoreCase)
        .ToArray();
    return Results.Ok(BuildLibraryAuditResponse(rows));
});

app.MapGet("/api/logs", (OperationLogStore logs) => Results.Ok(new OperationLogResponse(logs.List())));

app.MapDelete("/api/logs", (OperationLogStore logs) =>
{
    logs.Clear();
    return Results.Ok(new OperationLogResponse(logs.List()));
});

app.MapGet("/api/health", () => Results.Ok(new { status = "ok" }));

app.MapFallbackToFile("index.html");

app.Run();

static string ResolveMediaRoot()
{
    var configured = Environment.GetEnvironmentVariable("MKVO_MEDIA_ROOT");
    if (!string.IsNullOrWhiteSpace(configured))
    {
        return CrossPlatformRuntime.NormalizeUserPath(configured);
    }

    if (Directory.Exists("/media"))
    {
        return "/media";
    }

    var current = Directory.GetCurrentDirectory();
    var localMedia = Path.GetFullPath(Path.Combine(current, "media"));
    return Directory.Exists(localMedia) ? localMedia : current;
}

static IReadOnlyList<SourceRoot> ResolveSourceRoots(string mediaRoot)
{
    var roots = new List<SourceRoot>
    {
        new("media", mediaRoot)
    };

    var configured = Environment.GetEnvironmentVariable("MKVO_SOURCE_ROOTS");
    if (string.IsNullOrWhiteSpace(configured))
    {
        return roots;
    }

    foreach (var item in configured.Split(new[] { ';', ',', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
    {
        var separator = item.IndexOf('=');
        if (separator <= 0 || separator >= item.Length - 1) continue;

        var name = item[..separator].Trim();
        var path = CrossPlatformRuntime.NormalizeUserPath(item[(separator + 1)..].Trim());
        if (string.IsNullOrWhiteSpace(name) || string.IsNullOrWhiteSpace(path)) continue;
        if (roots.Any(root => root.Name.Equals(name, StringComparison.OrdinalIgnoreCase))) continue;

        roots.Add(new SourceRoot(name, path));
    }

    return roots;
}

static IReadOnlyList<SourceRoot> BuildStatusSourceRoots(IReadOnlyList<SourceRoot> baseRoots, AppSettings settings)
{
    var roots = new List<SourceRoot>();
    var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    void Add(string name, string path)
    {
        var normalized = CrossPlatformRuntime.NormalizeUserPath(path);
        if (string.IsNullOrWhiteSpace(name) || string.IsNullOrWhiteSpace(normalized)) return;
        if (!seen.Add(normalized)) return;
        roots.Add(new SourceRoot(name, normalized));
    }

    foreach (var root in baseRoots)
    {
        Add(root.Name, root.Path);
    }

    foreach (var folder in settings.WatchFolders)
    {
        Add($"Watch: {GetDisplayPathName(folder)}", folder);
    }

    foreach (var server in settings.MediaServers)
    {
        var serverName = string.IsNullOrWhiteSpace(server.Name) ? NormalizeMediaServerType(server.Type) : server.Name.Trim();
        foreach (var library in server.Libraries.Where(library => library.IsEnabled))
        {
            Add($"{serverName}: {library.Name}", library.ContainerPath);
        }
    }

    return roots;
}

static string TrimPathEnd(string? path)
    => MediaServerDiscoveryService.TrimPathEnd(path);

static string GetDisplayPathName(string path)
    => MediaServerDiscoveryService.GetDisplayPathName(path);

static string CreateStableLibraryId(string serverId, string path)
    => MediaServerDiscoveryService.CreateStableLibraryId(serverId, path);

static string NormalizeMediaServerType(string? type)
    => MediaServerDiscoveryService.NormalizeServerType(type);

static MediaServerSettings ResolveMediaServerRequest(MediaServerConnectionRequest request, AppSettings settings)
{
    var existing = !string.IsNullOrWhiteSpace(request.Id)
        ? settings.MediaServers.FirstOrDefault(server => server.Id.Equals(request.Id, StringComparison.OrdinalIgnoreCase))
        : null;

    return new MediaServerSettings
    {
        Id = existing?.Id ?? request.Id?.Trim() ?? Guid.NewGuid().ToString("N"),
        Name = FirstNonBlank(request.Name, existing?.Name, "Media Server"),
        Type = NormalizeMediaServerType(FirstNonBlank(request.Type, existing?.Type, "Emby")),
        ServerUrl = FirstNonBlank(request.ServerUrl, existing?.ServerUrl, string.Empty),
        ApiKey = FirstNonBlank(request.ApiKey, existing?.ApiKey, string.Empty),
        IsDefault = existing?.IsDefault ?? false,
        Libraries = existing?.Libraries ?? new List<MediaServerLibraryPath>(),
        LastSyncedUtc = existing?.LastSyncedUtc
    };
}

static List<MediaServerSettings> NormalizeMediaServers(
    IReadOnlyList<WebMediaServerRequest> requestServers,
    IReadOnlyList<MediaServerSettings> existingServers)
{
    var existingById = existingServers.ToDictionary(server => server.Id, StringComparer.OrdinalIgnoreCase);
    var servers = new List<MediaServerSettings>();

    foreach (var request in requestServers)
    {
        var id = string.IsNullOrWhiteSpace(request.Id) ? Guid.NewGuid().ToString("N") : request.Id.Trim();
        existingById.TryGetValue(id, out var existing);

        var server = new MediaServerSettings
        {
            Id = id,
            Name = FirstNonBlank(request.Name, existing?.Name, "Media Server"),
            Type = NormalizeMediaServerType(FirstNonBlank(request.Type, existing?.Type, "Emby")),
            ServerUrl = FirstNonBlank(request.ServerUrl, existing?.ServerUrl, string.Empty),
            ApiKey = FirstNonBlank(request.ApiKey, existing?.ApiKey, string.Empty),
            IsDefault = request.IsDefault,
            Libraries = NormalizeMediaServerLibraries(request.Libraries, existing?.Libraries ?? new List<MediaServerLibraryPath>()),
            LastSyncedUtc = existing?.LastSyncedUtc
        };

        if (string.IsNullOrWhiteSpace(server.ServerUrl)) continue;
        servers.Add(server);
    }

    if (servers.Count > 0 && servers.All(server => !server.IsDefault))
    {
        servers[0].IsDefault = true;
    }

    var defaultSet = false;
    foreach (var server in servers)
    {
        if (!server.IsDefault) continue;
        if (!defaultSet)
        {
            defaultSet = true;
            continue;
        }

        server.IsDefault = false;
    }

    return servers;
}

static List<MediaServerLibraryPath> NormalizeMediaServerLibraries(
    IReadOnlyList<WebMediaServerLibraryPath>? requestLibraries,
    IReadOnlyList<MediaServerLibraryPath> existingLibraries)
{
    if (requestLibraries is null)
    {
        return existingLibraries.ToList();
    }

    return requestLibraries
        .Where(library => !string.IsNullOrWhiteSpace(library.ContainerPath) || !string.IsNullOrWhiteSpace(library.ServerPath))
        .Select(library => new MediaServerLibraryPath
        {
            Id = string.IsNullOrWhiteSpace(library.Id) ? CreateStableLibraryId(string.Empty, library.ServerPath) : library.Id.Trim(),
            Name = FirstNonBlank(library.Name, GetDisplayPathName(library.ContainerPath), "Library"),
            Type = library.Type?.Trim() ?? string.Empty,
            ServerPath = library.ServerPath?.Trim() ?? string.Empty,
            ContainerPath = CrossPlatformRuntime.NormalizeUserPath(FirstNonBlank(library.ContainerPath, library.ServerPath, string.Empty)),
            IsEnabled = library.IsEnabled
        })
        .ToList();
}

static List<MediaServerPathMapping> NormalizeMediaServerPathMappings(IReadOnlyList<WebMediaServerPathMapping> mappings)
{
    return mappings
        .Select(mapping => new MediaServerPathMapping
        {
            ServerPathPrefix = TrimPathEnd(mapping.ServerPathPrefix),
            ContainerPathPrefix = TrimPathEnd(mapping.ContainerPathPrefix)
        })
        .Where(mapping => !string.IsNullOrWhiteSpace(mapping.ServerPathPrefix) && !string.IsNullOrWhiteSpace(mapping.ContainerPathPrefix))
        .DistinctBy(mapping => mapping.ServerPathPrefix, StringComparer.OrdinalIgnoreCase)
        .ToList();
}

static IReadOnlyList<string> NormalizeSources(ScanRequest request, string fallback)
{
    var sources = request.Sources?.Where(path => !string.IsNullOrWhiteSpace(path)).ToArray();
    if (sources is { Length: > 0 }) return sources;

    if (!string.IsNullOrWhiteSpace(request.SourcePath))
    {
        return new[] { request.SourcePath };
    }

    return new[] { fallback };
}

static IReadOnlyCollection<string> NormalizeIgnoredFolders(IEnumerable<string>? folders)
{
    return (folders ?? Array.Empty<string>())
        .SelectMany(value => value.Split(new[] { ',', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        .Where(value => !string.IsNullOrWhiteSpace(value))
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .ToArray();
}

static string ResolveToolCommand(string name)
    => name.Trim().ToLowerInvariant() switch
    {
        "mkvpropedit" => CrossPlatformRuntime.GetToolDisplayName("mkvpropedit.exe", "mkvpropedit"),
        "ffprobe" => CrossPlatformRuntime.GetToolDisplayName("ffprobe.exe", "ffprobe"),
        "ffmpeg" => CrossPlatformRuntime.GetToolDisplayName("ffmpeg.exe", "ffmpeg"),
        _ => CrossPlatformRuntime.GetToolDisplayName("mkvmerge.exe", "mkvmerge")
    };

static MkvFileItem ToMkvFileItem(MediaFileRow row, bool selected = true)
{
    var item = new MkvFileItem
    {
        FilePath = CrossPlatformRuntime.NormalizeUserPath(row.Path),
        Status = string.IsNullOrWhiteSpace(row.Status) ? "Ready" : row.Status,
        Codec = row.Codec,
        Resolution = row.Resolution,
        BitDepth = row.BitDepth,
        Hdr = row.Hdr,
        VideoSummary = row.VideoSummary,
        AudioSummary = row.AudioSummary,
        SubtitleSummary = row.SubtitleSummary,
        AttachmentSummary = row.AttachmentSummary,
        Selected = selected
    };

    foreach (var track in row.Tracks ?? Array.Empty<TrackRow>())
    {
        item.Tracks.Add(new MkvTrackItem
        {
            MkvMergeId = track.Id,
            PropEditTrackNumber = track.TrackNumber,
            Type = track.Type,
            Codec = track.Codec,
            Language = track.Language,
            Name = track.Name,
            Default = track.Default,
            Forced = track.Forced
        });
    }

    foreach (var attachment in row.Attachments ?? Array.Empty<AttachmentRow>())
    {
        item.Attachments.Add(new MkvAttachmentItem
        {
            Id = attachment.Id,
            FileName = attachment.FileName,
            ContentType = attachment.ContentType,
            Description = attachment.Description,
            SizeBytes = attachment.SizeBytes
        });
    }

    return item;
}

static MkvMergeRemuxPlan BuildMuxPlan(MuxPreviewRequest request, MkvMergeService muxService)
{
    var selectedPaths = BuildSelectedPathSet(request.SelectedPaths);
    var files = (request.Files ?? Array.Empty<MediaFileRow>())
        .Select(row => ToMkvFileItem(row, selectedPaths.Count == 0 || selectedPaths.Contains(row.Path)))
        .ToList();

    var plan = muxService.BuildRemuxPlan(
        files,
        request.KeepAudioLanguages ?? "eng,jpn",
        request.KeepSubtitleLanguages ?? "eng",
        request.RemoveUnwantedAudioLanguages,
        request.RemoveUnwantedSubtitleLanguages,
        request.RemoveUnwantedTrackIds,
        request.RemoveTrackIdsText ?? string.Empty,
        request.PreserveChapters,
        request.PreserveAttachments,
        request.MuxMatchingExternalSubtitles,
        request.ExternalSubtitleLanguage ?? "eng",
        "{tag}",
        request.ExternalSubtitleFormats ?? "srt,ass,ssa,sub,idx",
        request.PreserveExternalSubtitleFiles,
        request.SkipMuxIfSubtitleAlreadyExists,
        request.ExtractSubtitles,
        request.ExtractSubtitleLanguages ?? "eng",
        request.ExtractOverwriteExistingFiles);

    if (!request.ConvertMp4ToMkv)
    {
        return plan;
    }

    var convertPlan = muxService.BuildConvertToMkvPlan(files, request.DeleteMp4AfterConvert);
    plan.Actions.AddRange(convertPlan.Actions);

    // MP4s picked up by the conversion pass are no longer "no change".
    var actionPaths = plan.Actions
        .Select(action => action.SourceFilePath)
        .ToHashSet(StringComparer.OrdinalIgnoreCase);
    plan.NoChangeFiles.RemoveAll(actionPaths.Contains);

    return plan;
}

static MuxPreviewResponse BuildMuxPreviewResponse(
    MkvMergeRemuxPlan plan,
    bool dryRun,
    int completed,
    int failed,
    IReadOnlyList<string>? resultLines = null)
{
    var actions = plan.Actions
        .Select((action, index) => new MuxActionRow(
            Index: index + 1,
            FilePath: action.SourceFilePath,
            FileName: Path.GetFileName(action.SourceFilePath),
            Operation: action.Operation,
            ToolName: action.ToolName,
            Description: action.Description,
            Command: FormatCommand(action.ToolName, action.Arguments)))
        .ToArray();

    var lines = new List<string>
    {
        $"mkvmerge Summary - {(dryRun ? "DRY RUN" : "APPLY")}",
        $"Generated: {DateTime.Now:yyyy-MM-dd HH:mm:ss}",
        $"Planned actions: {actions.Length} | No Change: {plan.NoChangeFiles.Count}",
    };
    if (!dryRun)
    {
        lines.Add($"Completed: {completed} | Failed: {failed}");
    }
    lines.Add(new string('=', 92));

    if (plan.NoChangeFiles.Count > 0)
    {
        lines.Add("NO CHANGE FILES:");
        foreach (var path in plan.NoChangeFiles.OrderBy(Path.GetFileName, StringComparer.OrdinalIgnoreCase))
        {
            lines.Add("  - " + Path.GetFileName(path));
        }
        lines.Add(new string('=', 92));
    }

    foreach (var action in actions)
    {
        lines.Add($"FILE {action.Index}/{actions.Length}: {action.FileName}");
        lines.Add($"TOOL: {action.ToolName}");
        lines.Add("CHANGES:");
        foreach (var change in SplitDescription(action.Description))
        {
            lines.Add("  - " + change);
        }
        lines.Add("COMMAND:");
        lines.Add("  " + action.Command);
        lines.Add(dryRun ? "RESULT: DRY RUN | Command not executed" : "RESULT: See execution results below");
        lines.Add(new string('=', 92));
    }

    if (resultLines is { Count: > 0 })
    {
        lines.Add("EXECUTION RESULTS:");
        foreach (var line in resultLines)
        {
            lines.Add("  - " + line);
        }
        lines.Add(new string('=', 92));
    }

    var status = dryRun
        ? $"Preview ready: {actions.Length} planned, {plan.NoChangeFiles.Count} no change"
        : $"Apply complete: {completed} completed, {failed} failed";
    return new MuxPreviewResponse(actions, plan.NoChangeFiles, string.Join(Environment.NewLine, lines), status);
}

static PropEditTemplateResponse BuildPropEditTemplateResponse(MkvFileItem template)
{
    var audio = BuildPropEditTrackRows(template, "audio", "Audio").ToArray();
    var subtitles = BuildPropEditTrackRows(template, "subtitles", "Subtitle").ToArray();
    var defaultAudio = audio.FirstOrDefault(track => track.CurrentDefault)?.TrackLabel ?? "Keep existing";
    var defaultSubtitle = subtitles.FirstOrDefault(track => track.CurrentDefault)?.TrackLabel ?? "Keep existing";

    return new PropEditTemplateResponse(
        TemplatePath: template.FilePath,
        TemplateFileName: template.FileName,
        AudioTracks: audio,
        SubtitleTracks: subtitles,
        DefaultAudio: defaultAudio,
        ForcedAudio: "Keep existing",
        DefaultSubtitle: defaultSubtitle,
        ForcedSubtitle: "Keep existing");
}

static IEnumerable<PropEditTrackConfigRow> BuildPropEditTrackRows(MkvFileItem template, string type, string labelPrefix)
{
    var index = 1;
    foreach (var track in template.Tracks.Where(track => MkvTrackSelector.NormalizeTrackType(track.Type) == MkvTrackSelector.NormalizeTrackType(type)))
    {
        var label = $"{labelPrefix} {index++}";
        yield return new PropEditTrackConfigRow(
            TrackNumber: track.PropEditTrackNumber,
            TrackLabel: label,
            Type: track.Type,
            CurrentName: track.Name,
            CurrentLanguage: track.Language,
            CurrentDefault: track.Default,
            EditedName: track.Name,
            EditedLanguage: track.Language);
    }
}

static WebPropEditPlan BuildPropEditPlan(PropEditPreviewRequest request, MkvPropEditCommandBuilder builder)
{
    var selectedPaths = BuildSelectedPathSet(request.SelectedPaths);
    var files = (request.Files ?? Array.Empty<MediaFileRow>())
        .Select(row => ToMkvFileItem(row, selectedPaths.Count == 0 || selectedPaths.Contains(row.Path)))
        .ToList();
    var template = FindTemplate(files, request.TemplatePath);
    var plan = new WebPropEditPlan();

    if (template is null)
    {
        plan.Skipped.Add(new PropEditSkippedRow(string.Empty, "No template selected", "Select a scanned MKV template file first."));
        return plan;
    }

    var audioConfigs = ToPropEditConfigs(request.AudioTracks ?? BuildPropEditTemplateResponse(template).AudioTracks);
    var subtitleConfigs = ToPropEditConfigs(request.SubtitleTracks ?? BuildPropEditTemplateResponse(template).SubtitleTracks);
    var selectedFiles = files.Where(file => file.Selected).ToList();

    foreach (var file in selectedFiles)
    {
        if (!CrossPlatformRuntime.IsMkvPath(file.FilePath))
        {
            plan.Skipped.Add(new PropEditSkippedRow(file.FilePath, Path.GetFileName(file.FilePath), "Track property edits are only available for MKV files."));
            continue;
        }

        if (!HasCompatibleTrackLayout(template, file, out var layoutError))
        {
            plan.Skipped.Add(new PropEditSkippedRow(file.FilePath, Path.GetFileName(file.FilePath), layoutError));
            continue;
        }

        var build = builder.Build(new MkvPropEditCommandBuildRequest(
            file,
            audioConfigs,
            subtitleConfigs,
            request.SelectedDefaultAudio ?? "Keep existing",
            request.SelectedForcedAudio ?? "Keep existing",
            request.SelectedDefaultSubtitle ?? "Keep existing",
            request.SelectedForcedSubtitle ?? "Keep existing",
            IsMode(request.ContainerTitleMode, "file"),
            IsMode(request.ContainerTitleMode, "custom"),
            IsMode(request.ContainerTitleMode, "remove"),
            request.CustomContainerTitle ?? string.Empty,
            IsMode(request.VideoTitleMode, "file"),
            IsMode(request.VideoTitleMode, "custom"),
            IsMode(request.VideoTitleMode, "remove"),
            request.CustomVideoTitle ?? string.Empty));

        if (build.Descriptions.Count == 0)
        {
            plan.NoChange.Add(new PropEditNoChangeRow(file.FilePath, Path.GetFileName(file.FilePath), "All selected settings already match this file."));
            continue;
        }

        var action = new PlannedAction
        {
            FilePath = file.FilePath,
            Tool = "mkvpropedit",
            Description = string.Join("; ", build.Descriptions)
        };
        action.Arguments.AddRange(build.Arguments);
        plan.Actions.Add(action);
    }

    return plan;
}

static PropEditPreviewResponse BuildPropEditPreviewResponse(
    WebPropEditPlan plan,
    bool dryRun,
    int completed,
    int failed,
    IReadOnlyList<string>? resultLines = null)
{
    var actions = plan.Actions
        .Select((action, index) => new PropEditActionRow(
            Index: index + 1,
            FilePath: action.FilePath,
            FileName: Path.GetFileName(action.FilePath),
            Description: action.Description,
            Command: FormatCommand(action.Tool, action.Arguments)))
        .ToArray();

    var lines = new List<string>
    {
        $"mkvpropedit Summary - {(dryRun ? "DRY RUN" : "APPLY")}",
        $"Generated: {DateTime.Now:yyyy-MM-dd HH:mm:ss}",
        $"Planned edits: {actions.Length} | Skipped: {plan.Skipped.Count} | No Change: {plan.NoChange.Count}"
    };
    if (!dryRun)
    {
        lines.Add($"Completed: {completed} | Failed: {failed}");
    }
    lines.Add(new string('=', 92));

    if (plan.Skipped.Count > 0)
    {
        lines.Add("SKIPPED FILES:");
        foreach (var skipped in plan.Skipped)
        {
            lines.Add($"  - {skipped.FileName}");
            lines.Add($"    Reason: {skipped.Reason}");
        }
        lines.Add(new string('=', 92));
    }

    if (plan.NoChange.Count > 0)
    {
        lines.Add("NO CHANGE FILES:");
        foreach (var noChange in plan.NoChange)
        {
            lines.Add($"  - {noChange.FileName}");
            lines.Add($"    Reason: {noChange.Reason}");
        }
        lines.Add(new string('=', 92));
    }

    foreach (var action in actions)
    {
        lines.Add($"FILE {action.Index}/{actions.Length}: {action.FileName}");
        lines.Add("CHANGES:");
        foreach (var change in SplitDescription(action.Description))
        {
            lines.Add("  - " + change);
        }
        lines.Add("COMMAND:");
        lines.Add("  " + action.Command);
        lines.Add(dryRun ? "RESULT: DRY RUN | Command not executed" : "RESULT: See execution results below");
        lines.Add(new string('=', 92));
    }

    if (resultLines is { Count: > 0 })
    {
        lines.Add("EXECUTION RESULTS:");
        foreach (var line in resultLines)
        {
            lines.Add("  - " + line);
        }
        lines.Add(new string('=', 92));
    }

    var status = dryRun
        ? $"Preview ready: {actions.Length} planned, {plan.Skipped.Count} skipped, {plan.NoChange.Count} no change"
        : $"Apply complete: {completed} completed, {failed} failed";
    return new PropEditPreviewResponse(actions, plan.Skipped, plan.NoChange, string.Join(Environment.NewLine, lines), status);
}

static MkvFileItem? FindTemplate(IReadOnlyList<MkvFileItem> files, string? templatePath)
{
    if (!string.IsNullOrWhiteSpace(templatePath))
    {
        var template = files.FirstOrDefault(file => CrossPlatformRuntime.PathComparer.Equals(file.FilePath, CrossPlatformRuntime.NormalizeUserPath(templatePath)));
        if (template is not null) return template;
    }

    return files.FirstOrDefault(file => CrossPlatformRuntime.IsMkvPath(file.FilePath));
}

static IReadOnlyList<PropEditTrackConfig> ToPropEditConfigs(IEnumerable<PropEditTrackConfigRow> rows)
{
    return rows.Select(row => new PropEditTrackConfig
    {
        TrackNumber = row.TrackNumber,
        TrackLabel = row.TrackLabel,
        Type = row.Type,
        CurrentName = row.CurrentName,
        CurrentLanguage = row.CurrentLanguage,
        CurrentDefault = row.CurrentDefault,
        EditedName = row.EditedName,
        EditedLanguage = row.EditedLanguage
    }).ToArray();
}

static bool HasCompatibleTrackLayout(MkvFileItem template, MkvFileItem file, out string error)
{
    error = string.Empty;
    foreach (var type in new[] { "video", "audio", "subtitles" })
    {
        var normalizedType = MkvTrackSelector.NormalizeTrackType(type);
        var templateCount = template.Tracks.Count(track => MkvTrackSelector.NormalizeTrackType(track.Type) == normalizedType);
        var fileCount = file.Tracks.Count(track => MkvTrackSelector.NormalizeTrackType(track.Type) == normalizedType);
        if (templateCount == fileCount) continue;

        error = $"track layout mismatch for {type}: template has {templateCount}, file has {fileCount}";
        return false;
    }

    return true;
}

static bool IsMode(string? value, string mode)
    => string.Equals(value?.Trim(), mode, StringComparison.OrdinalIgnoreCase);

static HashSet<string> BuildSelectedPathSet(IReadOnlyList<string>? selectedPaths)
{
    return (selectedPaths ?? Array.Empty<string>())
        .Where(path => !string.IsNullOrWhiteSpace(path))
        .Select(CrossPlatformRuntime.NormalizeUserPath)
        .ToHashSet(CrossPlatformRuntime.PathComparer);
}

static IEnumerable<string> SplitDescription(string description)
{
    return description
        .Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
        .Select(item => string.IsNullOrWhiteSpace(item) ? "No description" : item);
}

static string FormatCommand(string toolPath, IEnumerable<string> arguments)
{
    return QuoteArgument(toolPath) + " " + string.Join(" ", arguments.Select(QuoteArgument));
}

static string QuoteArgument(string value)
{
    if (string.IsNullOrEmpty(value)) return "\"\"";
    return value.Any(char.IsWhiteSpace) || value.Contains('"')
        ? "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\""
        : value;
}

static LibraryAuditResponse BuildLibraryAuditResponse(IReadOnlyList<MediaFileRow> rows)
{
    var groups = rows
        .GroupBy(row => Path.GetDirectoryName(row.Path) ?? string.Empty, StringComparer.OrdinalIgnoreCase)
        .OrderBy(group => group.Key, StringComparer.OrdinalIgnoreCase)
        .ToArray();
    var items = new List<LibraryAuditRow>();

    foreach (var group in groups)
    {
        var files = group.OrderBy(row => row.FileName, StringComparer.OrdinalIgnoreCase).ToArray();
        var standardVideo = Dominant(files.Select(row => string.Join(" ", new[] { row.Resolution, row.Codec, row.BitDepth, row.Hdr }.Where(value => !string.IsNullOrWhiteSpace(value)))));
        var standardAudio = Dominant(files.Select(row => CleanAuditValue(row.AudioSummary, "none")));
        var standardSubtitles = Dominant(files.Select(row => CleanAuditValue(row.SubtitleSummary, "none")));
        var issues = new List<string>();
        var issuePaths = new List<string>();

        foreach (var file in files)
        {
            AddAuditMismatch(file, "video", standardVideo, string.Join(" ", new[] { file.Resolution, file.Codec, file.BitDepth, file.Hdr }.Where(value => !string.IsNullOrWhiteSpace(value))), issues, issuePaths);
            AddAuditMismatch(file, "audio", standardAudio, file.AudioSummary, issues, issuePaths);
            AddAuditMismatch(file, "subtitles", standardSubtitles, file.SubtitleSummary, issues, issuePaths);
        }

        var template = files.FirstOrDefault(file =>
            string.Equals(CleanAuditValue(string.Join(" ", new[] { file.Resolution, file.Codec, file.BitDepth, file.Hdr }.Where(value => !string.IsNullOrWhiteSpace(value))), "unknown"), standardVideo, StringComparison.OrdinalIgnoreCase) &&
            string.Equals(CleanAuditValue(file.AudioSummary, "none"), standardAudio, StringComparison.OrdinalIgnoreCase) &&
            string.Equals(CleanAuditValue(file.SubtitleSummary, "none"), standardSubtitles, StringComparison.OrdinalIgnoreCase)) ?? files[0];

        items.Add(new LibraryAuditRow(
            FolderPath: group.Key,
            FolderName: string.IsNullOrWhiteSpace(group.Key) ? "root" : Path.GetFileName(group.Key),
            FileCount: files.Length,
            StandardVideo: standardVideo,
            StandardAudio: standardAudio,
            StandardSubtitles: standardSubtitles,
            TemplateFilePath: template.Path,
            TemplateFileName: template.FileName,
            HasIssues: issues.Count > 0,
            IssueSummary: issues.Count == 0 ? "no issues found" : string.Join(" | ", issues.Take(3)),
            Issues: issues,
            IssueFilePaths: issuePaths.Distinct(CrossPlatformRuntime.PathComparer).ToArray(),
            AllFilePaths: files.Select(file => file.Path).Distinct(CrossPlatformRuntime.PathComparer).ToArray()));
    }

    var summary = new LibraryAuditSummary(
        Groups: items.Count,
        Files: rows.Count,
        IssueGroups: items.Count(item => item.HasIssues),
        StandardGroups: items.Count(item => !item.HasIssues));
    return new LibraryAuditResponse(summary, items);
}

static void AddAuditMismatch(MediaFileRow file, string label, string standard, string value, List<string> issues, List<string> issuePaths)
{
    var cleanValue = CleanAuditValue(value, label == "video" ? "unknown" : "none");
    if (string.IsNullOrWhiteSpace(standard) || standard == "unknown") return;
    if (string.Equals(cleanValue, standard, StringComparison.OrdinalIgnoreCase)) return;

    issues.Add($"{file.FileName}: {label} mismatch ({cleanValue} vs {standard})");
    issuePaths.Add(file.Path);
}

static string Dominant(IEnumerable<string> values)
{
    return values.Select(value => CleanAuditValue(value, "unknown"))
        .GroupBy(value => value, StringComparer.OrdinalIgnoreCase)
        .OrderByDescending(group => group.Count())
        .ThenBy(group => group.Key, StringComparer.OrdinalIgnoreCase)
        .Select(group => group.Key)
        .FirstOrDefault() ?? "unknown";
}

static List<string> NormalizeStringList(IReadOnlyList<string> values, IReadOnlyList<string> fallback)
{
    var normalized = values
        .Select(value => value?.Trim() ?? string.Empty)
        .Where(value => !string.IsNullOrWhiteSpace(value))
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .ToList();

    return normalized.Count > 0 ? normalized : fallback.ToList();
}

static string CleanAuditValue(string? value, string fallback)
{
    var clean = (value ?? string.Empty).Trim();
    return string.IsNullOrWhiteSpace(clean) ? fallback : clean;
}

static bool IsVisibleBrowseEntry(FileSystemInfo info)
{
    if (info.Name.StartsWith(".", StringComparison.Ordinal)) return false;
    if (info is DirectoryInfo) return true;
    return CrossPlatformRuntime.IsSupportedMediaPath(info.FullName);
}

static async Task RunScanJobAsync(
    ScanJobState job,
    ScanRequest request,
    MkvScannerService scanner,
    string mediaRoot,
    CurrentScanStore currentScan,
    CancellationToken token)
{
    job.MarkRunning();

    try
    {
        var sources = NormalizeSources(request, mediaRoot);
        var ignored = NormalizeIgnoredFolders(request.IgnoredFolderNames);
        var mkvMergePath = string.IsNullOrWhiteSpace(request.MkvMergePath) ? "mkvmerge" : request.MkvMergePath.Trim();
        var ffProbePath = string.IsNullOrWhiteSpace(request.FfProbePath) ? "ffprobe" : request.FfProbePath.Trim();
        var rows = new List<MediaFileRow>();
        var skipped = new List<string>();

        foreach (var source in sources)
        {
            token.ThrowIfCancellationRequested();
            var normalized = CrossPlatformRuntime.NormalizeUserPath(source);
            job.SetCurrentSource(normalized);

            if (File.Exists(normalized))
            {
                job.SetProgress(0, 1);

                if (!CrossPlatformRuntime.IsSupportedMediaPath(normalized))
                {
                    skipped.Add($"{normalized} is not an MKV or MP4 file.");
                    job.SetProgress(1, 1);
                    continue;
                }

                var file = await scanner.ScanFileSafeAsync(normalized, mkvMergePath, ffProbePath, token);
                var row = MediaFileRow.From(file);
                rows.Add(row);
                job.AddFile(row);
                job.SetProgress(1, 1);
                continue;
            }

            if (!Directory.Exists(normalized))
            {
                skipped.Add($"{normalized} was not found.");
                continue;
            }

            var progress = new Progress<(int Completed, int Total)>(p => job.SetProgress(p.Completed, p.Total));
            await foreach (var file in scanner.ScanAsync(normalized, mkvMergePath, ffProbePath, token, ignored, progress, ResolveWebWorkerSettings()))
            {
                var row = MediaFileRow.From(file);
                rows.Add(row);
                job.AddFile(row);
            }
        }

        var orderedRows = rows
            .OrderBy(row => row.Path, StringComparer.OrdinalIgnoreCase)
            .ToArray();

        job.MarkCompleted(
            orderedRows,
            skipped,
            new ScanSummary(
                Total: orderedRows.Length,
                Mkv: orderedRows.Count(row => row.Extension.Equals(".mkv", StringComparison.OrdinalIgnoreCase)),
                Mp4: orderedRows.Count(row => row.Extension.Equals(".mp4", StringComparison.OrdinalIgnoreCase)),
                Failed: orderedRows.Count(row => row.Status.Contains("failed", StringComparison.OrdinalIgnoreCase))));
        currentScan.Set(orderedRows, new ScanSummary(
            Total: orderedRows.Length,
            Mkv: orderedRows.Count(row => row.Extension.Equals(".mkv", StringComparison.OrdinalIgnoreCase)),
            Mp4: orderedRows.Count(row => row.Extension.Equals(".mp4", StringComparison.OrdinalIgnoreCase)),
            Failed: orderedRows.Count(row => row.Status.Contains("failed", StringComparison.OrdinalIgnoreCase))));
    }
    catch (OperationCanceledException)
    {
        job.MarkCanceled();
    }
    catch (Exception ex)
    {
        job.MarkFailed(ex.Message);
    }
}

static WorkerSettings ResolveWebWorkerSettings()
{
    var configured = Environment.GetEnvironmentVariable("MKVO_SCAN_WORKERS");
    var workers = int.TryParse(configured, out var value) ? value : 6;
    return new WorkerSettings { MaxScanWorkers = workers }.Normalize();
}

static int ResolveWebEditWorkers()
{
    var configured = Environment.GetEnvironmentVariable("MKVO_EDIT_WORKERS");
    var workers = int.TryParse(configured, out var value) ? value : 2;
    return new WorkerSettings { MaxEditWorkers = workers }.Normalize().MaxEditWorkers;
}

static async Task RunMuxJobAsync(
    OperationJobState job,
    MkvMergeRemuxPlan plan,
    MkvMergeService muxService,
    MkvScannerService scanner,
    CurrentScanStore currentScan,
    OperationLogStore logs,
    FileConflictService fileConflicts)
{
    job.MarkRunning();
    var resultLines = new List<string>();

    try
    {
        foreach (var action in plan.Actions)
        {
            job.Token.ThrowIfCancellationRequested();
            var fileName = Path.GetFileName(action.SourceFilePath);
            job.SetCurrentFile(fileName);

            // Pre-flight conflict check so locked or missing files skip cleanly
            // instead of surfacing as raw tool failures.
            var conflict = fileConflicts.CheckReadableWritable(action.SourceFilePath, requireWrite: true);
            if (!conflict.CanProceed)
            {
                job.RecordSkipped();
                var line = $"SKIPPED: {fileName} - {conflict.Reason}";
                resultLines.Add(line);
                job.AddLine(line);
                continue;
            }

            var result = await muxService.ExecuteRemuxAsync(
                ResolveToolCommand("mkvmerge"),
                action,
                percent => job.SetCurrentFilePercent(percent),
                job.Token);

            if (result.ExitCode == 0)
            {
                job.RecordCompleted();
                var line = $"SUCCESS: {fileName} - {action.Description}";
                resultLines.Add(line);
                job.AddLine(line);
                if (File.Exists(action.SourceFilePath))
                {
                    try
                    {
                        var refreshed = await scanner.ScanFileSafeAsync(action.SourceFilePath, ResolveToolCommand("mkvmerge"), ResolveToolCommand("ffprobe"), job.Token);
                        currentScan.Upsert(MediaFileRow.From(refreshed));
                    }
                    catch (Exception ex)
                    {
                        var warning = $"REFRESH WARNING: {fileName} - {ex.Message}";
                        resultLines.Add(warning);
                        job.AddLine(warning);
                    }
                }

                // Conversions create a new .mkv beside the source; scan it so it shows up immediately.
                if (string.Equals(action.Operation, "convert-mkv", StringComparison.OrdinalIgnoreCase) && File.Exists(action.FinalOutputPath))
                {
                    try
                    {
                        var converted = await scanner.ScanFileSafeAsync(action.FinalOutputPath, ResolveToolCommand("mkvmerge"), ResolveToolCommand("ffprobe"), job.Token);
                        currentScan.Upsert(MediaFileRow.From(converted));
                    }
                    catch (Exception ex)
                    {
                        var warning = $"REFRESH WARNING: {Path.GetFileName(action.FinalOutputPath)} - {ex.Message}";
                        resultLines.Add(warning);
                        job.AddLine(warning);
                    }
                }
            }
            else
            {
                job.RecordFailed();
                var error = string.IsNullOrWhiteSpace(result.StandardError) ? $"exit code {result.ExitCode}" : result.StandardError.Trim();
                var line = $"FAILED: {fileName} - {error}";
                resultLines.Add(line);
                job.AddLine(line);
            }
        }

        var response = BuildMuxPreviewResponse(plan, dryRun: false, job.Completed, job.Failed, resultLines);
        logs.Add("Mux / Remux", $"Mux/remux complete: {job.Completed} completed, {job.Failed} failed, {job.Skipped} skipped", response.Summary);
        job.MarkCompleted(response, null);
    }
    catch (OperationCanceledException)
    {
        resultLines.Add("CANCELED: remaining actions were not executed.");
        var response = BuildMuxPreviewResponse(plan, dryRun: false, job.Completed, job.Failed, resultLines);
        logs.Add("Mux / Remux", $"Mux/remux canceled: {job.Completed} completed, {job.Failed} failed", response.Summary);
        job.MarkCanceled(response, null);
    }
    catch (Exception ex)
    {
        logs.Add("Mux / Remux", "Mux/remux failed", ex.Message);
        job.MarkFailed(ex.Message);
    }
}

static async Task RunPropEditJobAsync(
    OperationJobState job,
    WebPropEditPlan plan,
    MkvPropEditService propEdit,
    MkvScannerService scanner,
    CurrentScanStore currentScan,
    OperationLogStore logs,
    FileConflictService fileConflicts)
{
    job.MarkRunning();
    var resultLines = new List<string>();
    var linesGate = new object();

    void AddResultLine(string line)
    {
        lock (linesGate)
        {
            resultLines.Add(line);
        }

        job.AddLine(line);
    }

    try
    {
        var editWorkers = ResolveWebEditWorkers();
        var processed = 0;

        await Parallel.ForEachAsync(
            plan.Actions,
            new ParallelOptions
            {
                CancellationToken = job.Token,
                MaxDegreeOfParallelism = editWorkers
            },
            async (action, token) =>
            {
                var fileName = Path.GetFileName(action.FilePath);

                var conflict = fileConflicts.CheckReadableWritable(action.FilePath, requireWrite: true);
                if (!conflict.CanProceed)
                {
                    job.RecordSkipped();
                    AddResultLine($"SKIPPED: {fileName} - {conflict.Reason}");
                    return;
                }

                var result = await propEdit.ExecuteAsync(ResolveToolCommand("mkvpropedit"), action, token);
                var done = Interlocked.Increment(ref processed);
                job.SetCurrentFile($"{done}/{plan.Actions.Count}");

                if (result.ExitCode == 0)
                {
                    job.RecordCompleted();
                    AddResultLine($"SUCCESS: {fileName} - {action.Description}");
                    try
                    {
                        var refreshed = await scanner.ScanFileSafeAsync(action.FilePath, ResolveToolCommand("mkvmerge"), ResolveToolCommand("ffprobe"), token);
                        currentScan.Upsert(MediaFileRow.From(refreshed));
                    }
                    catch (Exception ex)
                    {
                        AddResultLine($"REFRESH WARNING: {fileName} - {ex.Message}");
                    }
                }
                else
                {
                    job.RecordFailed();
                    var error = string.IsNullOrWhiteSpace(result.StandardError) ? $"exit code {result.ExitCode}" : result.StandardError.Trim();
                    AddResultLine($"FAILED: {fileName} - {error}");
                }
            });

        var response = BuildPropEditPreviewResponse(plan, dryRun: false, job.Completed, job.Failed, resultLines);
        logs.Add("Track Properties", $"Property edit complete: {job.Completed} completed, {job.Failed} failed, {job.Skipped} skipped", response.Summary);
        job.MarkCompleted(null, response);
    }
    catch (OperationCanceledException)
    {
        AddResultLine("CANCELED: remaining edits were not executed.");
        var response = BuildPropEditPreviewResponse(plan, dryRun: false, job.Completed, job.Failed, resultLines);
        logs.Add("Track Properties", $"Property edit canceled: {job.Completed} completed, {job.Failed} failed", response.Summary);
        job.MarkCanceled(null, response);
    }
    catch (Exception ex)
    {
        logs.Add("Track Properties", "Property edit failed", ex.Message);
        job.MarkFailed(ex.Message);
    }
}

static bool IsBasicAuthAuthorized(string authorizationHeader, string expectedUsername, string expectedPassword)
{
    const string scheme = "Basic ";
    if (string.IsNullOrWhiteSpace(authorizationHeader) || !authorizationHeader.StartsWith(scheme, StringComparison.OrdinalIgnoreCase))
    {
        return false;
    }

    string decoded;
    try
    {
        decoded = Encoding.UTF8.GetString(Convert.FromBase64String(authorizationHeader[scheme.Length..].Trim()));
    }
    catch (FormatException)
    {
        return false;
    }

    var separator = decoded.IndexOf(':');
    if (separator < 0) return false;

    var user = decoded[..separator];
    var password = decoded[(separator + 1)..];

    // Constant-time comparison so credential checking does not leak length/prefix timing.
    var userMatch = CryptographicOperations.FixedTimeEquals(Encoding.UTF8.GetBytes(user), Encoding.UTF8.GetBytes(expectedUsername));
    var passwordMatch = CryptographicOperations.FixedTimeEquals(Encoding.UTF8.GetBytes(password), Encoding.UTF8.GetBytes(expectedPassword));
    return userMatch && passwordMatch;
}

static IReadOnlyList<string> BuildAllowedBrowseRoots(string mediaRoot, IReadOnlyList<SourceRoot> sourceRoots, AppSettings settings)
{
    var roots = new List<string> { mediaRoot };
    roots.AddRange(sourceRoots.Select(root => root.Path));
    roots.AddRange(settings.WatchFolders);
    roots.AddRange(settings.MediaServers
        .SelectMany(server => server.Libraries)
        .Where(library => library.IsEnabled)
        .Select(library => library.ContainerPath));

    return roots
        .Select(CrossPlatformRuntime.NormalizeUserPath)
        .Where(root => !string.IsNullOrWhiteSpace(root))
        .Distinct(CrossPlatformRuntime.PathComparer)
        .ToArray();
}

static bool IsPathUnderAllowedRoots(string path, IReadOnlyList<string> allowedRoots)
{
    if (string.IsNullOrWhiteSpace(path)) return false;

    string fullPath;
    try
    {
        fullPath = Path.GetFullPath(CrossPlatformRuntime.NormalizeUserPath(path)).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
    }
    catch
    {
        return false;
    }

    var comparison = CrossPlatformRuntime.IsWindows || CrossPlatformRuntime.IsMacOS
        ? StringComparison.OrdinalIgnoreCase
        : StringComparison.Ordinal;

    foreach (var root in allowedRoots)
    {
        string fullRoot;
        try
        {
            fullRoot = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        }
        catch
        {
            continue;
        }

        if (fullPath.Equals(fullRoot, comparison)) return true;
        if (fullPath.StartsWith(fullRoot + Path.DirectorySeparatorChar, comparison)) return true;
        if (fullPath.StartsWith(fullRoot + Path.AltDirectorySeparatorChar, comparison)) return true;
    }

    return false;
}

static AppSettings BuildSettingsSnapshot(
    AppSettingsService settingsService,
    string? provider = null,
    string? language = null,
    string? renameTemplate = null)
{
    var settings = settingsService.Load();
    settings.TvdbApiKey = FirstNonBlank(Environment.GetEnvironmentVariable("TVDB_API_KEY"), Environment.GetEnvironmentVariable("MKVO_TVDB_API_KEY"), settings.TvdbApiKey);
    settings.TvdbPin = FirstNonBlank(Environment.GetEnvironmentVariable("TVDB_PIN"), Environment.GetEnvironmentVariable("MKVO_TVDB_PIN"), settings.TvdbPin);
    settings.TmdbApiKey = FirstNonBlank(Environment.GetEnvironmentVariable("TMDB_API_KEY"), Environment.GetEnvironmentVariable("MKVO_TMDB_API_KEY"), settings.TmdbApiKey);
    settings.TvdbLanguage = string.IsNullOrWhiteSpace(language) ? FirstNonBlank(settings.TvdbLanguage, "eng") : language.Trim();
    settings.RenameLookupProvider = NormalizeLookupProvider(string.IsNullOrWhiteSpace(provider) ? settings.RenameLookupProvider : provider);
    if (!string.IsNullOrWhiteSpace(renameTemplate))
    {
        settings.RenameTemplate = renameTemplate.Trim();
    }

    return settings;
}

static IRenameMetadataProvider GetRenameMetadataProvider(string? provider)
    => NormalizeLookupProvider(provider) == "TMDB"
        ? new TmdbRenameMetadataProvider()
        : new TvdbRenameMetadataProvider();

static async Task<IReadOnlyList<TvdbEpisode>> LoadRenameEpisodesAsync(
    TvdbSeriesSearchResult selectedResult,
    AppSettings settings,
    RenameEpisodeCache episodeCache,
    CancellationToken token)
{
    return await episodeCache.GetOrLoadAsync(settings.RenameLookupProvider, selectedResult, settings.TvdbLanguage, async () =>
    {
        var provider = GetRenameMetadataProvider(settings.RenameLookupProvider);
        var episodes = await provider.GetEpisodesAsync(selectedResult, settings.TvdbLanguage, settings, token);
        return OrderRenameEpisodes(episodes).ToArray();
    });
}

static IReadOnlyList<RenameScopeRow> BuildRenameScopeOptions(IReadOnlyList<TvdbEpisode> episodes, TvdbSeriesSearchResult selectedResult)
{
    if (selectedResult.Format.Equals("Movie", StringComparison.OrdinalIgnoreCase))
    {
        return new[]
        {
            new RenameScopeRow("Movie", "N/A", true)
        };
    }

    var scopes = new List<RenameScopeRow>();
    var regularCount = episodes.Count(episode => episode.SeasonNumber > 0);
    var specialCount = episodes.Count(episode => episode.SeasonNumber == 0);
    if (regularCount > 0)
    {
        scopes.Add(new RenameScopeRow("AllRegular", $"All seasons ({regularCount})", true));
    }

    scopes.Add(new RenameScopeRow("All", $"All seasons + specials ({episodes.Count})", regularCount == 0));

    foreach (var season in episodes.Where(episode => episode.SeasonNumber > 0).Select(episode => episode.SeasonNumber).Distinct().OrderBy(value => value))
    {
        var count = episodes.Count(episode => episode.SeasonNumber == season);
        scopes.Add(new RenameScopeRow($"Season:{season}", $"Season {season} ({count})", false));
    }

    if (specialCount > 0)
    {
        scopes.Add(new RenameScopeRow("Specials", $"Specials ({specialCount})", false));
    }

    return scopes;
}

static IReadOnlyList<string> NormalizeRenameScopeKeys(
    IReadOnlyList<string>? scopeKeys,
    string? legacyScopeKey,
    IReadOnlyList<RenameScopeRow> scopeOptions)
{
    var keys = (scopeKeys ?? Array.Empty<string>())
        .Concat(string.IsNullOrWhiteSpace(legacyScopeKey) ? Array.Empty<string>() : new[] { legacyScopeKey })
        .Select(key => key?.Trim() ?? string.Empty)
        .Where(key => !string.IsNullOrWhiteSpace(key))
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .ToArray();

    if (keys.Length > 0) return keys;

    var defaultKey = scopeOptions.FirstOrDefault(option => option.IsSelected)?.Key ?? "AllRegular";
    return new[] { defaultKey };
}

static IReadOnlyList<TvdbEpisode> FilterRenameEpisodes(
    IReadOnlyList<TvdbEpisode> episodes,
    IReadOnlyList<string> scopeKeys,
    TvdbSeriesSearchResult selectedResult)
{
    if (selectedResult.Format.Equals("Movie", StringComparison.OrdinalIgnoreCase))
    {
        return episodes.Take(1).ToArray();
    }

    if (scopeKeys.Any(key => key.Equals("All", StringComparison.OrdinalIgnoreCase)))
    {
        return OrderRenameEpisodes(episodes).ToArray();
    }

    // Multiple scopes act as a union, mirroring the desktop checkbox behavior
    // (e.g. Season 2 + Season 3, or all regular seasons plus specials).
    var selected = new HashSet<TvdbEpisode>();
    foreach (var key in scopeKeys)
    {
        IEnumerable<TvdbEpisode> matched = key switch
        {
            var value when value.Equals("AllRegular", StringComparison.OrdinalIgnoreCase)
                => episodes.Where(episode => episode.SeasonNumber > 0),
            var value when value.Equals("Specials", StringComparison.OrdinalIgnoreCase)
                => episodes.Where(episode => episode.SeasonNumber == 0),
            var value when value.StartsWith("Season:", StringComparison.OrdinalIgnoreCase)
                && int.TryParse(value["Season:".Length..], out var season)
                => episodes.Where(episode => episode.SeasonNumber == season),
            _ => episodes.Where(episode => episode.SeasonNumber > 0)
        };

        foreach (var episode in matched)
        {
            selected.Add(episode);
        }
    }

    return OrderRenameEpisodes(selected).ToArray();
}

static IReadOnlyList<RenameSourceFile> BuildRenameSourceFiles(IReadOnlyList<MediaFileRow> rows)
{
    return rows
        .Where(row => !string.IsNullOrWhiteSpace(row.Path))
        .Select(row =>
        {
            var detected = DetectRenameInfo(row.Path);
            return new RenameSourceFile(
                Path: row.Path,
                CurrentFileName: string.IsNullOrWhiteSpace(row.FileName) ? System.IO.Path.GetFileName(row.Path) : row.FileName,
                SeriesTitle: detected.SeriesTitle,
                Season: detected.Season,
                Episode: detected.Episode,
                AbsoluteEpisode: detected.AbsoluteEpisode,
                SortKey: GetRenameSortKey(row.Path));
        })
        .OrderBy(file => file.SortKey.Season)
        .ThenBy(file => file.SortKey.Episode)
        .ThenBy(file => file.SortKey.Part)
        .ThenBy(file => file.SortKey.RelativeName, NaturalStringComparer.Instance)
        .ToArray();
}

static RenamePreviewBuildResult BuildRenamePreviewRows(
    IReadOnlyList<RenameSourceFile> sourceFiles,
    IReadOnlyList<TvdbEpisode> providerEpisodes,
    TvdbSeriesSearchResult selectedResult,
    string template)
{
    var isMovie = selectedResult.Format.Equals("Movie", StringComparison.OrdinalIgnoreCase);
    var episodeMap = providerEpisodes
        .GroupBy(episode => (episode.SeasonNumber, episode.EpisodeNumber))
        .ToDictionary(group => group.Key, group => group.First());
    var orderedMatches = isMovie
        ? Array.Empty<OrderedEpisodeMatch>()
        : RenameEpisodeMatcher.MatchByListOrder(providerEpisodes, sourceFiles.Count).ToArray();
    var usedEpisodeIds = new HashSet<int>();
    var selectedYear = int.TryParse(selectedResult.Year, out var parsedYear) ? parsedYear : (int?)null;
    var rows = new List<RenamePreviewRow>();

    var exact = 0;
    var absolute = 0;
    var listOrder = 0;
    var sequential = 0;
    var unmatched = 0;

    for (var index = 0; index < sourceFiles.Count; index++)
    {
        var source = sourceFiles[index];
        TvdbEpisode? episode = null;
        AbsoluteEpisodeMatch? absoluteMatch = null;
        OrderedEpisodeMatch? orderedMatch = null;
        TvdbEpisode? exactEpisode = null;
        var exactMatch = false;
        var detectedSeason = source.Season;
        var detectedEpisode = source.Episode;

        if (!isMovie
            && source.Season.HasValue
            && source.Episode.HasValue
            && episodeMap.TryGetValue((source.Season.Value, source.Episode.Value), out var mappedEpisode))
        {
            exactEpisode = mappedEpisode;
        }

        if (isMovie)
        {
            episode = providerEpisodes.FirstOrDefault();
            exactMatch = episode is not null;
        }
        else if (orderedMatches.Length == sourceFiles.Count)
        {
            orderedMatch = orderedMatches[index];
            episode = orderedMatch.Episode;
            detectedSeason = episode.SeasonNumber;
            detectedEpisode = episode.EpisodeNumber;
            exactMatch = exactEpisode?.Id == episode.Id;
        }
        else if (exactEpisode is not null)
        {
            episode = exactEpisode;
            exactMatch = true;
        }
        else if (RenameEpisodeMatcher.TryMatchAbsoluteEpisode(providerEpisodes, source.AbsoluteEpisode, out var mappedAbsoluteEpisode))
        {
            absoluteMatch = mappedAbsoluteEpisode;
            episode = mappedAbsoluteEpisode.Episode;
            detectedSeason = episode.SeasonNumber;
            detectedEpisode = episode.EpisodeNumber;
        }
        else
        {
            episode = providerEpisodes.FirstOrDefault(candidate => !usedEpisodeIds.Contains(candidate.Id));
        }

        if (episode is null)
        {
            rows.Add(new RenamePreviewRow(
                Selected: false,
                SourcePath: source.Path,
                CurrentFileName: source.CurrentFileName,
                Detected: isMovie ? "Movie" : FormatDetectedEpisode(source.Season, source.Episode),
                EpisodeName: string.Empty,
                NewFileName: string.Empty,
                Confidence: "Low",
                Status: "No provider episode available",
                CanApply: false));
            unmatched++;
            continue;
        }

        usedEpisodeIds.Add(episode.Id);
        var newFileName = BuildRenameFileName(source.Path, selectedResult.Name, selectedYear, episode, template, isMovie);
        var status = "Sequential fallback - verify";
        var confidence = "Low";

        if (isMovie)
        {
            status = "Movie match";
            confidence = "High";
            exact++;
        }
        else if (exactMatch)
        {
            status = "Exact S/E match";
            confidence = "High";
            exact++;
        }
        else if (orderedMatch is not null)
        {
            status = orderedMatch.StatusText;
            confidence = "High";
            listOrder++;
        }
        else if (absoluteMatch is not null)
        {
            status = absoluteMatch.StatusText;
            confidence = "High";
            absolute++;
        }
        else
        {
            sequential++;
        }

        rows.Add(new RenamePreviewRow(
            Selected: !string.Equals(source.CurrentFileName, newFileName, StringComparison.OrdinalIgnoreCase),
            SourcePath: source.Path,
            CurrentFileName: source.CurrentFileName,
            Detected: isMovie ? "Movie" : FormatDetectedEpisode(detectedSeason, detectedEpisode),
            EpisodeName: episode.Name,
            NewFileName: newFileName,
            Confidence: confidence,
            Status: status,
            CanApply: !string.IsNullOrWhiteSpace(newFileName)));
    }

    var changed = rows.Count(row => row.CanApply && !string.Equals(row.CurrentFileName, row.NewFileName, StringComparison.OrdinalIgnoreCase));
    var skipped = rows.Count - changed;
    var summary = BuildRenameSummary(rows, changed, skipped, dryRun: true);
    var statusText = isMovie
        ? $"Preview ready: {exact} movie match, {unmatched} unmatched"
        : $"Preview ready: {exact} exact, {listOrder} list order, {absolute} absolute, {sequential} sequential fallback, {unmatched} unmatched";

    return new RenamePreviewBuildResult(rows, summary, statusText);
}

static IReadOnlyList<RenamePreviewRow> ApplyRenameRows(IReadOnlyList<RenamePreviewRow> rows)
{
    var selected = rows.Where(row => row.Selected).ToList();
    var targetCounts = selected
        .Where(row => !string.IsNullOrWhiteSpace(row.NewFileName))
        .Select(row => Path.Combine(Path.GetDirectoryName(row.SourcePath) ?? string.Empty, row.NewFileName))
        .GroupBy(path => path, StringComparer.OrdinalIgnoreCase)
        .ToDictionary(group => group.Key, group => group.Count(), StringComparer.OrdinalIgnoreCase);

    var applied = new List<RenamePreviewRow>(rows.Count);
    foreach (var row in rows)
    {
        if (!row.Selected)
        {
            applied.Add(row with { Status = "Skipped - not selected", CanApply = false });
            continue;
        }

        if (string.IsNullOrWhiteSpace(row.NewFileName))
        {
            applied.Add(row with { Status = "Skipped - no preview filename", CanApply = false });
            continue;
        }

        var targetPath = Path.Combine(Path.GetDirectoryName(row.SourcePath) ?? string.Empty, row.NewFileName);
        if (targetCounts.TryGetValue(targetPath, out var duplicateCount) && duplicateCount > 1)
        {
            applied.Add(row with { Status = "Skipped - duplicate target in plan", CanApply = false });
            continue;
        }

        if (!File.Exists(row.SourcePath))
        {
            applied.Add(row with { Status = "Skipped - source missing", CanApply = false });
            continue;
        }

        if (string.Equals(row.SourcePath, targetPath, StringComparison.OrdinalIgnoreCase))
        {
            applied.Add(row with { Status = "No change", CanApply = false });
            continue;
        }

        if (File.Exists(targetPath))
        {
            applied.Add(row with { Status = "Skipped - target exists", CanApply = false });
            continue;
        }

        try
        {
            File.Move(row.SourcePath, targetPath);
            applied.Add(row with
            {
                SourcePath = targetPath,
                CurrentFileName = Path.GetFileName(targetPath),
                Status = "Renamed",
                CanApply = false,
                Selected = false
            });
        }
        catch (Exception ex)
        {
            applied.Add(row with { Status = "Failed - " + ex.Message, CanApply = false });
        }
    }

    return applied;
}

static RenameBatchRecord? FindRenameBatch(RenameBatchHistoryService renameBatches, string id)
{
    return renameBatches.Load()
        .FirstOrDefault(batch => string.Equals(batch.Id, id, StringComparison.OrdinalIgnoreCase));
}

static IReadOnlyList<RenameBatchRestoreMove> BuildRestoredMoves(RenameBatchRecord batch)
{
    return batch.Entries
        .Where(entry => File.Exists(entry.OriginalPath) && !File.Exists(entry.RenamedPath))
        .Select(entry => new RenameBatchRestoreMove(
            OriginalPath: entry.OriginalPath,
            RenamedPath: entry.RenamedPath,
            OriginalFileName: entry.OriginalFileName))
        .ToArray();
}

static string BuildRenameFileName(
    string sourcePath,
    string title,
    int? year,
    TvdbEpisode episode,
    string template,
    bool isMovie)
{
    return RenameFileNameBuilder.Build(sourcePath, title, year, episode, template, isMovie);
}

static string BuildRenameSummary(IReadOnlyList<RenamePreviewRow> rows, int filesChanged, int filesSkipped, bool dryRun)
{
    var lines = new List<string>
    {
        $"mkvrename Summary - {(dryRun ? "DRY RUN" : "APPLY")}",
        $"Generated: {DateTime.Now:yyyy-MM-dd HH:mm:ss}",
        $"Files changed: {filesChanged} | Files skipped: {filesSkipped}",
        new string('=', 92)
    };

    var changedRows = rows
        .Where(row => !string.IsNullOrWhiteSpace(row.NewFileName)
                      && !string.Equals(row.CurrentFileName, row.NewFileName, StringComparison.OrdinalIgnoreCase))
        .OrderBy(row => row.CurrentFileName, StringComparer.OrdinalIgnoreCase)
        .ToArray();

    if (changedRows.Length == 0)
    {
        lines.Add("CHANGED FILES: None");
    }
    else
    {
        lines.Add("CHANGED FILES:");
        foreach (var row in changedRows)
        {
            lines.Add($"  - {row.CurrentFileName} -> {row.NewFileName}");
        }
    }

    lines.Add(new string('=', 92));
    return string.Join(Environment.NewLine, lines);
}

static RenameDetection DetectRenameInfo(string filePath)
{
    var fileName = Path.GetFileNameWithoutExtension(filePath);
    var match = Regex.Match(fileName, @"(?<title>.*?)(?:[\s._\-\[]+)S(?<season>\d{1,3})\s*E(?<episode>\d{1,4})", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    if (match.Success)
    {
        return new RenameDetection(
            CleanSeriesTitle(match.Groups["title"].Value),
            ParseSortNumber(match.Groups["season"].Value, 0),
            ParseSortNumber(match.Groups["episode"].Value, 0),
            null);
    }

    var absolute = Regex.Match(fileName, @"(?<title>.*?)(?:[\s._\-\[]+)(?<episode>\d{1,4})(?:v\d+)?$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    if (absolute.Success)
    {
        var episodeNumber = ParseSortNumber(absolute.Groups["episode"].Value, 0);
        return new RenameDetection(CleanSeriesTitle(absolute.Groups["title"].Value), 1, episodeNumber, episodeNumber);
    }

    return new RenameDetection(CleanSeriesTitle(fileName), null, null, null);
}

static RenameSortKey GetRenameSortKey(string filePath)
{
    var fileName = Path.GetFileNameWithoutExtension(filePath);
    var episodeMatch = Regex.Match(
        fileName,
        @"(?:^|[\s._\-\[\(])S(?<season>\d{1,3})\s*E(?<episode>\d{1,4})(?:\s*[-+&]\s*E?(?<part>\d{1,4}))?",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    if (episodeMatch.Success)
    {
        return new RenameSortKey(
            ParseSortNumber(episodeMatch.Groups["season"].Value, int.MaxValue),
            ParseSortNumber(episodeMatch.Groups["episode"].Value, int.MaxValue),
            ParseSortNumber(episodeMatch.Groups["part"].Value, 0),
            Path.GetFileName(filePath));
    }

    var absoluteEpisodeMatch = Regex.Match(
        fileName,
        @"(?:^|[\s._\-\[\(])(?<episode>\d{1,4})(?:v\d+)?(?:$|[\s._\-\]\)])",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    if (absoluteEpisodeMatch.Success)
    {
        return new RenameSortKey(
            int.MaxValue - 1,
            ParseSortNumber(absoluteEpisodeMatch.Groups["episode"].Value, int.MaxValue),
            0,
            Path.GetFileName(filePath));
    }

    return new RenameSortKey(int.MaxValue, int.MaxValue, 0, Path.GetFileName(filePath));
}

static IEnumerable<TvdbEpisode> OrderRenameEpisodes(IEnumerable<TvdbEpisode> episodes)
    => episodes
        .OrderBy(episode => episode.SeasonNumber == 0 ? int.MaxValue : episode.SeasonNumber)
        .ThenBy(episode => episode.EpisodeNumber)
        .ThenBy(episode => episode.Name, StringComparer.OrdinalIgnoreCase);

static string FormatDetectedEpisode(int? season, int? episode)
    => season.HasValue && episode.HasValue ? $"S{season.Value:00}E{episode.Value:00}" : "Unknown";

static string CleanSeriesTitle(string value)
{
    value = value.Replace('.', ' ').Replace('_', ' ');
    value = Regex.Replace(value, @"\[[^\]]*\]|\([^\)]*\)", " ");
    value = Regex.Replace(value, @"\b(1080p|720p|2160p|480p|bluray|web[- ]?dl|webrip|x264|x265|hevc|avc|aac|flac|opus|10bit|8bit)\b", " ", RegexOptions.IgnoreCase);
    value = Regex.Replace(value, @"\s+", " ").Trim(' ', '-', '.');
    return value;
}

static int ParseSortNumber(string value, int fallback)
    => int.TryParse(value, out var parsed) ? parsed : fallback;

static string FirstNonBlank(params string?[] values)
    => values.FirstOrDefault(value => !string.IsNullOrWhiteSpace(value))?.Trim() ?? string.Empty;

static string NormalizeLookupProvider(string? provider)
{
    var value = (provider ?? string.Empty).Trim();
    return value.Equals("TMDB", StringComparison.OrdinalIgnoreCase)
        || value.Equals("TheMovieDB", StringComparison.OrdinalIgnoreCase)
        ? "TMDB"
        : "TVDB";
}

public sealed record ApiError(string Message);

public sealed record AppStatusResponse(
    string Name,
    string Version,
    string MediaRoot,
    string ConfigRoot,
    IReadOnlyList<SourceRoot> SourceRoots,
    IReadOnlyList<ToolStatus> Tools);

public sealed record SourceRoot(string Name, string Path);

public sealed record ToolStatus(
    string Name,
    string Command,
    string ResolvedPath,
    bool Available,
    string Version)
{
    public static ToolStatus Create(string name, string windowsName, string unixName)
    {
        var command = CrossPlatformRuntime.GetToolDisplayName(windowsName, unixName);
        var resolved = CrossPlatformRuntime.ResolveExecutable(command, windowsName, unixName);
        var pathHit = File.Exists(resolved) ? resolved : CrossPlatformRuntime.FindExecutableOnPath(resolved);
        var available = !string.IsNullOrWhiteSpace(pathHit);
        return new ToolStatus(name, command, string.IsNullOrWhiteSpace(pathHit) ? resolved : pathHit, available, available ? ReadVersion(pathHit!) : string.Empty);
    }

    private static string ReadVersion(string path)
    {
        try
        {
            using var process = Process.Start(new ProcessStartInfo
            {
                FileName = path,
                ArgumentList = { "--version" },
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                UseShellExecute = false,
                CreateNoWindow = true
            });

            if (process is null) return string.Empty;
            if (!process.WaitForExit(1500))
            {
                try { process.Kill(entireProcessTree: true); } catch { }
                return string.Empty;
            }

            var line = process.StandardOutput.ReadLine();
            return string.IsNullOrWhiteSpace(line) ? process.StandardError.ReadLine() ?? string.Empty : line;
        }
        catch
        {
            return string.Empty;
        }
    }
}

public sealed record FileSystemEntry(string Name, string Path, string Kind, long? SizeBytes, DateTime ModifiedUtc);

public sealed record FileSystemResponse(string Path, string? ParentPath, IReadOnlyList<FileSystemEntry> Entries);

public sealed record ScanRequest(
    string? SourcePath,
    IReadOnlyList<string>? Sources,
    IReadOnlyList<string>? IgnoredFolderNames,
    string? MkvMergePath,
    string? FfProbePath);

public sealed record ScanSummary(int Total, int Mkv, int Mp4, int Failed);

public sealed record CurrentScanResponse(
    DateTimeOffset? UpdatedUtc,
    IReadOnlyList<MediaFileRow> Files,
    ScanSummary Summary);

public sealed record ScanJobResponse(
    string Id,
    string Status,
    DateTimeOffset CreatedUtc,
    DateTimeOffset? StartedUtc,
    DateTimeOffset? CompletedUtc,
    string CurrentSource,
    int Completed,
    int Total,
    IReadOnlyList<MediaFileRow> Files,
    IReadOnlyList<string> Skipped,
    ScanSummary Summary,
    string Error);

public sealed record WebSettingsRequest(
    string? TvdbApiKey,
    string? TvdbPin,
    string? TmdbApiKey,
    string? TvdbLanguage,
    string? RenameLookupProvider,
    string? RenameTemplate,
    IReadOnlyList<string>? RenameTemplates,
    IReadOnlyList<string>? AudioNamePresets,
    IReadOnlyList<string>? SubtitleNamePresets,
    IReadOnlyList<string>? LanguagePresets,
    string? MkvMergeDefaultAudioLanguages,
    string? MkvMergeDefaultSubtitleLanguages,
    IReadOnlyList<string>? WatchFolders,
    bool? EnableLiveWatchFolderMonitoring,
    IReadOnlyList<WebMediaServerRequest>? MediaServers,
    IReadOnlyList<WebMediaServerPathMapping>? MediaServerPathMappings);

public sealed record WebSettingsResponse(
    bool HasTvdbApiKey,
    bool HasTvdbPin,
    bool HasTmdbApiKey,
    string TvdbLanguage,
    string RenameLookupProvider,
    string RenameTemplate,
    IReadOnlyList<string> RenameTemplates,
    IReadOnlyList<string> AudioNamePresets,
    IReadOnlyList<string> SubtitleNamePresets,
    IReadOnlyList<string> LanguagePresets,
    string MkvMergeDefaultAudioLanguages,
    string MkvMergeDefaultSubtitleLanguages,
    IReadOnlyList<string> WatchFolders,
    bool EnableLiveWatchFolderMonitoring,
    IReadOnlyList<WebMediaServerResponse> MediaServers,
    IReadOnlyList<WebMediaServerPathMapping> MediaServerPathMappings)
{
    public static WebSettingsResponse From(AppSettings settings)
        => new(
            HasTvdbApiKey: !string.IsNullOrWhiteSpace(settings.TvdbApiKey),
            HasTvdbPin: !string.IsNullOrWhiteSpace(settings.TvdbPin),
            HasTmdbApiKey: !string.IsNullOrWhiteSpace(settings.TmdbApiKey),
            TvdbLanguage: string.IsNullOrWhiteSpace(settings.TvdbLanguage) ? "eng" : settings.TvdbLanguage,
            RenameLookupProvider: string.IsNullOrWhiteSpace(settings.RenameLookupProvider) ? "TVDB" : settings.RenameLookupProvider,
            RenameTemplate: string.IsNullOrWhiteSpace(settings.RenameTemplate) ? "{series} - S{season:00}E{episode:00} - {episodeTitle}" : settings.RenameTemplate,
            RenameTemplates: settings.RenameTemplates,
            AudioNamePresets: settings.AudioNamePresets,
            SubtitleNamePresets: settings.SubtitleNamePresets,
            LanguagePresets: settings.LanguagePresets,
            MkvMergeDefaultAudioLanguages: string.IsNullOrWhiteSpace(settings.MkvMergeDefaultAudioLanguages) ? "eng,jpn" : settings.MkvMergeDefaultAudioLanguages,
            MkvMergeDefaultSubtitleLanguages: string.IsNullOrWhiteSpace(settings.MkvMergeDefaultSubtitleLanguages) ? "eng" : settings.MkvMergeDefaultSubtitleLanguages,
            WatchFolders: settings.WatchFolders,
            EnableLiveWatchFolderMonitoring: settings.EnableLiveWatchFolderMonitoring,
            MediaServers: settings.MediaServers.Select(WebMediaServerResponse.From).ToArray(),
            MediaServerPathMappings: settings.MediaServerPathMappings
                .Select(mapping => new WebMediaServerPathMapping(mapping.ServerPathPrefix, mapping.ContainerPathPrefix))
                .ToArray());
}

public sealed record WebMediaServerRequest(
    string? Id,
    string? Name,
    string? Type,
    string? ServerUrl,
    string? ApiKey,
    bool IsDefault,
    IReadOnlyList<WebMediaServerLibraryPath>? Libraries);

public sealed record WebMediaServerResponse(
    string Id,
    string Name,
    string Type,
    string ServerUrl,
    bool HasApiKey,
    bool IsDefault,
    DateTimeOffset? LastSyncedUtc,
    IReadOnlyList<WebMediaServerLibraryPath> Libraries)
{
    public static WebMediaServerResponse From(MediaServerSettings server)
        => new(
            Id: server.Id,
            Name: server.Name,
            Type: string.IsNullOrWhiteSpace(server.Type) ? "Emby" : server.Type.Trim(),
            ServerUrl: server.ServerUrl,
            HasApiKey: !string.IsNullOrWhiteSpace(server.ApiKey),
            IsDefault: server.IsDefault,
            LastSyncedUtc: server.LastSyncedUtc,
            Libraries: server.Libraries.Select(library => new WebMediaServerLibraryPath(
                library.Id,
                library.Name,
                library.Type,
                library.ServerPath,
                library.ContainerPath,
                library.IsEnabled)).ToArray());
}

public sealed record WebMediaServerLibraryPath(
    string Id,
    string Name,
    string Type,
    string ServerPath,
    string ContainerPath,
    bool IsEnabled);

public sealed record WebMediaServerPathMapping(string ServerPathPrefix, string ContainerPathPrefix);

public sealed record MediaServerConnectionRequest(string? Id, string? Name, string? Type, string? ServerUrl, string? ApiKey);

public sealed record MediaServerTestResponse(bool Success, string Status, int LibraryCount);

public sealed record MediaServerSyncResponse(
    WebMediaServerResponse Server,
    IReadOnlyList<WebMediaServerLibraryPath> Libraries,
    string Status);

public sealed record MuxPreviewRequest(
    IReadOnlyList<MediaFileRow>? Files,
    IReadOnlyList<string>? SelectedPaths,
    bool RemoveUnwantedAudioLanguages,
    string? KeepAudioLanguages,
    bool RemoveUnwantedSubtitleLanguages,
    string? KeepSubtitleLanguages,
    bool RemoveUnwantedTrackIds,
    string? RemoveTrackIdsText,
    bool PreserveChapters,
    bool PreserveAttachments,
    bool MuxMatchingExternalSubtitles,
    string? ExternalSubtitleLanguage,
    string? ExternalSubtitleFormats,
    bool PreserveExternalSubtitleFiles,
    bool SkipMuxIfSubtitleAlreadyExists,
    bool ExtractSubtitles,
    string? ExtractSubtitleLanguages,
    bool ExtractOverwriteExistingFiles,
    bool ConvertMp4ToMkv = false,
    bool DeleteMp4AfterConvert = false);

public sealed record MuxActionRow(
    int Index,
    string FilePath,
    string FileName,
    string Operation,
    string ToolName,
    string Description,
    string Command);

public sealed record MuxPreviewResponse(
    IReadOnlyList<MuxActionRow> Actions,
    IReadOnlyList<string> NoChangeFiles,
    string Summary,
    string Status);

public sealed record PropEditTemplateRequest(IReadOnlyList<MediaFileRow>? Files, string? TemplatePath);

public sealed record PropEditTemplateResponse(
    string TemplatePath,
    string TemplateFileName,
    IReadOnlyList<PropEditTrackConfigRow> AudioTracks,
    IReadOnlyList<PropEditTrackConfigRow> SubtitleTracks,
    string DefaultAudio,
    string ForcedAudio,
    string DefaultSubtitle,
    string ForcedSubtitle);

public sealed record PropEditTrackConfigRow(
    int TrackNumber,
    string TrackLabel,
    string Type,
    string CurrentName,
    string CurrentLanguage,
    bool CurrentDefault,
    string EditedName,
    string EditedLanguage);

public sealed record PropEditPreviewRequest(
    IReadOnlyList<MediaFileRow>? Files,
    IReadOnlyList<string>? SelectedPaths,
    string? TemplatePath,
    string? ContainerTitleMode,
    string? CustomContainerTitle,
    string? VideoTitleMode,
    string? CustomVideoTitle,
    IReadOnlyList<PropEditTrackConfigRow>? AudioTracks,
    IReadOnlyList<PropEditTrackConfigRow>? SubtitleTracks,
    string? SelectedDefaultAudio,
    string? SelectedForcedAudio,
    string? SelectedDefaultSubtitle,
    string? SelectedForcedSubtitle);

public sealed record PropEditActionRow(
    int Index,
    string FilePath,
    string FileName,
    string Description,
    string Command);

public sealed record PropEditSkippedRow(string FilePath, string FileName, string Reason);

public sealed record PropEditNoChangeRow(string FilePath, string FileName, string Reason);

public sealed record PropEditPreviewResponse(
    IReadOnlyList<PropEditActionRow> Actions,
    IReadOnlyList<PropEditSkippedRow> Skipped,
    IReadOnlyList<PropEditNoChangeRow> NoChange,
    string Summary,
    string Status);

public sealed class WebPropEditPlan
{
    public List<PlannedAction> Actions { get; } = new();
    public List<PropEditSkippedRow> Skipped { get; } = new();
    public List<PropEditNoChangeRow> NoChange { get; } = new();
}

public sealed record LibraryAuditRequest(IReadOnlyList<MediaFileRow>? Files);

public sealed record LibraryAuditSummary(int Groups, int Files, int IssueGroups, int StandardGroups);

public sealed record LibraryAuditRow(
    string FolderPath,
    string FolderName,
    int FileCount,
    string StandardVideo,
    string StandardAudio,
    string StandardSubtitles,
    string TemplateFilePath,
    string TemplateFileName,
    bool HasIssues,
    string IssueSummary,
    IReadOnlyList<string> Issues,
    IReadOnlyList<string> IssueFilePaths,
    IReadOnlyList<string> AllFilePaths);

public sealed record LibraryAuditResponse(LibraryAuditSummary Summary, IReadOnlyList<LibraryAuditRow> Items);

public sealed record OperationLogResponse(IReadOnlyList<OperationLogEntry> Entries);

public sealed record OperationLogEntry(DateTimeOffset TimestampUtc, string Area, string Message, string Detail);

public sealed record RenameSearchRequest(string? Query, string? Provider, string? Language);

public sealed record RenameSearchResponse(IReadOnlyList<TvdbSeriesSearchResult> Results);

public sealed record RenameScopesRequest(TvdbSeriesSearchResult? SelectedResult, string? Provider, string? Language);

public sealed record RenameScopesResponse(IReadOnlyList<RenameScopeRow> Scopes);

public sealed record RenameScopeRow(string Key, string Label, bool IsSelected);

public sealed record RenamePreviewRequest(
    IReadOnlyList<MediaFileRow>? Files,
    TvdbSeriesSearchResult? SelectedResult,
    string? Provider,
    string? Language,
    string? ScopeKey,
    IReadOnlyList<string>? ScopeKeys,
    string? Template);

public sealed record RenameProviderTestRequest(string? Provider, string? Language);

public sealed record RenameProviderTestResponse(bool Success, string Status);

public sealed record RenamePreviewResponse(
    IReadOnlyList<RenamePreviewRow> Items,
    string Summary,
    IReadOnlyList<RenameScopeRow> Scopes,
    string Status);

public sealed record RenameApplyRequest(IReadOnlyList<RenamePreviewRow>? Items, string? Provider, string? Template);

public sealed record RenameApplyResponse(IReadOnlyList<RenamePreviewRow> Items, string Summary, string Status);

public sealed record RenameBatchListResponse(IReadOnlyList<RenameBatchRecord> Batches);

public sealed record RenameBatchUndoPreviewResponse(int Restorable, int Skipped, IReadOnlyList<string> Lines, bool HasSkippedFiles)
{
    public static RenameBatchUndoPreviewResponse From(RenameBatchUndoPreview preview)
        => new(preview.Restorable, preview.Skipped, preview.Lines, preview.HasSkippedFiles);
}

public sealed record RenameBatchUndoResponse(
    int Renamed,
    int Skipped,
    IReadOnlyList<string> Lines,
    IReadOnlyList<RenameBatchRestoreMove> Restored);

public sealed record RenameBatchRestoreMove(string OriginalPath, string RenamedPath, string OriginalFileName);

public sealed record RenamePreviewRow(
    bool Selected,
    string SourcePath,
    string CurrentFileName,
    string Detected,
    string EpisodeName,
    string NewFileName,
    string Confidence,
    string Status,
    bool CanApply);

public sealed record RenameSourceFile(
    string Path,
    string CurrentFileName,
    string SeriesTitle,
    int? Season,
    int? Episode,
    int? AbsoluteEpisode,
    RenameSortKey SortKey);

public sealed record RenameSortKey(int Season, int Episode, int Part, string RelativeName);

public sealed record RenameDetection(string SeriesTitle, int? Season, int? Episode, int? AbsoluteEpisode);

public sealed record RenamePreviewBuildResult(IReadOnlyList<RenamePreviewRow> Items, string Summary, string Status);

public sealed record TrackRow(
    int Id,
    int TrackNumber,
    string Type,
    string Codec,
    string Language,
    string Name,
    bool Default,
    bool Forced);

public sealed record AttachmentRow(
    int Id,
    string FileName,
    string ContentType,
    string Description,
    long? SizeBytes);

public sealed record MediaFileRow(
    string Path,
    string FileName,
    string Extension,
    string Status,
    string Reader,
    string Codec,
    string Resolution,
    string BitDepth,
    string Hdr,
    string VideoSummary,
    string AudioSummary,
    string SubtitleSummary,
    string AttachmentSummary,
    IReadOnlyList<TrackRow> Tracks,
    IReadOnlyList<AttachmentRow> Attachments)
{
    public static MediaFileRow From(MkvFileItem item)
    {
        return new MediaFileRow(
            Path: item.FilePath,
            FileName: item.FileName,
            Extension: System.IO.Path.GetExtension(item.FilePath),
            Status: item.Status,
            Reader: MkvScannerService.GetPrimaryMetadataReaderName(item.FilePath),
            Codec: item.Codec,
            Resolution: item.Resolution,
            BitDepth: item.BitDepth,
            Hdr: item.Hdr,
            VideoSummary: item.VideoSummary,
            AudioSummary: item.AudioSummary,
            SubtitleSummary: item.SubtitleSummary,
            AttachmentSummary: item.AttachmentSummary,
            Tracks: item.Tracks.Select(track => new TrackRow(
                Id: track.MkvMergeId,
                TrackNumber: track.PropEditTrackNumber,
                Type: track.Type,
                Codec: track.Codec,
                Language: track.Language,
                Name: track.Name,
                Default: track.Default,
                Forced: track.Forced)).ToArray(),
            Attachments: item.Attachments.Select(attachment => new AttachmentRow(
                Id: attachment.Id,
                FileName: attachment.FileName,
                ContentType: attachment.ContentType,
                Description: attachment.Description,
                SizeBytes: attachment.SizeBytes)).ToArray());
    }
}

public sealed class ScanJobStore
{
    private readonly ConcurrentDictionary<string, ScanJobState> _jobs = new();

    public ScanJobState Create()
    {
        var job = new ScanJobState(Guid.NewGuid().ToString("N"));
        _jobs[job.Id] = job;
        PruneCompletedJobs();
        return job;
    }

    public bool TryGet(string id, out ScanJobState job) => _jobs.TryGetValue(id, out job!);

    private void PruneCompletedJobs()
    {
        var cutoff = DateTimeOffset.UtcNow.AddMinutes(-30);
        foreach (var job in _jobs.Values)
        {
            var response = job.ToResponse();
            if (response.CompletedUtc is not null && response.CompletedUtc < cutoff)
            {
                _jobs.TryRemove(response.Id, out _);
            }
        }
    }
}

public sealed class CurrentScanStore
{
    private readonly object _sync = new();
    private IReadOnlyList<MediaFileRow> _files = Array.Empty<MediaFileRow>();
    private ScanSummary _summary = new(0, 0, 0, 0);
    private DateTimeOffset? _updatedUtc;

    public void Set(IReadOnlyList<MediaFileRow> files, ScanSummary summary)
    {
        lock (_sync)
        {
            _files = files.OrderBy(file => file.Path, StringComparer.OrdinalIgnoreCase).ToArray();
            _summary = summary;
            _updatedUtc = DateTimeOffset.UtcNow;
        }
    }

    public void Upsert(MediaFileRow row)
    {
        lock (_sync)
        {
            var next = _files
                .Where(file => !CrossPlatformRuntime.PathComparer.Equals(file.Path, row.Path))
                .Append(row)
                .OrderBy(file => file.Path, StringComparer.OrdinalIgnoreCase)
                .ToArray();
            _files = next;
            _summary = BuildSummary(next);
            _updatedUtc = DateTimeOffset.UtcNow;
        }
    }

    public void Clear()
    {
        lock (_sync)
        {
            _files = Array.Empty<MediaFileRow>();
            _summary = new ScanSummary(0, 0, 0, 0);
            _updatedUtc = DateTimeOffset.UtcNow;
        }
    }

    public CurrentScanResponse ToResponse()
    {
        lock (_sync)
        {
            return new CurrentScanResponse(_updatedUtc, _files, _summary);
        }
    }

    private static ScanSummary BuildSummary(IReadOnlyList<MediaFileRow> files)
        => new(
            Total: files.Count,
            Mkv: files.Count(row => row.Extension.Equals(".mkv", StringComparison.OrdinalIgnoreCase)),
            Mp4: files.Count(row => row.Extension.Equals(".mp4", StringComparison.OrdinalIgnoreCase)),
            Failed: files.Count(row => row.Status.Contains("failed", StringComparison.OrdinalIgnoreCase)));
}

public sealed class ScanJobState
{
    private readonly object _sync = new();
    private readonly CancellationTokenSource _cts = new();
    private readonly List<MediaFileRow> _files = new();
    private IReadOnlyList<string> _skipped = Array.Empty<string>();
    private ScanSummary _summary = new(0, 0, 0, 0);

    public ScanJobState(string id)
    {
        Id = id;
        CreatedUtc = DateTimeOffset.UtcNow;
    }

    public string Id { get; }
    public DateTimeOffset CreatedUtc { get; }
    public DateTimeOffset? StartedUtc { get; private set; }
    public DateTimeOffset? CompletedUtc { get; private set; }
    public string Status { get; private set; } = "Queued";
    public string CurrentSource { get; private set; } = string.Empty;
    public int Completed { get; private set; }
    public int Total { get; private set; }
    public string Error { get; private set; } = string.Empty;
    public CancellationToken Token => _cts.Token;

    public void MarkRunning()
    {
        lock (_sync)
        {
            StartedUtc = DateTimeOffset.UtcNow;
            Status = "Running";
        }
    }

    public void SetCurrentSource(string source)
    {
        lock (_sync)
        {
            CurrentSource = source;
        }
    }

    public void SetProgress(int completed, int total)
    {
        lock (_sync)
        {
            Completed = Math.Max(0, completed);
            Total = Math.Max(0, total);
        }
    }

    public void AddFile(MediaFileRow file)
    {
        lock (_sync)
        {
            _files.Add(file);
            _summary = BuildSummary(_files);
        }
    }

    public void MarkCompleted(IReadOnlyList<MediaFileRow> files, IReadOnlyList<string> skipped, ScanSummary summary)
    {
        lock (_sync)
        {
            _files.Clear();
            _files.AddRange(files);
            _skipped = skipped;
            _summary = summary;
            Completed = summary.Total;
            Total = Math.Max(Total, summary.Total);
            Status = "Completed";
            CompletedUtc = DateTimeOffset.UtcNow;
        }
    }

    public void Cancel()
    {
        lock (_sync)
        {
            if (Status is "Completed" or "Failed" or "Canceled") return;
            Status = "Canceling";
        }

        _cts.Cancel();
    }

    public void MarkCanceled()
    {
        lock (_sync)
        {
            Status = "Canceled";
            CompletedUtc = DateTimeOffset.UtcNow;
        }
    }

    public void MarkFailed(string error)
    {
        lock (_sync)
        {
            Status = "Failed";
            Error = error;
            CompletedUtc = DateTimeOffset.UtcNow;
        }
    }

    public ScanJobResponse ToResponse()
    {
        lock (_sync)
        {
            return new ScanJobResponse(
                Id,
                Status,
                CreatedUtc,
                StartedUtc,
                CompletedUtc,
                CurrentSource,
                Completed,
                Total,
                _files.OrderBy(file => file.Path, StringComparer.OrdinalIgnoreCase).ToArray(),
                _skipped,
                _summary,
                Error);
        }
    }

    private static ScanSummary BuildSummary(IReadOnlyList<MediaFileRow> files)
        => new(
            Total: files.Count,
            Mkv: files.Count(row => row.Extension.Equals(".mkv", StringComparison.OrdinalIgnoreCase)),
            Mp4: files.Count(row => row.Extension.Equals(".mp4", StringComparison.OrdinalIgnoreCase)),
            Failed: files.Count(row => row.Status.Contains("failed", StringComparison.OrdinalIgnoreCase)));
}

public sealed class OperationLogStore
{
    private readonly object _sync = new();
    private readonly List<OperationLogEntry> _entries = new();

    public void Add(string area, string message, string detail = "")
    {
        lock (_sync)
        {
            _entries.Insert(0, new OperationLogEntry(DateTimeOffset.UtcNow, area, message, detail));
            if (_entries.Count > 300)
            {
                _entries.RemoveRange(300, _entries.Count - 300);
            }
        }
    }

    public IReadOnlyList<OperationLogEntry> List()
    {
        lock (_sync)
        {
            return _entries.ToArray();
        }
    }

    public void Clear()
    {
        lock (_sync)
        {
            _entries.Clear();
        }
    }
}
