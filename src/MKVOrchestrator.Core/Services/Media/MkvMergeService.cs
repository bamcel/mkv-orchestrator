using System.Text.RegularExpressions;
using System.Text.Json;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.Core.Services;

public sealed class MkvMergeService
{
    private readonly ProcessRunner _runner = new();

    public async Task<MkvFileItem> IdentifyAsync(string mkvMergePath, string filePath, CancellationToken token)
    {
        ValidateMkvMergePath(mkvMergePath);
        mkvMergePath = CrossPlatformRuntime.ResolveExecutable(
            mkvMergePath,
            "mkvmerge.exe",
            "mkvmerge",
            @"C:\Program Files\MKVToolNix\mkvmerge.exe",
            @"C:\Program Files (x86)\MKVToolNix\mkvmerge.exe",
            "/usr/bin/mkvmerge",
            "/usr/local/bin/mkvmerge",
            "/opt/homebrew/bin/mkvmerge");

        // First try the common short form. If an older or unusual MKVToolNix build rejects it,
        // fall back to the long identify syntax before reporting failure.
        var processFilePath = CrossPlatformRuntime.ToProcessArgumentPath(filePath);
        var result = await _runner.RunAsync(mkvMergePath, new[] { "-J", processFilePath }, token);
        if (result.ExitCode != 0 && LooksLikeUnknownMode(result))
        {
            result = await _runner.RunAsync(mkvMergePath, new[] { "--identification-format", "json", "--identify", processFilePath }, token);
        }

        var identifyWarning = string.Empty;
        if (result.ExitCode != 0 && !string.IsNullOrWhiteSpace(result.StandardOutput))
        {
            identifyWarning = string.IsNullOrWhiteSpace(result.StandardError)
                ? $"mkvmerge identify returned warning exit code {result.ExitCode}."
                : result.StandardError.Trim();
        }

        if (result.ExitCode != 0 && string.IsNullOrWhiteSpace(result.StandardOutput))
        {
            var error = string.IsNullOrWhiteSpace(result.StandardError) ? result.StandardOutput : result.StandardError;
            var detail = string.IsNullOrWhiteSpace(error) ? $"mkvmerge exited with code {result.ExitCode}." : error.Trim();
            throw new InvalidOperationException($"mkvmerge identify failed. Command tried: {DescribeCommand(mkvMergePath, filePath)}. Error: {detail}");
        }

        if (string.IsNullOrWhiteSpace(result.StandardOutput))
            throw new InvalidOperationException($"mkvmerge returned no JSON output. Command tried: {DescribeCommand(mkvMergePath, filePath)}");

        using var doc = ParseJson(result.StandardOutput);
        var root = doc.RootElement;
        var item = new MkvFileItem { FilePath = filePath };

        if (root.TryGetProperty("container", out var container) &&
            container.TryGetProperty("properties", out var cprops) &&
            cprops.TryGetProperty("title", out var title))
            item.ContainerTitle = title.GetString() ?? string.Empty;

        if (!root.TryGetProperty("tracks", out var tracks) || tracks.ValueKind != JsonValueKind.Array)
        {
            item.Status = "Scanned - no tracks found";
            return item;
        }

        var propEditNumber = 1;
        foreach (var track in tracks.EnumerateArray())
        {
            if (!track.TryGetProperty("properties", out var props))
                continue;

            var type = track.TryGetProperty("type", out var typeElement)
                ? typeElement.GetString() ?? string.Empty
                : string.Empty;

            var codec = track.TryGetProperty("codec", out var codecElement)
                ? codecElement.GetString() ?? string.Empty
                : string.Empty;

            var t = new MkvTrackItem
            {
                MkvMergeId = track.TryGetProperty("id", out var idElement) ? idElement.GetInt32() : 0,
                PropEditTrackNumber = propEditNumber++,
                Type = type,
                Codec = codec,
                Language = GetString(props, "language") ?? GetString(props, "language_ietf") ?? "und",
                Name = GetString(props, "track_name") ?? string.Empty,
                Default = GetBool(props, "default_track"),
                Forced = GetBool(props, "forced_track")
            };
            item.Tracks.Add(t);

            if (type.Equals("video", StringComparison.OrdinalIgnoreCase))
            {
                t.Codec = CodecDisplayNormalizer.Normalize(t.Codec);
                t.Resolution = GetResolution(props);
                t.BitDepth = GetBitDepth(props, t.Codec, filePath);
                item.Codec = CodecDisplayNormalizer.Normalize(t.Codec);
                item.Resolution = CodecDisplayNormalizer.DisplayValue(t.Resolution);
                item.BitDepth = CodecDisplayNormalizer.DisplayValue(t.BitDepth);
                item.VideoSummary = CodecDisplayNormalizer.BuildVideoSummary(item.Codec, item.Resolution, item.BitDepth);
            }
        }

        if (root.TryGetProperty("attachments", out var attachments) && attachments.ValueKind == JsonValueKind.Array)
        {
            foreach (var attachment in attachments.EnumerateArray())
            {
                JsonElement attachmentProps = default;
                var hasProps = attachment.TryGetProperty("properties", out attachmentProps);

                item.Attachments.Add(new MkvAttachmentItem
                {
                    Id = attachment.TryGetProperty("id", out var attachmentId) && attachmentId.ValueKind == JsonValueKind.Number
                        ? attachmentId.GetInt32()
                        : item.Attachments.Count + 1,
                    FileName = GetString(attachment, "file_name") ?? GetString(attachment, "name") ?? string.Empty,
                    ContentType = GetString(attachment, "content_type") ?? GetString(attachment, "mime_type") ?? string.Empty,
                    Description = GetString(attachment, "description") ?? string.Empty,
                    SizeBytes = (hasProps ? GetLong(attachmentProps, "size") : null) ?? GetLong(attachment, "size")
                });
            }
        }

        item.AudioSummary = CodecDisplayNormalizer.BuildLanguageTrackSummary(item.Tracks.Where(t => t.Type.Equals("audio", StringComparison.OrdinalIgnoreCase)));
        item.SubtitleSummary = CodecDisplayNormalizer.BuildLanguageTrackSummary(item.Tracks.Where(t => t.Type.Equals("subtitles", StringComparison.OrdinalIgnoreCase)));
        item.AttachmentSummary = BuildAttachmentSummary(item.Attachments);
        item.Status = item.Tracks.Any(t => t.Type.Equals("video", StringComparison.OrdinalIgnoreCase))
            ? "Scanned"
            : "Scanned - no video track";
        if (!string.IsNullOrWhiteSpace(identifyWarning))
        {
            item.Status += " / mkvmerge warning";
        }
        return item;
    }

