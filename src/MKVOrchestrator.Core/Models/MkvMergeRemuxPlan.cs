namespace MKVOrchestrator.Core.Models;

public sealed class MkvMergeRemuxPlan
{
    public List<MkvMergeRemuxAction> Actions { get; } = new();
    public List<string> NoChangeFiles { get; } = new();
}

public sealed class MkvMergeRemuxAction
{
    public string SourceFilePath { get; set; } = string.Empty;
    public string TempOutputPath { get; set; } = string.Empty;
    public string FinalOutputPath { get; set; } = string.Empty; // convert-mkv: the new .mkv path next to the source
    public string Description { get; set; } = string.Empty;
    public string ToolName { get; set; } = "mkvmerge";
    public string Operation { get; set; } = "remux";
    public string? ExternalSubtitleFilePath { get; set; }
    public List<string> ExternalSubtitleFilePaths { get; set; } = new();
    public bool DeleteExternalSubtitleAfterSuccess { get; set; }
    public bool DeleteSourceAfterSuccess { get; set; } // convert-mkv: remove the original MP4 once the MKV exists
    public List<string> Arguments { get; set; } = new();
}
