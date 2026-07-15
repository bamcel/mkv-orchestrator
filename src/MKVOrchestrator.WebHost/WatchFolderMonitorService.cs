using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.WebHost;

/// <summary>
/// Container-side equivalent of the desktop live watch-folder monitor. When
/// EnableLiveWatchFolderMonitoring is on, configured watch folders are observed
/// recursively and the shared metadata cache is refreshed for created/changed
/// files and pruned for deleted or renamed files.
/// </summary>
public sealed class WatchFolderMonitorService : IHostedService, IDisposable
{
    private static readonly TimeSpan DebounceDelay = TimeSpan.FromSeconds(3);

    private readonly AppSettingsService _settingsService;
    private readonly MkvScannerService _scanner;
    private readonly ILogger<WatchFolderMonitorService> _logger;
    private readonly List<FileSystemWatcher> _watchers = new();
    private readonly object _debounceGate = new();
    private readonly Dictionary<string, CancellationTokenSource> _debounce = new();
    private readonly SemaphoreSlim _restartGate = new(1, 1);

    public WatchFolderMonitorService(
        AppSettingsService settingsService,
        MkvScannerService scanner,
        ILogger<WatchFolderMonitorService> logger)
    {
        _settingsService = settingsService;
        _scanner = scanner;
        _logger = logger;
    }

    public bool IsMonitoring
    {
        get
        {
            lock (_watchers)
            {
                return _watchers.Count > 0;
            }
        }
    }

    public Task StartAsync(CancellationToken cancellationToken) => RestartAsync();

    public Task StopAsync(CancellationToken cancellationToken)
    {
        StopWatchers();
        return Task.CompletedTask;
    }

    public async Task RestartAsync()
    {
        await _restartGate.WaitAsync();
        try
        {
            StopWatchers();

            var settings = _settingsService.Load();
            if (!settings.EnableLiveWatchFolderMonitoring)
            {
                _logger.LogInformation("Live watch-folder monitoring is disabled.");
                return;
            }

            // Directory.Exists can block on unavailable network mounts; validate off the caller's thread.
            var roots = await Task.Run(() => settings.WatchFolders
                .Select(CrossPlatformRuntime.NormalizeUserPath)
                .Where(root => !string.IsNullOrWhiteSpace(root))
                .Distinct(CrossPlatformRuntime.PathComparer)
                .Where(Directory.Exists)
                .ToList());

            foreach (var root in roots)
            {
                try
                {
                    var watcher = new FileSystemWatcher(root, "*.*")
                    {
                        IncludeSubdirectories = true,
                        EnableRaisingEvents = true,
                        NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName | NotifyFilters.LastWrite | NotifyFilters.Size,
                        InternalBufferSize = 64 * 1024
                    };
                    watcher.Created += (_, e) => QueuePathRefresh(e.FullPath);
                    watcher.Changed += (_, e) => QueuePathRefresh(e.FullPath);
                    watcher.Deleted += (_, e) => RemovePathFromCache(e.FullPath);
                    watcher.Renamed += (_, e) =>
                    {
                        RemovePathFromCache(e.OldFullPath);
                        QueuePathRefresh(e.FullPath);
                    };

                    lock (_watchers)
                    {
                        _watchers.Add(watcher);
                    }
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(ex, "Watch folder monitor failed for {Root}", root);
                }
            }

            _logger.LogInformation("Live watch-folder monitoring active for {Count} root(s).", roots.Count);
        }
        finally
        {
            _restartGate.Release();
        }
    }

    private void StopWatchers()
    {
        lock (_watchers)
        {
            foreach (var watcher in _watchers)
            {
                try
                {
                    watcher.EnableRaisingEvents = false;
                    watcher.Dispose();
                }
                catch
                {
                    // Best-effort teardown.
                }
            }

            _watchers.Clear();
        }

        lock (_debounceGate)
        {
            foreach (var cts in _debounce.Values) cts.Cancel();
            _debounce.Clear();
        }
    }

    private void QueuePathRefresh(string path)
    {
        path = CrossPlatformRuntime.NormalizeUserPath(path);
        if (File.Exists(path))
        {
            QueueFileRefresh(path);
            return;
        }

        if (!Directory.Exists(path)) return;

        try
        {
            var ignored = _settingsService.Load().IgnoredScanFolderNames;
            foreach (var file in MkvScannerService.EnumerateMediaFiles(path, ignored, CancellationToken.None))
            {
                QueueFileRefresh(file);
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Watch folder directory refresh failed for {Path}", path);
        }
    }

    private void RemovePathFromCache(string path)
    {
        path = CrossPlatformRuntime.NormalizeUserPath(path);
        if (CrossPlatformRuntime.IsSupportedMediaPath(path))
        {
            _scanner.Cache.Remove(path);
        }
        else
        {
            var removed = _scanner.Cache.RemoveUnderPath(path);
            if (removed > 0)
            {
                _logger.LogInformation("Watch folder monitor removed {Count} stale cache entries under {Path}", removed, path);
            }
        }
    }

    private void QueueFileRefresh(string filePath)
    {
        filePath = CrossPlatformRuntime.NormalizeUserPath(filePath);
        if (!CrossPlatformRuntime.IsSupportedMediaPath(filePath)) return;

        CancellationTokenSource cts;
        lock (_debounceGate)
        {
            if (_debounce.TryGetValue(filePath, out var existing)) existing.Cancel();
            cts = new CancellationTokenSource();
            _debounce[filePath] = cts;
        }

        _ = Task.Run(async () =>
        {
            try
            {
                await Task.Delay(DebounceDelay, cts.Token);
                if (!File.Exists(filePath))
                {
                    _scanner.Cache.Remove(filePath);
                    return;
                }

                var mkvMerge = CrossPlatformRuntime.GetToolDisplayName("mkvmerge.exe", "mkvmerge");
                var ffProbe = CrossPlatformRuntime.GetToolDisplayName("ffprobe.exe", "ffprobe");
                var item = await _scanner.ScanFileAsync(filePath, mkvMerge, ffProbe, cts.Token, forceRefresh: true);
                _logger.LogInformation("Watch folder cache refreshed: {File} ({Tracks} track(s))", Path.GetFileName(filePath), item.Tracks.Count);
            }
            catch (OperationCanceledException)
            {
                // Debounced or shutdown.
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Watch folder refresh failed for {File}", Path.GetFileName(filePath));
            }
            finally
            {
                lock (_debounceGate)
                {
                    if (_debounce.TryGetValue(filePath, out var current) && ReferenceEquals(current, cts))
                    {
                        _debounce.Remove(filePath);
                    }
                }

                cts.Dispose();
            }
        });
    }

    public void Dispose()
    {
        StopWatchers();
        _restartGate.Dispose();
    }
}