    private static void ValidateMkvMergePath(string mkvMergePath)
    {
        if (string.IsNullOrWhiteSpace(mkvMergePath))
            throw new InvalidOperationException("mkvmerge path is blank. Set it in Settings.");

        var exeName = Path.GetFileName(mkvMergePath.Trim().Trim('"'));
        if (string.IsNullOrWhiteSpace(exeName)) return;

        // Allow plain PATH usage: mkvmerge or mkvmerge.exe.
        if (exeName.Equals("mkvmerge", StringComparison.OrdinalIgnoreCase) ||
            exeName.Equals("mkvmerge.exe", StringComparison.OrdinalIgnoreCase))
            return;

        if (exeName.Contains("mkvtoolnix", StringComparison.OrdinalIgnoreCase) ||
            exeName.Equals("mkvpropedit.exe", StringComparison.OrdinalIgnoreCase) ||
            exeName.Equals("mkvextract.exe", StringComparison.OrdinalIgnoreCase) ||
            exeName.Equals("mkvinfo.exe", StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"The configured mkvmerge tool points to '{exeName}'. Select the MKVToolNix folder in Settings or ensure mkvmerge is available on PATH.");
        }
    }

    private static bool LooksLikeUnknownMode(ProcessResult result)
    {
        var text = ((result.StandardError ?? string.Empty) + " " + (result.StandardOutput ?? string.Empty)).ToLowerInvariant();
        return text.Contains("unknown mode") || text.Contains("unknown option") || text.Contains("invalid option");
    }

    private static string DescribeCommand(string mkvMergePath, string filePath)
        => $"\"{mkvMergePath}\" -J \"{filePath}\"";

    private static JsonDocument ParseJson(string json)
    {
        try
        {
            return JsonDocument.Parse(json);
        }
        catch (JsonException ex)
        {
            var sample = json.Length > 240 ? json[..240] : json;
            throw new InvalidOperationException($"Could not parse mkvmerge JSON output. {ex.Message}. Output starts with: {sample}", ex);
        }
    }

    private static string BuildAttachmentSummary(IEnumerable<MkvAttachmentItem> attachments)
    {
        var items = attachments.ToList();
        if (items.Count == 0) return "None";

        var fontCount = items.Count(a => IsFontAttachment(a));
        var otherCount = items.Count - fontCount;
        var parts = new List<string>();
        if (fontCount > 0) parts.Add($"Fonts x{fontCount}");
        if (otherCount > 0) parts.Add($"Other x{otherCount}");
        return string.Join(", ", parts);
    }

    private static bool IsFontAttachment(MkvAttachmentItem attachment)
    {
        var text = $"{attachment.FileName} {attachment.ContentType}";
        return text.Contains("font", StringComparison.OrdinalIgnoreCase)
            || text.EndsWith(".ttf", StringComparison.OrdinalIgnoreCase)
            || text.EndsWith(".otf", StringComparison.OrdinalIgnoreCase)
            || text.EndsWith(".ttc", StringComparison.OrdinalIgnoreCase);
    }

    private static string? GetString(JsonElement props, string name)
        => props.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    private static bool GetBool(JsonElement props, string name)
        => props.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;

    private static int? GetInt(JsonElement props, string name)
    {
        if (!props.TryGetProperty(name, out var v)) return null;
        if (v.ValueKind == JsonValueKind.Number && v.TryGetInt32(out var n)) return n;
        if (v.ValueKind == JsonValueKind.String && int.TryParse(v.GetString(), out var s)) return s;
        return null;
    }

