namespace MKVOrchestrator.Core.Models;

/// <summary>
/// Shared worker limits used by desktop workflows.
/// Keep defaults conservative for Unraid/network-share workflows.
/// </summary>
public sealed class WorkerSettings
{
    public int MaxScanWorkers { get; set; } = 4;
    public int MaxEditWorkers { get; set; } = 2;
    public int MaxRemuxWorkers { get; set; } = 1;

    public static WorkerSettings Defaults => new();

    public WorkerSettings Normalize()
    {
        MaxScanWorkers = Clamp(MaxScanWorkers, 1, 8);
        MaxEditWorkers = Clamp(MaxEditWorkers, 1, 6);
        MaxRemuxWorkers = Clamp(MaxRemuxWorkers, 1, 2);
        return this;
    }

    public WorkerSettings CloneNormalized()
    {
        return new WorkerSettings
        {
            MaxScanWorkers = MaxScanWorkers,
            MaxEditWorkers = MaxEditWorkers,
            MaxRemuxWorkers = MaxRemuxWorkers
        }.Normalize();
    }

    private static int Clamp(int value, int minimum, int maximum)
    {
        if (value < minimum) return minimum;
        if (value > maximum) return maximum;
        return value;
    }
}
