using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Rename;

var tests = new (string Name, Action Test)[]
{
    ("ActionPlanner builds mkvpropedit cleanup arguments", ActionPlannerBuildsCleanupArguments),
    ("RenamePlanner sanitizes invalid and Windows-risky filename characters", RenamePlannerSanitizesFileNames),
    ("RenamePlanner blocks duplicate targets", RenamePlannerBlocksDuplicateTargets),
    ("RenameEpisodeMatcher maps absolute episode to season episode", RenameEpisodeMatcherMapsAbsoluteEpisode),
    ("RenameEpisodeMatcher maps full list by row order", RenameEpisodeMatcherMapsFullListByRowOrder),
    ("CrossPlatformRuntime normalizes quoted and environment paths", CrossPlatformRuntimeNormalizesPaths),
    ("CrossPlatformRuntime recognizes MP4 as readable media", CrossPlatformRuntimeRecognizesMp4Media),
    ("CodecDisplayNormalizer normalizes common video aliases", CodecDisplayNormalizerNormalizesCommonVideoAliases),
    ("MkvScannerService routes MKV metadata through mkvmerge first", MkvScannerServiceRoutesMkvThroughMkvMergeFirst),
    ("MkvPropEditCommandBuilder uses type ordinal selectors", MkvPropEditCommandBuilderUsesTrackSelectors),
    ("MkvMergeService muxes multiple matching external subtitles", MkvMergeServiceMuxesMultipleMatchingExternalSubtitles),
    ("MkvMergeService leaves MP4 files read-only", MkvMergeServiceLeavesMp4ReadOnly)
};

var failures = 0;
foreach (var (name, test) in tests)
{
    try
    {
        test();
        Console.WriteLine($"PASS {name}");
    }
    catch (Exception ex)
    {
        failures++;
        Console.Error.WriteLine($"FAIL {name}: {ex.Message}");
    }
}

return failures == 0 ? 0 : 1;

static void ActionPlannerBuildsCleanupArguments()
{
    var file = new MkvFileItem
    {
        FilePath = Path.Combine("media", "Show - S01E01.mkv"),
        ContainerTitle = "Old Title"
    };
    file.Tracks.Add(new MkvTrackItem { Type = "video", Name = "Old Video", PropEditTrackNumber = 1 });
    file.Tracks.Add(new MkvTrackItem { Type = "audio", Name = "Old Audio", PropEditTrackNumber = 2 });

    var actions = new ActionPlanner().BuildActions(new[] { file }, new EditOptions
    {
        RemoveContainerTitle = true,
        RemoveVideoTrackTitles = true,
        RemoveAudioTrackTitles = true,
        SetAudioLanguage = "eng"
    });

    AssertEqual(1, actions.Count);
    AssertContains("--delete", actions[0].Arguments);
    AssertContains("title", actions[0].Arguments);
    AssertContains("language=eng", actions[0].Arguments);
}

static void RenamePlannerSanitizesFileNames()
{
    var clean = RenamePlanner.SanitizeFileName("Show: Bad*Name? <Pilot>|");
    AssertEqual("Show Bad Name Pilot", clean);
}

static void RenamePlannerBlocksDuplicateTargets()
{
    var files = new[]
    {
        new MediaFile { FilePath = Path.Combine("media", "a.mkv"), SeriesTitle = "Show", Season = 1, Episode = 1, EpisodeTitle = "Pilot" },
        new MediaFile { FilePath = Path.Combine("media", "b.mkv"), SeriesTitle = "Show", Season = 1, Episode = 1, EpisodeTitle = "Pilot" }
    };

    var plan = new RenamePlanner().BuildPlan(new RenamePlanRequest(files, "{series} - S{season:00}E{episode:00} - {episodeTitle}", CheckExistingFiles: false));
    AssertTrue(plan.HasBlockingIssues, "duplicate targets should block apply");
    AssertTrue(plan.Items.All(i => !i.CanApply), "all duplicate rows should be non-applicable");
}

static void RenameEpisodeMatcherMapsAbsoluteEpisode()
{
    var episodes = BuildAttackOnTitanEpisodeShape();

    AssertTrue(RenameEpisodeMatcher.TryMatchAbsoluteEpisode(episodes, 87, out var match), "absolute episode should map to provider episode");
    AssertEqual(4, match.Episode.SeasonNumber);
    AssertEqual(23, match.Episode.EpisodeNumber);
    AssertEqual("Episode 87 = S04E23", match.StatusText);
}