    private static long? GetLong(JsonElement props, string name)
    {
        if (!props.TryGetProperty(name, out var v)) return null;
        if (v.ValueKind == JsonValueKind.Number && v.TryGetInt64(out var n)) return n;
        if (v.ValueKind == JsonValueKind.String && long.TryParse(v.GetString(), out var s)) return s;
        return null;
    }

    private static string GetResolution(JsonElement props)
    {
        var direct = GetString(props, "pixel_dimensions")
                  ?? GetString(props, "display_dimensions")
                  ?? GetString(props, "display_unit");

        if (!string.IsNullOrWhiteSpace(direct))
        {
            var match = Regex.Match(direct, @"(?<w>\d{3,5})\s*[xX]\s*(?<h>\d{3,5})");
            if (match.Success) return $"{match.Groups["w"].Value}x{match.Groups["h"].Value}";
        }

        var w = GetInt(props, "pixel_width")
             ?? GetInt(props, "display_width")
             ?? GetInt(props, "video_pixel_width");

        var h = GetInt(props, "pixel_height")
             ?? GetInt(props, "display_height")
             ?? GetInt(props, "video_pixel_height");

        return w.HasValue && h.HasValue ? $"{w}x{h}" : string.Empty;
    }

    private static string GetBitDepth(JsonElement props, string codec, string filePath)
    {
        var bits = GetInt(props, "bits_per_channel")
                ?? GetInt(props, "color_bits_per_channel")
                ?? GetInt(props, "video_color_bits_per_channel")
                ?? GetInt(props, "bit_depth");

        if (bits.HasValue) return $"{bits}bit";

        var combined = (codec + " " + Path.GetFileNameWithoutExtension(filePath)).ToLowerInvariant();
        if (Regex.IsMatch(combined, @"\b(10bit|10-bit|10 bit|hi10p|main\s*10)\b")) return "10bit";
        if (Regex.IsMatch(combined, @"\b(12bit|12-bit|12 bit)\b")) return "12bit";
        if (Regex.IsMatch(combined, @"\b(8bit|8-bit|8 bit)\b")) return "8bit";

        return string.Empty;
    }



    public MkvMergeRemuxPlan BuildRemuxPlan(
        IEnumerable<MkvFileItem> files,
        string keepAudioLanguages,
        string keepSubtitleLanguages,
        bool removeUnwantedAudioLanguages,
        bool removeUnwantedSubtitleLanguages,
        bool removeUnwantedTrackIds,
        string removeTrackIdsText,
        bool preserveChapters,
        bool preserveAttachments,
        bool useSafeTempReplacement,
        bool muxMatchingExternalSubtitles,
        string externalSubtitleLanguage,
        string externalSubtitleTrackName,
        string externalSubtitleFormats,
        bool preserveExternalSubtitleFiles,
        bool skipMuxIfSubtitleAlreadyExists,
        bool extractSubtitles,
        string extractSubtitleLanguages,
        bool extractOverwriteExistingFiles)
    {
        var plan = new MkvMergeRemuxPlan();
        var selectedFiles = files.Where(f => f.Selected).ToList();
        var keepAudio = ParseLanguageSet(keepAudioLanguages);
        var keepSubtitle = ParseLanguageSet(keepSubtitleLanguages);
        var extractLanguages = ParseLanguageSet(extractSubtitleLanguages);
        var parsedRemoveTrackIds = removeUnwantedTrackIds ? ParseTrackIdSet(removeTrackIdsText) : new HashSet<int>();

        foreach (var file in selectedFiles)
        {
            if (!CrossPlatformRuntime.IsMkvPath(file.FilePath))
            {
                plan.NoChangeFiles.Add(file.FilePath);
                continue;
            }

            var addedForFile = false;

            var action = BuildRemuxAction(
                file,
                keepAudio,
                keepSubtitle,
                removeUnwantedAudioLanguages,
                removeUnwantedSubtitleLanguages,
                removeUnwantedTrackIds,
                parsedRemoveTrackIds,
                preserveChapters,
                preserveAttachments,
                useSafeTempReplacement);

            if (action is not null)
            {
                plan.Actions.Add(action);
                addedForFile = true;
            }

            if (muxMatchingExternalSubtitles)
            {
                var muxAction = BuildMuxMatchingExternalSubtitleAction(
                    file,
                    externalSubtitleLanguage,
                    externalSubtitleTrackName,
                    externalSubtitleFormats,
                    preserveExternalSubtitleFiles,
                    skipMuxIfSubtitleAlreadyExists,
                    useSafeTempReplacement);

                if (muxAction is not null)
                {
                    plan.Actions.Add(muxAction);
                    addedForFile = true;
                }
            }

            if (extractSubtitles)
            {
                foreach (var extractAction in BuildExtractSubtitleActions(file, extractLanguages, extractOverwriteExistingFiles))
                {
                    plan.Actions.Add(extractAction);
                    addedForFile = true;
                }
            }

            if (!addedForFile)
            {
                plan.NoChangeFiles.Add(file.FilePath);
            }
        }

        return plan;
    }

