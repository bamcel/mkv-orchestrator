using Avalonia.Threading;
using CommunityToolkit.Mvvm.Input;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{
    private static readonly TimeSpan TempCacheRetention = TimeSpan.FromDays(7);

    [RelayCommand]
    private async Task BuildCacheFromWatchFolders()
    {
        if (IsCacheBusy) return;
        var roots = ParseWatchFolderText(WatchFolderText).Where(Directory.Exists).ToList();
        if (roots.Count == 0)
        {
            CacheStatusText = "No valid watch folders configured.";
            Log(CacheStatusText);
            return;
        }

        _cacheCts?.Cancel();
        _cacheCts?.Dispose();
        _cacheCts = new CancellationTokenSource();

        IsCacheBusy = true;
        CacheStatusText = "Building metadata cache...";
        BeginGlobalOperation("cache build");
        Log(CacheStatusText);
        try
        {
            var ignored = ParseIgnoredScanFolderNames(IgnoredScanFolderNameText).ToList();
            var result = await _mediaLibrary.BuildCacheFromWatchFoldersAsync(
                roots,
                MkvMergePath,
                FfProbePath,
                ignored,
                _workerSettings.CloneNormalized(),
                Log,
                status =>
                {
                    Dispatcher.UIThread.Post(() =>
                    {
                        CacheStatusText = status;
                        if (TryParseCacheValidationStatus(status, out var completed, out var total, out var currentItem))
                        {
                            UpdateGlobalOperation(completed, total, currentItem);
                        }
                        else
                        {
                            StatusText = status;
                            ExecutionStatusText = $"Execution Center: {status}";
                        }
                    });
                },
                _cacheCts.Token);

            RefreshCacheLifecycleStatus();
            CacheStatusText = $"Watch cache validation complete: {result.Skipped} skipped, {result.Updated} updated, {result.Failed} failed, {result.StaleRemoved} stale removed. Watch entries: {CacheWatchEntryCountText}. Temp entries: {CacheTempEntryCountText}.";
            CompleteGlobalOperation(CacheStatusText);
            Log(CacheStatusText);
        }
        catch (OperationCanceledException)
        {
            CacheStatusText = "Cache build canceled.";
            CompleteGlobalOperation(CacheStatusText);
            Log(CacheStatusText);
        }
        finally
        {
            IsCacheBusy = false;
            _cacheCts?.Dispose();
            _cacheCts = null;
        }
    }

    [RelayCommand]
    private void ClearMetadataCache()
    {
        if (IsCacheBusy)
        {
            CacheStatusText = "Watch cache clear skipped: cache build is currently running.";
            Log(CacheStatusText);
            return;
        }

        var removed = _mediaCache.Clear();
        RefreshCacheLifecycleStatus();
        CacheStatusText = $"Watch-folder cache cleared: {removed} entries removed. Watch entries: {CacheWatchEntryCountText}. Temp entries: {CacheTempEntryCountText}.";
        Log(CacheStatusText);
    }

    [RelayCommand]
    private void ClearTempMetadataCache()
    {
        if (IsBusy || IsCacheBusy)
        {
            CacheStatusText = "Temp cache clear skipped: a scan or cache build is currently running.";
            Log(CacheStatusText);
            return;
        }

        var removed = _tempMediaCache.Clear();
        CacheLastCleanupText = $"Manual clear: {DateTime.Now:g}";
        RefreshCacheLifecycleStatus();
        CacheStatusText = $"Temp scan cache cleared: {removed} entries removed. Watch entries: {CacheWatchEntryCountText}. Temp entries: {CacheTempEntryCountText}.";
        Log(CacheStatusText);
    }

    private int PruneExpiredTempMetadataCache()
    {
        var cutoffUtc = DateTime.UtcNow.Subtract(TempCacheRetention);
        var removed = _tempMediaCache.RemoveOlderThan(cutoffUtc);
        CacheLastCleanupText = removed > 0
            ? $"Auto cleanup: {DateTime.Now:g} ({removed} removed)"
            : $"Auto cleanup: {DateTime.Now:g} (none expired)";
        RefreshCacheLifecycleStatus();
        return removed;
    }

    private static bool TryParseCacheValidationStatus(string status, out int completed, out int total, out string currentItem)
    {
        completed = 0;
        total = 0;
        currentItem = string.Empty;

        const string prefix = "Validating ";
        if (!status.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) return false;

        var remaining = status[prefix.Length..];
        var ofIndex = remaining.IndexOf(" of ", StringComparison.OrdinalIgnoreCase);
        if (ofIndex < 0) return false;

        var totalStart = ofIndex + " of ".Length;
        var colonIndex = remaining.IndexOf(':', totalStart);
        if (colonIndex < 0) return false;

        if (!int.TryParse(remaining[..ofIndex], out completed)) return false;
        if (!int.TryParse(remaining[totalStart..colonIndex], out total)) return false;

        currentItem = remaining[(colonIndex + 1)..].Trim();
        return true;
    }

    [RelayCommand]
    private async Task RestartMetadataWatchers()
    {
        await RestartWatchersAsync(force: true);
    }

    private async Task EnsureWatchersInitializedAsync()
    {
        await RestartWatchersAsync(force: false);
    }

    private async Task RestartWatchersAsync(bool force = true)
    {
        if (!EnableLiveWatchFolderMonitoring)
        {
            RefreshCacheLifecycleStatus();
            CacheStatusText = $"Live watcher disabled. Watch entries: {CacheWatchEntryCountText}. Temp entries: {CacheTempEntryCountText}.";
            await _watchFolders.RestartAsync(force, false, Array.Empty<string>(), QueueWatchedPathRefresh, RemoveWatchedPathFromCache, Log);
            return;
        }

        CacheStatusText = "Initializing live watcher...";
        var count = await _watchFolders.RestartAsync(
            force,
            true,
            ParseWatchFolderText(WatchFolderText),
            QueueWatchedPathRefresh,
            RemoveWatchedPathFromCache,
            Log);
        RefreshCacheLifecycleStatus();
        CacheStatusText = $"Recursive live watcher active: {count} root(s). Watch entries: {CacheWatchEntryCountText}. Temp entries: {CacheTempEntryCountText}.";
        Log(CacheStatusText);
    }

    private void StopWatchers()
    {
        _watchFolders.Stop();

        lock (_watchGate)
        {
            foreach (var cts in _watchDebounce.Values) cts.Cancel();
            _watchDebounce.Clear();
        }
    }

    private void PostUiLog(string message)
    {
        Dispatcher.UIThread.Post(() => Log(message));
    }

    private void QueueWatchedPathRefresh(string path)
    {
        path = CrossPlatformRuntime.NormalizeUserPath(path);
        if (File.Exists(path))
        {
            QueueWatchedFileRefresh(path);
            return;
        }

        if (!Directory.Exists(path)) return;

        try
        {
            var ignored = ParseIgnoredScanFolderNames(IgnoredScanFolderNameText).ToList();
            foreach (var file in MkvScannerService.EnumerateMediaFiles(path, ignored, CancellationToken.None))
            {
                QueueWatchedFileRefresh(file);
            }
        }
        catch (Exception ex)
        {
            PostUiLog($"Watcher directory refresh failed: {Path.GetFileName(path)} - {ex.Message}");
        }
    }

    private void RemoveWatchedPathFromCache(string path)
    {
        path = CrossPlatformRuntime.NormalizeUserPath(path);
        if (CrossPlatformRuntime.IsSupportedMediaPath(path))
        {
            _mediaCache.Remove(path);
        }
        else
        {
            var removed = _mediaCache.RemoveUnderPath(path);
            if (removed > 0) PostUiLog($"Watcher removed stale cache entries: {removed}");
        }
    }

    private void QueueWatchedFileRefresh(string filePath)
    {
        filePath = CrossPlatformRuntime.NormalizeUserPath(filePath);
        if (!CrossPlatformRuntime.IsSupportedMediaPath(filePath)) return;
        CancellationTokenSource cts;
        lock (_watchGate)
        {
            if (_watchDebounce.TryGetValue(filePath, out var existing)) existing.Cancel();
            cts = new CancellationTokenSource();
            _watchDebounce[filePath] = cts;
        }

        _ = Task.Run(async () =>
        {
            try
            {
                await Task.Delay(TimeSpan.FromSeconds(3), cts.Token);
                if (!File.Exists(filePath))
                {
                    _mediaCache.Remove(filePath);
                    return;
                }

                var media = await _mediaLibrary.ScanFileAsync(filePath, MkvMergePath, FfProbePath, cts.Token, forceRefresh: true);
                PostUiLog(media.Tracks.Count > 0
                    ? $"Watcher cache refreshed: {Path.GetFileName(filePath)} | Tracks: {media.Tracks.Count}"
                    : $"Watcher refresh warning: {Path.GetFileName(filePath)} returned no tracks.");
            }
            catch (OperationCanceledException) { }
            catch (Exception ex)
            {
                PostUiLog($"Watcher refresh failed: {Path.GetFileName(filePath)} - {ex.Message}");
            }
            finally
            {
                lock (_watchGate)
                {
                    if (_watchDebounce.TryGetValue(filePath, out var current) && ReferenceEquals(current, cts))
                    {
                        _watchDebounce.Remove(filePath);
                    }
                }
                cts.Dispose();
            }
        });
    }

    private bool IsPathUnderAnyWatchFolder(string path)
    {
        if (string.IsNullOrWhiteSpace(path)) return false;
        var normalizedPath = NormalizeCacheRootComparisonPath(path);
        if (string.IsNullOrWhiteSpace(normalizedPath)) return false;

        foreach (var root in ParseWatchFolderText(WatchFolderText))
        {
            var normalizedRoot = NormalizeCacheRootComparisonPath(root);
            if (string.IsNullOrWhiteSpace(normalizedRoot)) continue;

            if (string.Equals(normalizedPath, normalizedRoot, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }

            var prefix = normalizedRoot + Path.DirectorySeparatorChar;
            var altPrefix = normalizedRoot + Path.AltDirectorySeparatorChar;
            if (normalizedPath.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
                || normalizedPath.StartsWith(altPrefix, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }

    private static string NormalizeCacheRootComparisonPath(string path)
    {
        var normalized = CrossPlatformRuntime.NormalizeUserPath(path);
        if (string.IsNullOrWhiteSpace(normalized)) return string.Empty;

        try
        {
            normalized = Path.GetFullPath(normalized);
        }
        catch
        {
            // Keep the normalized user path if GetFullPath cannot resolve a disconnected UNC/network path.
        }

        return normalized.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
    }

}