static void RenameEpisodeMatcherMapsFullListByRowOrder()
{
    var episodes = BuildAttackOnTitanEpisodeShape();
    var matches = RenameEpisodeMatcher.MatchByListOrder(episodes, episodes.Count);

    AssertEqual(94, matches.Count);
    AssertEqual(4, matches[86].Episode.SeasonNumber);
    AssertEqual(23, matches[86].Episode.EpisodeNumber);
    AssertEqual(4, matches[92].Episode.SeasonNumber);
    AssertEqual(29, matches[92].Episode.EpisodeNumber);
    AssertEqual(4, matches[93].Episode.SeasonNumber);
    AssertEqual(30, matches[93].Episode.EpisodeNumber);
    AssertEqual("List order match: row 93 = S04E29", matches[92].StatusText);
}

static List<TvdbEpisode> BuildAttackOnTitanEpisodeShape()
{
    var episodes = new List<TvdbEpisode>();
    var id = 1;
    for (var episode = 1; episode <= 25; episode++)
    {
        episodes.Add(new TvdbEpisode { Id = id++, SeasonNumber = 1, EpisodeNumber = episode, Name = $"S1 E{episode}" });
    }

    for (var episode = 1; episode <= 12; episode++)
    {
        episodes.Add(new TvdbEpisode { Id = id++, SeasonNumber = 2, EpisodeNumber = episode, Name = $"S2 E{episode}" });
    }

    for (var episode = 1; episode <= 27; episode++)
    {
        episodes.Add(new TvdbEpisode { Id = id++, SeasonNumber = 3, EpisodeNumber = episode, Name = $"S3 E{episode}" });
    }

    for (var episode = 1; episode <= 30; episode++)
    {
        episodes.Add(new TvdbEpisode { Id = id++, SeasonNumber = 4, EpisodeNumber = episode, Name = $"S4 E{episode}" });
    }

    return episodes;
}

static void CrossPlatformRuntimeNormalizesPaths()
{
    var variableName = "MKVO_TEST_PATH";
    Environment.SetEnvironmentVariable(variableName, "expanded");
    var normalized = CrossPlatformRuntime.NormalizeUserPath("\"%" + variableName + "%\\folder\"");

    if (CrossPlatformRuntime.IsWindows)
    {
        AssertEqual("expanded\\folder", normalized);
    }
    else
    {
        AssertTrue(normalized.Length > 0, "normalized path should not be empty");
    }
}

static void CrossPlatformRuntimeRecognizesMp4Media()
{
    AssertTrue(CrossPlatformRuntime.IsSupportedMediaPath("episode.mkv"), "MKV should be supported media");
    AssertTrue(CrossPlatformRuntime.IsSupportedMediaPath("episode.mp4"), "MP4 should be supported media");
    AssertTrue(!CrossPlatformRuntime.IsSupportedMediaPath("episode.avi"), "AVI should not be included in this read-only pass");
}

static void CodecDisplayNormalizerNormalizesCommonVideoAliases()
{
    AssertEqual("HEVC/H.265", CodecDisplayNormalizer.Normalize("hevc"));
    AssertEqual("HEVC/H.265", CodecDisplayNormalizer.Normalize("HEVC/H.265/MPEG-H"));
    AssertEqual("AVC/H.264", CodecDisplayNormalizer.Normalize("h264"));
    AssertEqual("AVC/H.264", CodecDisplayNormalizer.Normalize("AVC/H.264"));
    AssertEqual("AV1", CodecDisplayNormalizer.Normalize("av1"));
}

static void MkvScannerServiceRoutesMkvThroughMkvMergeFirst()
{
    AssertEqual("mkvmerge", MkvScannerService.GetPrimaryMetadataReaderName("Episode 01.mkv"));
    AssertEqual("mkvmerge", MkvScannerService.GetPrimaryMetadataReaderName("Episode 01.MKV"));
    AssertEqual("ffprobe", MkvScannerService.GetPrimaryMetadataReaderName("Episode 01.mp4"));
    AssertEqual("ffprobe", MkvScannerService.GetPrimaryMetadataReaderName("Episode 01.MP4"));
}

static void MkvPropEditCommandBuilderUsesTrackSelectors()
{
    var file = new MkvFileItem { FilePath = Path.Combine("media", "movie.mkv") };
    file.Tracks.Add(new MkvTrackItem { Type = "video", PropEditTrackNumber = 1, MkvMergeId = 0, Name = "Old Video" });
    file.Tracks.Add(new MkvTrackItem { Type = "audio", PropEditTrackNumber = 2, MkvMergeId = 1, Name = "Stereo", Language = "jpn" });
    file.Tracks.Add(new MkvTrackItem { Type = "audio", PropEditTrackNumber = 3, MkvMergeId = 2, Name = "Commentary", Language = "eng" });

    var result = new MkvPropEditCommandBuilder().Build(new MkvPropEditCommandBuildRequest(
        file,
        AudioConfigs: new[]
        {
            new PropEditTrackConfig { Type = "audio", TrackNumber = 3, TrackLabel = "Audio 2", EditedName = "English", EditedLanguage = "eng" }
        },
        SubtitleConfigs: Array.Empty<PropEditTrackConfig>(),
        SelectedDefaultAudio: "Audio 2",
        SelectedForcedAudio: "Keep existing",
        SelectedDefaultSubtitle: "Keep existing",
        SelectedForcedSubtitle: "Keep existing",
        ContainerTitleFromFile: false,
        ContainerTitleCustom: false,
        RemoveContainerTitle: false,
        CustomContainerTitle: string.Empty,
        VideoTitleFromFile: false,
        VideoTitleCustom: false,
        RemoveVideoTitle: true,
        CustomVideoTitle: string.Empty));

    AssertContains("track:v1", result.Arguments);
    AssertContains("track:a2", result.Arguments);
    AssertContains("name=English", result.Arguments);
}