    public async Task<ProcessResult> ExecuteRemuxAsync(string mkvMergePath, MkvMergeRemuxAction action, CancellationToken token)
        => await ExecuteRemuxAsync(mkvMergePath, action, null, token);

    public async Task<ProcessResult> ExecuteRemuxAsync(
        string mkvMergePath,
        MkvMergeRemuxAction action,
        Action<int>? onProgressPercent,
        CancellationToken token)
    {
        if (string.Equals(action.Operation, "extract-subtitle", StringComparison.OrdinalIgnoreCase))
        {
            return await ExecuteSubtitleExtractAsync(mkvMergePath, action, onProgressPercent, token);
        }

        ValidateMkvMergePath(mkvMergePath);
        mkvMergePath = ResolveMkvMergeExecutable(mkvMergePath);

        var tempPath = action.TempOutputPath;
        if (File.Exists(tempPath))
        {
            File.Delete(tempPath);
        }

        var lastProgress = -1;
        var result = await _runner.RunWithOutputCallbackAsync(
            mkvMergePath,
            CrossPlatformRuntime.ConvertExistingPathArgumentsForProcess(action.Arguments),
            line =>
            {
                var percent = TryParseMkvMergeProgress(line);
                if (percent is null || percent == lastProgress) return;
                lastProgress = percent.Value;
                onProgressPercent?.Invoke(percent.Value);
            },
            token);

        if (result.ExitCode != 0)
        {
            TryDeleteTempFile(tempPath);
            return result;
        }

        if (!File.Exists(tempPath))
        {
            return new ProcessResult(-1, result.StandardOutput, "mkvmerge completed but the temp output file was not created.");
        }

        ReplaceOriginalWithTemp(action.SourceFilePath, tempPath);
        if (action.DeleteExternalSubtitleAfterSuccess)
        {
            foreach (var externalSubtitlePath in GetExternalSubtitlePaths(action))
            {
                TryDeleteTempFile(externalSubtitlePath);
            }
        }

        onProgressPercent?.Invoke(100);
        return result;
    }

    private async Task<ProcessResult> ExecuteSubtitleExtractAsync(
        string mkvMergePath,
        MkvMergeRemuxAction action,
        Action<int>? onProgressPercent,
        CancellationToken token)
    {
        var mkvExtractPath = ResolveSiblingTool(mkvMergePath, "mkvextract.exe", "mkvextract");
        var outputPath = action.TempOutputPath;

        if (File.Exists(outputPath))
        {
            File.Delete(outputPath);
        }

        var result = await _runner.RunAsync(mkvExtractPath, CrossPlatformRuntime.ConvertExistingPathArgumentsForProcess(action.Arguments), token);
        if (result.ExitCode == 0)
        {
            onProgressPercent?.Invoke(100);
        }

        return result;
    }

    private static string ResolveMkvMergeExecutable(string mkvMergePath)
    {
        return CrossPlatformRuntime.ResolveExecutable(
            mkvMergePath,
            "mkvmerge.exe",
            "mkvmerge",
            @"C:\Program Files\MKVToolNix\mkvmerge.exe",
            @"C:\Program Files (x86)\MKVToolNix\mkvmerge.exe",
            "/usr/bin/mkvmerge",
            "/usr/local/bin/mkvmerge",
            "/opt/homebrew/bin/mkvmerge");
    }

    private static string ResolveSiblingTool(string configuredMkvMergePath, string windowsName, string unixName)
    {
        var configured = CrossPlatformRuntime.NormalizeUserPath(configuredMkvMergePath);
        if (!string.IsNullOrWhiteSpace(configured) &&
            (Path.IsPathFullyQualified(configured) || configured.Contains(Path.DirectorySeparatorChar) || configured.Contains(Path.AltDirectorySeparatorChar)))
        {
            var folder = Path.GetDirectoryName(configured);
            if (!string.IsNullOrWhiteSpace(folder))
            {
                var sibling = Path.Combine(folder, CrossPlatformRuntime.IsWindows ? windowsName : unixName);
                if (File.Exists(sibling))
                {
                    return sibling;
                }
            }
        }

        return CrossPlatformRuntime.ResolveExecutable(
            CrossPlatformRuntime.GetToolDisplayName(windowsName, unixName),
            windowsName,
            unixName,
            @"C:\Program Files\MKVToolNix\" + windowsName,
            @"C:\Program Files (x86)\MKVToolNix\" + windowsName,
            "/usr/bin/" + unixName,
            "/usr/local/bin/" + unixName,
            "/opt/homebrew/bin/" + unixName);
    }


