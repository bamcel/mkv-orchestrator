namespace MKVOrchestrator.App.Workflows;

public sealed class WatchFolderCoordinator : IDisposable
{
    private readonly List<FileSystemWatcher> _watchers = new();
    private readonly SemaphoreSlim _initializationGate = new(1, 1);
    private bool _initialized;
    private bool _disposed;

    public int WatcherCount => _watchers.Count;

    public async Task<int> RestartAsync(
        bool force,
        bool enabled,
        IEnumerable<string> configuredRoots,
        Action<string> refreshPath,
        Action<string> removePath,
        Action<string> logError)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
        await _initializationGate.WaitAsync();
        try
        {
            if (!force && _initialized) return WatcherCount;

            Stop();
            if (!enabled)
            {
                _initialized = true;
                return 0;
            }

            var roots = await Task.Run(() => configuredRoots.Where(Directory.Exists).ToList());
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
                    watcher.Created += (_, e) => refreshPath(e.FullPath);
                    watcher.Changed += (_, e) => refreshPath(e.FullPath);
                    watcher.Deleted += (_, e) => removePath(e.FullPath);
                    watcher.Renamed += (_, e) =>
                    {
                        removePath(e.OldFullPath);
                        refreshPath(e.FullPath);
                    };
                    _watchers.Add(watcher);
                }
                catch (Exception ex)
                {
                    logError($"Watcher failed for {root}: {ex.Message}");
                }
            }

            _initialized = true;
            return WatcherCount;
        }
        finally
        {
            _initializationGate.Release();
        }
    }

    public void Stop()
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
                // Best-effort shutdown: a watcher can already be invalidated by an offline share.
            }
        }

        _watchers.Clear();
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        Stop();
        _initializationGate.Dispose();
    }
}
