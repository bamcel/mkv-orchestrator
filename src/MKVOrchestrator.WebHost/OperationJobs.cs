using System.Collections.Concurrent;

public sealed record OperationJobResponse(
    string Id,
    string Kind,
    string Status,
    DateTimeOffset CreatedUtc,
    DateTimeOffset? StartedUtc,
    DateTimeOffset? CompletedUtc,
    int Completed,
    int Failed,
    int Skipped,
    int Total,
    string CurrentFile,
    int CurrentFilePercent,
    IReadOnlyList<string> Lines,
    MuxPreviewResponse? MuxResult,
    PropEditPreviewResponse? PropEditResult,
    string Error);

public sealed class OperationJobStore
{
    private readonly ConcurrentDictionary<string, OperationJobState> _jobs = new();

    public OperationJobState Create(string kind, int total)
    {
        var job = new OperationJobState(Guid.NewGuid().ToString("N"), kind, total);
        _jobs[job.Id] = job;
        PruneCompletedJobs();
        return job;
    }

    public bool TryGet(string id, out OperationJobState job) => _jobs.TryGetValue(id, out job!);

    private void PruneCompletedJobs()
    {
        var cutoff = DateTimeOffset.UtcNow.AddMinutes(-60);
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

public sealed class OperationJobState
{
    private readonly object _sync = new();
    private readonly CancellationTokenSource _cts = new();
    private readonly List<string> _lines = new();

    public OperationJobState(string id, string kind, int total)
    {
        Id = id;
        Kind = kind;
        Total = total;
        CreatedUtc = DateTimeOffset.UtcNow;
    }

    public string Id { get; }
    public string Kind { get; }
    public DateTimeOffset CreatedUtc { get; }
    public DateTimeOffset? StartedUtc { get; private set; }
    public DateTimeOffset? CompletedUtc { get; private set; }
    public string Status { get; private set; } = "Queued";
    public int Completed { get; private set; }
    public int Failed { get; private set; }
    public int Skipped { get; private set; }
    public int Total { get; private set; }
    public string CurrentFile { get; private set; } = string.Empty;
    public int CurrentFilePercent { get; private set; }
    public MuxPreviewResponse? MuxResult { get; private set; }
    public PropEditPreviewResponse? PropEditResult { get; private set; }
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

    public void AddLine(string line)
    {
        lock (_sync)
        {
            _lines.Add(line);
        }
    }

    public void SetCurrentFile(string fileName)
    {
        lock (_sync)
        {
            CurrentFile = fileName;
            CurrentFilePercent = 0;
        }
    }

    public void SetCurrentFilePercent(int percent)
    {
        lock (_sync)
        {
            CurrentFilePercent = Math.Clamp(percent, 0, 100);
        }
    }

    public void RecordCompleted() { lock (_sync) { Completed++; } }
    public void RecordFailed() { lock (_sync) { Failed++; } }
    public void RecordSkipped() { lock (_sync) { Skipped++; } }

    public void MarkCompleted(MuxPreviewResponse? muxResult, PropEditPreviewResponse? propEditResult)
    {
        lock (_sync)
        {
            MuxResult = muxResult;
            PropEditResult = propEditResult;
            CurrentFile = string.Empty;
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

    public void MarkCanceled(MuxPreviewResponse? muxResult, PropEditPreviewResponse? propEditResult)
    {
        lock (_sync)
        {
            MuxResult = muxResult;
            PropEditResult = propEditResult;
            CurrentFile = string.Empty;
            Status = "Canceled";
            CompletedUtc = DateTimeOffset.UtcNow;
        }
    }

    public void MarkFailed(string error)
    {
        lock (_sync)
        {
            Error = error;
            CurrentFile = string.Empty;
            Status = "Failed";
            CompletedUtc = DateTimeOffset.UtcNow;
        }
    }

    public OperationJobResponse ToResponse()
    {
        lock (_sync)
        {
            return new OperationJobResponse(
                Id,
                Kind,
                Status,
                CreatedUtc,
                StartedUtc,
                CompletedUtc,
                Completed,
                Failed,
                Skipped,
                Total,
                CurrentFile,
                CurrentFilePercent,
                _lines.ToArray(),
                MuxResult,
                PropEditResult,
                Error);
        }
    }
}