    private static int? TryParseMkvMergeProgress(string line)
    {
        if (string.IsNullOrWhiteSpace(line)) return null;

        // mkvmerge usually emits progress as: "Progress: 97%".
        var match = Regex.Match(line, @"Progress:\s*(?<percent>\d{1,3})\s*%", RegexOptions.IgnoreCase);
        if (!match.Success)
        {
            match = Regex.Match(line, @"(?<percent>\d{1,3})\s*%", RegexOptions.IgnoreCase);
        }

        if (!match.Success || !int.TryParse(match.Groups["percent"].Value, out var percent))
            return null;

        return Math.Clamp(percent, 0, 100);
    }

    private static MkvMergeRemuxAction? BuildMuxMatchingExternalSubtitleAction(
        MkvFileItem file,
        string language,
        string trackName,
        string formats,
        bool preserveExternalSubtitleFile,
        bool skipIfSubtitleAlreadyExists,
        bool useSafeTempReplacement)
    {
        var normalizedLanguage = string.IsNullOrWhiteSpace(language) ? "eng" : language.Trim().ToLowerInvariant();
        var subtitleInputs = FindMatchingExternalSubtitles(file.FilePath, formats)
            .Select(path => BuildExternalSubtitleInput(path, file.FilePath, normalizedLanguage, trackName))
            .Where(input => !skipIfSubtitleAlreadyExists || !HasMatchingSubtitleTrack(file, input.Language, input.TrackName))
            .ToList();

        if (subtitleInputs.Count == 0)
        {
            return null;
        }

        var subtitlePaths = subtitleInputs.Select(input => input.Path).ToList();

        var tempPath = BuildTempOutputPath(file.FilePath);
        var args = new List<string>
        {
            "-o",
            tempPath,
            file.FilePath
        };

        foreach (var input in subtitleInputs)
        {
            args.Add("--language");
            args.Add("0:" + input.Language);

            if (!string.IsNullOrWhiteSpace(input.TrackName))
            {
                args.Add("--track-name");
                args.Add("0:" + input.TrackName);
            }

            args.Add(input.Path);
        }

        return new MkvMergeRemuxAction
        {
            SourceFilePath = file.FilePath,
            TempOutputPath = tempPath,
            UseSafeTempReplacement = useSafeTempReplacement,
            Description = BuildExternalSubtitleDescription(subtitleInputs),
            ToolName = "mkvmerge",
            Operation = "mux-external-subtitle",
            ExternalSubtitleFilePath = subtitlePaths[0],
            ExternalSubtitleFilePaths = subtitlePaths,
            DeleteExternalSubtitleAfterSuccess = !preserveExternalSubtitleFile,
            Arguments = args
        };
    }

    private static IEnumerable<MkvMergeRemuxAction> BuildExtractSubtitleActions(
        MkvFileItem file,
        ISet<string> extractLanguages,
        bool overwriteExistingFiles)
    {
        var subtitleTracks = file.Tracks
            .Where(t => IsSubtitleTrack(t.Type))
            .Where(t => extractLanguages.Count == 0 || extractLanguages.Contains("all") || LanguageMatches(t.Language, extractLanguages))
            .OrderBy(t => t.MkvMergeId)
            .ToList();

        foreach (var track in subtitleTracks)
        {
            var outputPath = BuildSubtitleExtractOutputPath(file.FilePath, track);
            if (File.Exists(outputPath) && !overwriteExistingFiles)
            {
                continue;
            }

            yield return new MkvMergeRemuxAction
            {
                SourceFilePath = file.FilePath,
                TempOutputPath = outputPath,
                UseSafeTempReplacement = false,
                Description = $"Extract subtitle #{track.MkvMergeId}: {FormatTrackLabel(track)} -> {Path.GetFileName(outputPath)}",
                ToolName = "mkvextract",
                Operation = "extract-subtitle",
                Arguments = new List<string>
                {
                    "tracks",
                    file.FilePath,
                    $"{track.MkvMergeId}:{outputPath}"
                }
            };
        }
    }

    private sealed record ExternalSubtitleInput(string Path, string Language, string TrackName);

    private static IReadOnlyList<string> GetExternalSubtitlePaths(MkvMergeRemuxAction action)
    {
        if (action.ExternalSubtitleFilePaths.Count > 0)
        {
            return action.ExternalSubtitleFilePaths;
        }

        return string.IsNullOrWhiteSpace(action.ExternalSubtitleFilePath)
            ? Array.Empty<string>()
            : new[] { action.ExternalSubtitleFilePath };
    }

    private static string BuildExternalSubtitleDescription(IReadOnlyList<ExternalSubtitleInput> subtitleInputs)
    {
        if (subtitleInputs.Count == 1)
        {
            var input = subtitleInputs[0];
            return $"Mux external subtitle: {Path.GetFileName(input.Path)} as {input.Language}" +
                   (string.IsNullOrWhiteSpace(input.TrackName) ? string.Empty : $" / \"{input.TrackName}\"");
        }

        var parts = subtitleInputs
            .Select(input => $"{Path.GetFileName(input.Path)} as {input.Language}" +
                             (string.IsNullOrWhiteSpace(input.TrackName) ? string.Empty : $" / \"{input.TrackName}\""));

        return $"Mux {subtitleInputs.Count} external subtitles: " + string.Join("; ", parts);
    }

