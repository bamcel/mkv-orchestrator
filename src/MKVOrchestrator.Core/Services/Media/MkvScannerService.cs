using System.Runtime.CompilerServices;
using System.Threading.Channels;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services.Cache;

namespace MKVOrchestrator.Core.Services;

public sealed class MkvScannerService
{
    private readonly MkvMergeService _mkvMerge = new();
    private readonly FfProbeService _ffProbe = new();
    private readonly MetadataCacheDatabase _cache;

    public MkvScannerService()
        : this(new MetadataCacheDatabase())
    {
    }

    public MkvScannerService(MetadataCacheDatabase cache)
    {
        _cache = cache;
    }

    public MetadataCacheDatabase Cache => _cache;

    public async IAsyncEnumerable<MkvFileItem> ScanAsync(
        string folder,
        string mkvMergePath,
        string ffProbePath,
        [EnumeratorCancellation] CancellationToken token,
        IReadOnlyCollection<string>? ignoredFolderNames = null,
        IProgress<(int Completed, int Total)>? progress = null,
        WorkerSettings? workers = null)
    {
        folder = CrossPlatformRuntime.NormalizeUserPath(folder);
        var files = EnumerateMediaFiles(folder, ignoredFolderNames, token).ToArray();
        var totalCount = files.Length;
        var completedCount = 0;
        progress?.Report((0, totalCount));
        var workerCount = GetScanWorkerCount(totalCount, workers);
        var channel = Channel.CreateUnbounded<MkvFileItem>(new UnboundedChannelOptions
        {
            SingleReader = true,
            SingleWriter = false
        });

        var producer = Task.Run(async () =>
        {
            try
            {
                await Parallel.ForEachAsync(files, new ParallelOptions
                {
                    CancellationToken = token,
                    MaxDegreeOfParallelism = workerCount
                }, async (file, ct) =>
                {
                    var item = await ScanFileSafeAsync(file, mkvMergePath, ffProbePath, ct);
                    var completed = Interlocked.Increment(ref completedCount);
                    progress?.Report((completed, totalCount));
                    await channel.Writer.WriteAsync(item, ct);
                });

                channel.Writer.TryComplete();
            }
            catch (OperationCanceledException ex)
            {
                channel.Writer.TryComplete(ex);
            }
            catch (Exception ex)
            {
                channel.Writer.TryComplete(ex);
            }
        }, token);

        await foreach (var item in channel.Reader.ReadAllAsync(token))
        {
            yield return item;
        }

        await producer;
    }

    public async Task<MkvFileItem> ScanFileAsync(string filePath, string mkvMergePath, string ffProbePath, CancellationToken token, bool forceRefresh = false)
    {
        token.ThrowIfCancellationRequested();
        filePath = CrossPlatformRuntime.NormalizeUserPath(filePath);

        if (!forceRefresh)
        {
            var cached = _cache.TryGetValid(filePath);
            if (cached is not null) return cached;
        }

        var item = GetPrimaryMetadataReaderName(filePath) == "ffprobe"
            ? await _ffProbe.IdentifyAsync(ffProbePath, filePath, token)
            : await _mkvMerge.IdentifyAsync(mkvMergePath, filePath, token);

        // mkvmerge is fast and provides the full track layout. ffprobe is only used as a lightweight
        // video fallback for fields mkvmerge often omits, especially bit depth/pixel format details.
        if (CrossPlatformRuntime.IsMkvPath(filePath) && ShouldRunFfProbeFallback(item))
        {
            try
            {
                await _ffProbe.ApplyMediaInfoAsync(ffProbePath, item, token);
            }
            catch (Exception ffEx)
            {
                item.Status = item.Status + $" / ffprobe fallback: {ffEx.Message}";
            }
        }

        if (item.Tracks.Count > 0)
        {
            _cache.Upsert(item);
        }

        return item;
    }

    private static bool ShouldRunFfProbeFallback(MkvFileItem item)
    {
        return IsMissing(item.Codec)
            || IsMissing(item.Resolution)
            || IsMissing(item.BitDepth);
    }

    public static string GetPrimaryMetadataReaderName(string filePath)
        => CrossPlatformRuntime.IsMp4Path(filePath) ? "ffprobe" : "mkvmerge";

    private static bool IsMissing(string? value)
    {
        return string.IsNullOrWhiteSpace(value)
            || value.Equals("Unknown", StringComparison.OrdinalIgnoreCase);
    }

    public async Task<MkvFileItem> ScanFileSafeAsync(string filePath, string mkvMergePath, string ffProbePath, CancellationToken token)
    {
        try
        {
            return await ScanFileAsync(filePath, mkvMergePath, ffProbePath, token);
        }
        catch (Exception ex)
        {
            return new MkvFileItem
            {
                FilePath = filePath,
                Status = "Scan failed: " + ex.Message,
                Codec = "Scan failed",
                Resolution = "Scan failed",
                BitDepth = "Scan failed"
            };
        }
    }

    private static int GetScanWorkerCount(int fileCount, WorkerSettings? workers)
    {
        if (fileCount <= 1) return 1;

        // External media probes are process-heavy and often run against network shares.
        // Use shared worker settings so scan paths honor the same concurrency profile.
        var configured = (workers ?? WorkerSettings.Defaults).CloneNormalized().MaxScanWorkers;
        return Math.Min(fileCount, configured);
    }

    public static IEnumerable<string> EnumerateMediaFiles(
        string rootFolder,
        IReadOnlyCollection<string>? ignoredFolderNames,
        CancellationToken token)
    {
        var ignored = new HashSet<string>(
            (ignoredFolderNames ?? Array.Empty<string>())
                .Select(name => name.Trim())
                .Where(name => !string.IsNullOrWhiteSpace(name)),
            StringComparer.OrdinalIgnoreCase);

        var pending = new Stack<string>();
        pending.Push(CrossPlatformRuntime.NormalizeUserPath(rootFolder));

        while (pending.Count > 0)
        {
            token.ThrowIfCancellationRequested();
            var current = pending.Pop();

            IEnumerable<string> files;
            try
            {
                files = Directory.EnumerateFiles(current, "*.*", SearchOption.TopDirectoryOnly)
                    .Where(CrossPlatformRuntime.IsSupportedMediaPath)
                    .ToArray();
            }
            catch
            {
                continue;
            }

            foreach (var file in files)
            {
                token.ThrowIfCancellationRequested();
                yield return file;
            }

            IEnumerable<string> directories;
            try
            {
                directories = Directory.EnumerateDirectories(current).ToArray();
            }
            catch
            {
                continue;
            }

            foreach (var directory in directories.Reverse())
            {
                token.ThrowIfCancellationRequested();
                var name = Path.GetFileName(directory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar));
                if (ignored.Contains(name))
                {
                    continue;
                }

                pending.Push(directory);
            }
        }
    }
}
