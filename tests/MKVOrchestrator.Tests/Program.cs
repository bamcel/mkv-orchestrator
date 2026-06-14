using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Media;
using MKVOrchestrator.Core.Services.Rename;

var tests = new (string Name, Action Test)[]
{
    ("ActionPlanner builds mkvpropedit cleanup arguments", ActionPlannerBuildsCleanupArguments),
    ("RenamePlanner sanitizes invalid and Windows-risky filename characters", RenamePlannerSanitizesFileNames),
    ("RenamePlanner blocks duplicate targets", RenamePlannerBlocksDuplicateTargets),
    ("CrossPlatformRuntime normalizes quoted and environment paths", CrossPlatformRuntimeNormalizesPaths),
    ("MkvPropEditCommandBuilder uses type ordinal selectors", MkvPropEditCommandBuilderUsesTrackSelectors),
    ("MkvMergeService muxes multiple matching external subtitles", MkvMergeServiceMuxesMultipleMatchingExternalSubtitles)
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