    private static bool HasMatchingSubtitleTrack(MkvFileItem file, string language, string trackName)
    {
        return file.Tracks.Any(t =>
            IsSubtitleTrack(t.Type) &&
            LanguageMatches(t.Language, new HashSet<string>(StringComparer.OrdinalIgnoreCase) { language }) &&
            (string.IsNullOrWhiteSpace(trackName) || string.Equals(t.Name?.Trim(), trackName.Trim(), StringComparison.OrdinalIgnoreCase)));
    }

    private static ExternalSubtitleInput BuildExternalSubtitleInput(string subtitlePath, string mkvPath, string fallbackLanguage, string configuredTrackName)
    {
        var parts = ParseExternalSubtitleParts(subtitlePath, mkvPath, fallbackLanguage);
        return new ExternalSubtitleInput(
            subtitlePath,
            parts.Language,
            BuildExternalSubtitleTrackName(configuredTrackName, parts.Language, parts.Tag));
    }

    private static string BuildExternalSubtitleTrackName(string configuredTrackName, string language, string tag)
    {
        if (string.IsNullOrWhiteSpace(configuredTrackName))
        {
            return FormatSubtitleTagAsTrackName(tag);
        }

        return configuredTrackName
            .Trim()
            .Replace("{language}", language, StringComparison.OrdinalIgnoreCase)
            .Replace("{tag}", FormatSubtitleTagAsTrackName(tag), StringComparison.OrdinalIgnoreCase)
            .Replace("  ", " ", StringComparison.Ordinal)
            .Trim();
    }