static void MkvMergeServiceMuxesMultipleMatchingExternalSubtitles()
{
    var folder = Path.Combine(Path.GetTempPath(), "mkvo-subtitle-test-" + Guid.NewGuid().ToString("N"));
    Directory.CreateDirectory(folder);
    try
    {
        var mkvPath = Path.Combine(folder, "Episode 01.mkv");
        var signsPath = Path.Combine(folder, "Episode 01.eng.Signs & Songs.ass");
        var dialoguePath = Path.Combine(folder, "Episode 01.jpn.Dialogue.ass");
        File.WriteAllText(mkvPath, string.Empty);
        File.WriteAllText(signsPath, string.Empty);
        File.WriteAllText(dialoguePath, string.Empty);

        var file = new MkvFileItem
        {
            FilePath = mkvPath,
            Selected = true
        };

        var plan = new MkvMergeService().BuildRemuxPlan(
            new[] { file },
            keepAudioLanguages: "eng,jpn",
            keepSubtitleLanguages: "eng",
            removeUnwantedAudioLanguages: false,
            removeUnwantedSubtitleLanguages: false,
            removeUnwantedTrackIds: false,
            removeTrackIdsText: string.Empty,
            preserveChapters: true,
            preserveAttachments: true,
            useSafeTempReplacement: true,
            muxMatchingExternalSubtitles: true,
            externalSubtitleLanguage: "und",
            externalSubtitleTrackName: "{tag}",
            externalSubtitleFormats: "ass,srt",
            preserveExternalSubtitleFiles: true,
            skipMuxIfSubtitleAlreadyExists: true,
            extractSubtitles: false,
            extractSubtitleLanguages: "eng",
            extractOverwriteExistingFiles: false);

        AssertEqual(1, plan.Actions.Count);
        var action = plan.Actions[0];
        AssertEqual(2, action.ExternalSubtitleFilePaths.Count);
        AssertContains(signsPath, action.Arguments);
        AssertContains(dialoguePath, action.Arguments);
        AssertContains("0:eng", action.Arguments);
        AssertContains("0:jpn", action.Arguments);
        AssertContains("0:Signs & Songs", action.Arguments);
        AssertContains("0:Dialogue", action.Arguments);
    }
    finally
    {
        if (Directory.Exists(folder))
        {
            Directory.Delete(folder, recursive: true);
        }
    }
}

static void MkvMergeServiceLeavesMp4ReadOnly()
{
    var file = new MkvFileItem
    {
        FilePath = Path.Combine("media", "Episode 01.mp4"),
        Selected = true
    };

    var plan = new MkvMergeService().BuildRemuxPlan(
        new[] { file },
        keepAudioLanguages: "eng,jpn",
        keepSubtitleLanguages: "eng",
        removeUnwantedAudioLanguages: true,
        removeUnwantedSubtitleLanguages: true,
        removeUnwantedTrackIds: false,
        removeTrackIdsText: string.Empty,
        preserveChapters: true,
        preserveAttachments: true,
        useSafeTempReplacement: true,
        muxMatchingExternalSubtitles: true,
        externalSubtitleLanguage: "eng",
        externalSubtitleTrackName: "{tag}",
        externalSubtitleFormats: "ass,srt",
        preserveExternalSubtitleFiles: true,
        skipMuxIfSubtitleAlreadyExists: true,
        extractSubtitles: true,
        extractSubtitleLanguages: "eng",
        extractOverwriteExistingFiles: false);

    AssertEqual(0, plan.Actions.Count);
    AssertContains(file.FilePath, plan.NoChangeFiles);
}

static void AssertEqual<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"expected '{expected}', got '{actual}'");
    }
}

static void AssertTrue(bool condition, string message)
{
    if (!condition)
    {
        throw new InvalidOperationException(message);
    }
}

static void AssertContains(string expected, IEnumerable<string> values)
{
    if (!values.Contains(expected))
    {
        throw new InvalidOperationException($"expected sequence to contain '{expected}'");
    }
}