    private static (string Language, string Tag) ParseExternalSubtitleParts(string subtitlePath, string mkvPath, string fallbackLanguage)
    {
        var suffix = GetExternalSubtitleSuffix(subtitlePath, mkvPath);
        if (string.IsNullOrWhiteSpace(suffix))
        {
            return (fallbackLanguage, string.Empty);
        }

        var segments = suffix
            .Split('.', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .ToList();

        if (segments.Count == 0)
        {
            return (fallbackLanguage, string.Empty);
        }

        if (IsLanguageToken(segments[0]))
        {
            return (segments[0].ToLowerInvariant(), string.Join(".", segments.Skip(1)));
        }

        return (fallbackLanguage, suffix);
    }

    private static bool IsLanguageToken(string value)
    {
        return Regex.IsMatch(value ?? string.Empty, "^[a-zA-Z]{2,3}$", RegexOptions.CultureInvariant);
    }

    private static string GetExternalSubtitleSuffix(string subtitlePath, string mkvPath)
    {
        var baseName = Path.GetFileNameWithoutExtension(mkvPath);
        var subtitleName = Path.GetFileNameWithoutExtension(subtitlePath);
        if (subtitleName.Equals(baseName, StringComparison.OrdinalIgnoreCase))
        {
            return string.Empty;
        }

        var prefix = baseName + ".";
        return subtitleName.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
            ? subtitleName[prefix.Length..]
            : string.Empty;
    }

    private static string FormatSubtitleTagAsTrackName(string tag)
    {
        if (string.IsNullOrWhiteSpace(tag))
        {
            return string.Empty;
        }

        var words = Regex.Split(tag, @"[\s._-]+")
            .Where(word => !string.IsNullOrWhiteSpace(word))
            .Select(word => char.ToUpperInvariant(word[0]) + (word.Length > 1 ? word[1..].ToLowerInvariant() : string.Empty));

        return string.Join(" ", words);
    }

    private static IReadOnlyList<string> FindMatchingExternalSubtitles(string mkvPath, string formats)
    {
        var folder = Path.GetDirectoryName(mkvPath);
        if (string.IsNullOrWhiteSpace(folder) || !Directory.Exists(folder))
        {
            return Array.Empty<string>();
        }

        var baseName = Path.GetFileNameWithoutExtension(mkvPath);
        var extensions = ParseSubtitleExtensions(formats);
        var extensionOrder = extensions
            .Select((extension, index) => new { Extension = extension, Index = index })
            .ToDictionary(item => item.Extension, item => item.Index, StringComparer.OrdinalIgnoreCase);

        return Directory.EnumerateFiles(folder)
            .Where(path => IsMatchingExternalSubtitlePath(path, baseName, extensions))
            .OrderBy(path => GetExternalSubtitleSuffix(path, mkvPath).Length == 0 ? 0 : 1)
            .ThenBy(path => ParseExternalSubtitleParts(path, mkvPath, string.Empty).Language, StringComparer.OrdinalIgnoreCase)
            .ThenBy(path => ParseExternalSubtitleParts(path, mkvPath, string.Empty).Tag, StringComparer.OrdinalIgnoreCase)
            .ThenBy(path => extensionOrder.GetValueOrDefault(Path.GetExtension(path), int.MaxValue))
            .ThenBy(Path.GetFileName, StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private static bool IsMatchingExternalSubtitlePath(string path, string baseName, IReadOnlyCollection<string> extensions)
    {
        var extension = Path.GetExtension(path);
        if (!extensions.Contains(extension, StringComparer.OrdinalIgnoreCase))
        {
            return false;
        }

        var subtitleName = Path.GetFileNameWithoutExtension(path);
        return subtitleName.Equals(baseName, StringComparison.OrdinalIgnoreCase) ||
               subtitleName.StartsWith(baseName + ".", StringComparison.OrdinalIgnoreCase);
    }

    private static List<string> ParseSubtitleExtensions(string formats)
    {
        var parsed = formats
            .Split(new[] { ',', ';', ' ', '\t', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(x => x.StartsWith('.') ? x.ToLowerInvariant() : "." + x.ToLowerInvariant())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToList();

        return parsed.Count == 0
            ? new List<string> { ".srt", ".ass", ".ssa", ".sub", ".idx" }
            : parsed;
    }

    private static string BuildSubtitleExtractOutputPath(string mkvPath, MkvTrackItem track)
    {
        var folder = Path.GetDirectoryName(mkvPath) ?? string.Empty;
        var baseName = Path.GetFileNameWithoutExtension(mkvPath);
        var language = string.IsNullOrWhiteSpace(track.Language) ? "und" : track.Language.Trim().ToLowerInvariant();
        var extension = GetSubtitleExtension(track);
        return Path.Combine(folder, $"{baseName}.{language}.track{track.MkvMergeId}{extension}");
    }

    private static string GetSubtitleExtension(MkvTrackItem track)
    {
        var text = $"{track.Codec} {track.Name}".ToLowerInvariant();
        if (text.Contains("ass")) return ".ass";
        if (text.Contains("ssa")) return ".ssa";
        if (text.Contains("vobsub") || text.Contains("dvd")) return ".sub";
        return ".srt";
    }

    private static bool IsSubtitleTrack(string? type)
    {
        return string.Equals(type, "subtitles", StringComparison.OrdinalIgnoreCase)
            || string.Equals(type, "subtitle", StringComparison.OrdinalIgnoreCase);
    }

    private static MkvMergeRemuxAction? BuildRemuxAction(
        MkvFileItem file,
        ISet<string> keepAudio,
        ISet<string> keepSubtitle,
        bool removeUnwantedAudioLanguages,
        bool removeUnwantedSubtitleLanguages,
        bool removeUnwantedTrackIds,
        ISet<int> removeTrackIds,
        bool preserveChapters,
        bool preserveAttachments,
        bool useSafeTempReplacement)
    {
        var descriptions = new List<string>();
        var args = new List<string>();
        var tempPath = BuildTempOutputPath(file.FilePath);

        args.AddRange(new[] { "-o", tempPath });

        if (!preserveChapters)
        {
            args.Add("--no-chapters");
            descriptions.Add("Remove chapters");
        }

        if (!preserveAttachments)
        {
            args.Add("--no-attachments");
            descriptions.Add("Remove attachments/fonts");
        }

        var removedById = removeUnwantedTrackIds && removeTrackIds.Count > 0
            ? file.Tracks.Where(t => removeTrackIds.Contains(t.MkvMergeId)).OrderBy(t => t.MkvMergeId).ToList()
            : new List<MkvTrackItem>();

        ApplyTrackSelection(
            file,
            args,
            descriptions,
            type: "video",
            keepOption: "--video-tracks",
            noneOption: "--no-video",
            removeByLanguage: false,
            keepLanguages: new HashSet<string>(),
            removedById: removedById);

        ApplyTrackSelection(
            file,
            args,
            descriptions,
            type: "audio",
            keepOption: "--audio-tracks",
            noneOption: "--no-audio",
            removeByLanguage: removeUnwantedAudioLanguages,
            keepLanguages: keepAudio,
            removedById: removedById);

        ApplyTrackSelection(
            file,
            args,
            descriptions,
            type: "subtitles",
            keepOption: "--subtitle-tracks",
            noneOption: "--no-subtitles",
            removeByLanguage: removeUnwantedSubtitleLanguages,
            keepLanguages: keepSubtitle,
            removedById: removedById);

        if (removedById.Count > 0)
        {
            var removed = removedById.Select(t => $"#{MkvTrackSelector.GetMkvMergeTrackId(t)} {MkvTrackSelector.NormalizeTrackType(t.Type)} {FormatTrackLabel(t)}");
            descriptions.Add("Remove track IDs: " + string.Join(", ", removed));
        }

        args.Add(file.FilePath);

        if (descriptions.Count == 0)
        {
            return null;
        }

        return new MkvMergeRemuxAction
        {
            SourceFilePath = file.FilePath,
            TempOutputPath = tempPath,
            UseSafeTempReplacement = useSafeTempReplacement,
            Description = string.Join("; ", descriptions),
            Arguments = args
        };
    }

    private static HashSet<int> ParseTrackIdSet(string text)
    {
        return text
            .Split(new[] { ',', ';', ' ', '\t', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(x => int.TryParse(x, out _))
            .Select(int.Parse)
            .ToHashSet();
    }

    private static void ApplyTrackSelection(
        MkvFileItem file,
        List<string> args,
        List<string> descriptions,
        string type,
        string keepOption,
        string noneOption,
        bool removeByLanguage,
        ISet<string> keepLanguages,
        IReadOnlyCollection<MkvTrackItem> removedById)
    {
        var typeTracks = file.Tracks.Where(t => t.Type.Equals(type, StringComparison.OrdinalIgnoreCase)).ToList();
        if (typeTracks.Count == 0)
        {
            return;
        }

        var hasTrackIdRemovalsForType = removedById.Any(t => t.Type.Equals(type, StringComparison.OrdinalIgnoreCase));
        if (!removeByLanguage && !hasTrackIdRemovalsForType)
        {
            return;
        }

        var keepTracks = typeTracks
            .Where(t => !removedById.Any(r => r.MkvMergeId == t.MkvMergeId))
            .Where(t => !removeByLanguage || LanguageMatches(t.Language, keepLanguages))
            .ToList();

        if (keepTracks.Count == 0)
        {
            args.Add(noneOption);
        }
        else
        {
            args.AddRange(new[] { keepOption, string.Join(',', keepTracks.Select(t => MkvTrackSelector.ForMkvMergeTrackId(t.MkvMergeId))) });
        }

        if (removeByLanguage)
        {
            var label = type.Equals("audio", StringComparison.OrdinalIgnoreCase) ? "audio" : "subtitle";
            descriptions.Add($"Keep {label} languages: {FormatLanguageList(keepLanguages)}");
        }
    }

    private static string NormalizeTrackType(string? type)
        => string.IsNullOrWhiteSpace(type) ? "track" : type.Trim().ToLowerInvariant();

    private static string FormatTrackLabel(MkvTrackItem track)
    {
        var lang = string.IsNullOrWhiteSpace(track.Language) ? "und" : track.Language.Trim();
        var codec = string.IsNullOrWhiteSpace(track.Codec) ? "unknown" : track.Codec.Trim();
        var label = $"{lang}:{codec}";
        return string.IsNullOrWhiteSpace(track.Name) ? label : $"{label} - \"{track.Name.Trim()}\"";
    }

    private static HashSet<string> ParseLanguageSet(string text)
    {
        return text
            .Split(new[] { ',', ';', ' ', '\t', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(x => !string.IsNullOrWhiteSpace(x))
            .Select(x => x.ToLowerInvariant())
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
    }

    private static bool LanguageMatches(string? language, ISet<string> keepLanguages)
    {
        if (keepLanguages.Count == 0) return false;
        var normalized = (language ?? "und").Trim().ToLowerInvariant();
        return keepLanguages.Contains(normalized);
    }

    private static string FormatLanguageList(ISet<string> languages)
        => languages.Count == 0 ? "none" : string.Join(", ", languages.OrderBy(x => x));

    private static string BuildTempOutputPath(string sourceFilePath)
    {
        var folder = Path.GetDirectoryName(sourceFilePath) ?? string.Empty;
        var name = Path.GetFileNameWithoutExtension(sourceFilePath);
        return Path.Combine(folder, $"{name}.mkvo-remux.tmp.mkv");
    }

    private static void ReplaceOriginalWithTemp(string sourcePath, string tempPath)
    {
        var backupPath = sourcePath + ".mkvo-remux.bak";
        TryDeleteTempFile(backupPath);

        try
        {
            // File.Replace is preferred when available because it is atomic on supported filesystems.
            // Some Linux/macOS mounted volumes can reject it, so fall back to a conservative
            // move-based replacement that keeps a backup until the new file is in place.
            File.Replace(tempPath, sourcePath, backupPath, ignoreMetadataErrors: true);
            TryDeleteTempFile(backupPath);
        }
        catch (PlatformNotSupportedException)
        {
            ReplaceOriginalWithTempMoveFallback(sourcePath, tempPath, backupPath);
        }
        catch (IOException)
        {
            ReplaceOriginalWithTempMoveFallback(sourcePath, tempPath, backupPath);
        }
        catch (UnauthorizedAccessException)
        {
            ReplaceOriginalWithTempMoveFallback(sourcePath, tempPath, backupPath);
        }
    }

    private static void ReplaceOriginalWithTempMoveFallback(string sourcePath, string tempPath, string backupPath)
    {
        try
        {
            if (File.Exists(sourcePath))
            {
                File.Move(sourcePath, backupPath, overwrite: true);
            }

            File.Move(tempPath, sourcePath, overwrite: true);
            TryDeleteTempFile(backupPath);
        }
        catch
        {
            try
            {
                if (!File.Exists(sourcePath) && File.Exists(backupPath))
                {
                    File.Move(backupPath, sourcePath, overwrite: true);
                }
            }
            catch
            {
                // Preserve the original exception. Recovery is best-effort only.
            }

            throw;
        }
    }

    private static void TryDeleteTempFile(string path)
    {
        try
        {
            if (!string.IsNullOrWhiteSpace(path) && File.Exists(path)) File.Delete(path);
        }
        catch
        {
            // Best-effort cleanup only.
        }
    }

}
