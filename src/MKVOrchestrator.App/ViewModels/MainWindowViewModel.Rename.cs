using System.Collections.Generic;
using System.Linq;
using System;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Text;
using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.Input;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Rename;

namespace MKVOrchestrator.App.ViewModels;

public partial class MainWindowViewModel
{
    private void SyncRenameFromDashboardSelection(bool preserveSearchTitle = true, bool writeLog = false)
    {
        if (AppState.IsScanning)
        {
            RenameStatusText = "Loading...";
            return;
        }

        var previousSearchTitle = RenameSearchTitle;
        RenameItems.Clear();

        var sourceFiles = AppState.GetDashboardRenameSourceFiles().ToList();
        if (sourceFiles.Count == 0)
        {
            RenameStatusText = Files.Count == 0
                ? "Linked to Dashboard: no scanned files"
                : "Linked to Dashboard: no checked Dashboard files";
            if (writeLog) RenameLog(Files.Count == 0
                ? "Scan files on the Dashboard first, then return here to build rename rows."
                : "Check one or more files on the Dashboard to include them in MKVRename.");
            return;
        }

        foreach (var file in sourceFiles.OrderBy(f => GetFileSortKey(f.FilePath).Season)
                                        .ThenBy(f => GetFileSortKey(f.FilePath).Episode)
                                        .ThenBy(f => GetFileSortKey(f.FilePath).RelativeName, NaturalStringComparer.Instance))
        {
            var detected = DetectEpisodeInfo(file.FilePath);
            var mediaFile = file.ToMediaFile();
            mediaFile.SeriesTitle = detected.SeriesTitle;
            mediaFile.Season = detected.Season;
            mediaFile.Episode = detected.Episode;
            mediaFile.AbsoluteEpisode = detected.AbsoluteEpisode;
            mediaFile.ProviderMatch.Confidence = detected.Season.HasValue && detected.Episode.HasValue ? "Detected" : "Needs review";
            mediaFile.ProviderMatch.Status = detected.Season.HasValue && detected.Episode.HasValue ? "Ready for metadata match" : "Could not detect S/E";

            RenameItems.Add(new RenamePreviewItem
            {
                MediaFile = mediaFile,
                Confidence = mediaFile.ProviderMatch.Confidence,
                Status = mediaFile.ProviderMatch.Status
            });
        }

        var guessedTitle = GuessRenameSearchTitle();
        if (!preserveSearchTitle || string.IsNullOrWhiteSpace(previousSearchTitle))
        {
            RenameSearchTitle = guessedTitle;
        }

        RenameStatusText = $"Linked to Dashboard: {RenameItems.Count} file(s) ready for rename preview";
        if (writeLog)
        {
            RenameLog(RenameStatusText);
            if (!string.IsNullOrWhiteSpace(RenameSearchTitle)) RenameLog($"Suggested metadata search: {RenameSearchTitle}");
        }
    }


    private void ResetRenameMetadataContextForNewScan()
    {
        // A new Dashboard scan represents a new media context. Clear provider/search
        // state so MKVRename does not keep the previous show's title or selected result.
        RenameSearchTitle = string.Empty;
        SelectedTvdbSeries = null;
        TvdbSeriesResults.Clear();
        TvdbSeasonScopeOptions.Clear();
        EpisodeScopeSummary = string.Empty;
        _cachedTvdbEpisodes.Clear();
        _cachedTvdbSeriesId = null;
        _cachedTvdbLanguage = string.Empty;
        _cachedLookupProvider = string.Empty;
        RenameStatusText = "Linked to Dashboard: waiting for scan to finish";
    }

    [RelayCommand]
    private void RefreshRenameFromDashboard()
    {
        RenameConsoleLines.Clear();
        SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: true);
    }

    [RelayCommand]
    private async Task SearchTvdb()
    {
        var query = RenameSearchTitle?.Trim() ?? string.Empty;
        if (string.IsNullOrWhiteSpace(query))
        {
            RenameLog("Enter a show title to search.");
            return;
        }

        _cts?.Cancel();
        _cts = new CancellationTokenSource();
        IsBusy = true;
        var provider = NormalizeLookupProvider(RenameLookupProvider);
        if (!ValidateRenameProviderConfigured(provider, log: true))
        {
            return;
        }

        RenameStatusText = $"Searching {provider}...";
        TvdbSeriesResults.Clear();
        RenameLog($"{provider} search: {query} | language: {TvdbLanguage}");

        try
        {
            var settings = BuildCurrentSettingsSnapshot();
            var metadataProvider = GetRenameMetadataProvider(provider);
            IReadOnlyList<TvdbSeriesSearchResult> results = await metadataProvider.SearchSeriesAsync(query, TvdbLanguage, settings, _cts.Token);
            foreach (var result in results)
            {
                result.Provider = metadataProvider.Key;
                TvdbSeriesResults.Add(result);
            }

            // Default to the top provider result so lookup results and episode scopes are
            // visually populated immediately after search. The user can still change the
            // selected provider result afterward.
            _suppressSelectedSeriesAutoLoad = true;
            SelectedTvdbSeries = TvdbSeriesResults.FirstOrDefault();
            _suppressSelectedSeriesAutoLoad = false;

            EpisodeScopeSummary = "Loading...";
            RenameStatusText = $"{provider} results: {TvdbSeriesResults.Count}";
            RenameLog(RenameStatusText);
            if (SelectedTvdbSeries is not null)
            {
                RenameLog($"Default selected {provider} result: {SelectedTvdbSeries.DisplayName}");
                await LoadTvdbSeasonScopesAndPreviewAsync();
            }
            else
            {
                TvdbSeasonScopeOptions.Clear();
                RenameLog($"No {provider} results found.");
            }
        }
        catch (Exception ex)
        {
            RenameStatusText = $"{provider} search failed";
            RenameLog($"{provider} search failed: " + ex.Message);
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task BuildRenamePreview()
    {
        if (SelectedTvdbSeries is null)
        {
            RenameLog($"Select a {NormalizeLookupProvider(RenameLookupProvider)} result first.");
            return;
        }

        if (RenameItems.Count == 0)
        {
            SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: true);
            if (RenameItems.Count == 0) return;
        }

        var provider = NormalizeLookupProvider(RenameLookupProvider);
        if (!ValidateRenameProviderConfigured(provider, log: true))
        {
            return;
        }

        _cts?.Cancel();
        _cts = new CancellationTokenSource();
        IsBusy = true;
        var isMovie = IsSelectedMetadataResultMovie();
        RenameStatusText = isMovie ? $"Loading {provider} movie metadata..." : $"Loading {provider} episodes...";
        RenameLog($"Loading {(isMovie ? "movie metadata" : "episodes")} for {SelectedTvdbSeries.DisplayName} using {provider} language {TvdbLanguage}...");

        try
        {
            var allEpisodes = await LoadAllTvdbEpisodesForSelectedSeriesAsync(_cts.Token);
            var orderedEpisodes = FilterEpisodesByCheckedScopes(allEpisodes);
            RenameLog(isMovie
                ? $"{provider} movie metadata loaded for preview."
                : $"{provider} episodes selected for preview: {orderedEpisodes.Count} of {allEpisodes.Count}");

            var episodeMap = orderedEpisodes
                .GroupBy(e => (e.SeasonNumber, e.EpisodeNumber))
                .ToDictionary(g => g.Key, g => g.First());
            var renameItemsInPreviewOrder = RenameItems.ToList();
            var orderedEpisodeMatches = isMovie
                ? Array.Empty<OrderedEpisodeMatch>()
                : RenameEpisodeMatcher.MatchByListOrder(orderedEpisodes, renameItemsInPreviewOrder.Count).ToArray();

            var usedEpisodeIds = new HashSet<int>();
            var exactMatched = 0;
            var absoluteMatched = 0;
            var listOrderMatched = 0;
            var sequentialMatched = 0;
            var selectedYear = int.TryParse(SelectedTvdbSeries.Year, out var year) ? year : (int?)null;

            for (var itemIndex = 0; itemIndex < renameItemsInPreviewOrder.Count; itemIndex++)
            {
                var item = renameItemsInPreviewOrder[itemIndex];
                item.IsMovieMatch = isMovie;
                TvdbEpisode? episode = null;
                TvdbEpisode? exactEpisode = null;
                var exactMatch = false;
                AbsoluteEpisodeMatch? absoluteMatch = null;
                OrderedEpisodeMatch? orderedMatch = null;

                if (!isMovie
                    && item.Season.HasValue
                    && item.Episode.HasValue
                    && episodeMap.TryGetValue((item.Season.Value, item.Episode.Value), out var mappedEpisode))
                {
                    exactEpisode = mappedEpisode;
                }

                if (isMovie)
                {
                    episode = orderedEpisodes.FirstOrDefault();
                    exactMatch = episode is not null;
                }
                else if (orderedEpisodeMatches.Length == renameItemsInPreviewOrder.Count)
                {
                    orderedMatch = orderedEpisodeMatches[itemIndex];
                    episode = orderedMatch.Episode;
                    item.Season = episode.SeasonNumber;
                    item.Episode = episode.EpisodeNumber;
                    exactMatch = exactEpisode?.Id == episode.Id;
                }
                else if (exactEpisode is not null)
                {
                    episode = exactEpisode;
                    exactMatch = true;
                }
                else if (RenameEpisodeMatcher.TryMatchAbsoluteEpisode(orderedEpisodes, item.AbsoluteEpisode, out var mappedAbsoluteEpisode))
                {
                    absoluteMatch = mappedAbsoluteEpisode;
                    episode = mappedAbsoluteEpisode.Episode;
                    item.Season = episode.SeasonNumber;
                    item.Episode = episode.EpisodeNumber;
                }
                else
                {
                    episode = orderedEpisodes.FirstOrDefault(e => !usedEpisodeIds.Contains(e.Id));
                }

                if (episode is null)
                {
                    item.Status = "No provider episode available";
                    item.Confidence = "Low";
                    continue;
                }

                usedEpisodeIds.Add(episode.Id);
                item.SeriesTitle = SelectedTvdbSeries.Name;
                item.SeriesYear = selectedYear;
                item.TvdbEpisodeId = episode.Id;
                item.MatchedEpisodeTitle = episode.Name;
                item.MediaFile.EpisodeTitle = episode.Name;
                item.MediaFile.ProviderMatch.Provider = provider;
                item.MediaFile.ProviderMatch.SeriesId = SelectedTvdbSeries.Id;
                item.MediaFile.ProviderMatch.SeriesName = SelectedTvdbSeries.Name;
                item.MediaFile.ProviderMatch.EpisodeName = episode.Name;
                item.NewFileName = BuildRenameFileName(item, episode);

                if (isMovie)
                {
                    item.Confidence = "High";
                    item.Status = "Movie match";
                    exactMatched++;
                }
                else if (exactMatch)
                {
                    item.Confidence = "High";
                    item.Status = "Exact S/E match";
                    exactMatched++;
                }
                else if (orderedMatch is not null)
                {
                    item.Confidence = "High";
                    item.Status = orderedMatch.StatusText;
                    listOrderMatched++;
                }
                else if (absoluteMatch is not null)
                {
                    item.Confidence = "High";
                    item.Status = absoluteMatch.StatusText;
                    absoluteMatched++;
                }
                else
                {
                    item.Confidence = "Low";
                    item.Status = "Sequential fallback - verify";
                    sequentialMatched++;
                }

                item.MediaFile.ProviderMatch.Confidence = item.Confidence;
                item.MediaFile.ProviderMatch.Status = item.Status;
            }

            var filesScanned = RenameItems.Count;
            var filesChanged = RenameItems.Count(i =>
                !string.IsNullOrWhiteSpace(i.NewFileName)
                && !string.Equals(i.CurrentFileName, i.NewFileName, StringComparison.OrdinalIgnoreCase));
            var filesSkipped = filesScanned - filesChanged;

            RenameStatusText = isMovie
                ? $"Preview ready: {exactMatched} movie match, {RenameItems.Count - exactMatched} unmatched"
                : $"Preview ready: {exactMatched} exact, {listOrderMatched} list order, {absoluteMatched} absolute, {sequentialMatched} sequential fallback, {RenameItems.Count - exactMatched - listOrderMatched - absoluteMatched - sequentialMatched} unmatched";
            IsRenamePreviewDirty = false;
            BuildRenamePreviewSummary(filesChanged, filesSkipped);
        }
        catch (Exception ex)
        {
            RenameStatusText = "Preview failed";
            RenameLog("Preview failed: " + ex.Message);
        }
        finally
        {
            IsBusy = false;
        }
    }

    [RelayCommand]
    private async Task CopySelectedDatabaseUrl(Window window)
    {
        var url = SelectedDatabaseUrl;
        if (string.IsNullOrWhiteSpace(url)) return;

        if (window.Clipboard is not null)
        {
            await window.Clipboard.SetTextAsync(url);
            RenameStatusText = "Database URL copied to clipboard.";
            RenameLog(RenameStatusText);
        }
    }

    [RelayCommand]
    private void OpenSelectedDatabaseUrl()
    {
        var url = SelectedDatabaseUrl;
        if (string.IsNullOrWhiteSpace(url)) return;

        try
        {
            Process.Start(new ProcessStartInfo
            {
                FileName = url,
                UseShellExecute = true
            });
        }
        catch (Exception ex)
        {
            RenameStatusText = "Could not open database URL.";
            RenameLog($"Could not open database URL: {ex.Message}");
        }
    }

    private void BuildRenamePreviewSummary(int filesChanged, int filesSkipped)
    {
        RenameConsoleLines.Clear();
        RenameConsoleLines.Add("mkvrename Summary - DRY RUN");
        RenameConsoleLines.Add($"Generated: {DateTime.Now:yyyy-MM-dd HH:mm:ss}");
        RenameConsoleLines.Add($"Files changed: {filesChanged} | Files skipped: {filesSkipped}");
        RenameConsoleLines.Add(new string('=', 92));

        var changedItems = RenameItems
            .Where(i => !string.IsNullOrWhiteSpace(i.NewFileName)
                        && !string.Equals(i.CurrentFileName, i.NewFileName, StringComparison.OrdinalIgnoreCase))
            .OrderBy(i => i.Season ?? int.MaxValue)
            .ThenBy(i => i.Episode ?? int.MaxValue)
            .ThenBy(i => i.CurrentFileName, StringComparer.OrdinalIgnoreCase)
            .ToList();

        if (changedItems.Count > 0)
        {
            RenameConsoleLines.Add("CHANGED FILES:");

            foreach (var item in changedItems)
            {
                RenameConsoleLines.Add($"  - {item.CurrentFileName} -> {item.NewFileName}");
            }
        }
        else
        {
            RenameConsoleLines.Add("CHANGED FILES: None");
        }

        RenameConsoleLines.Add(new string('=', 92));
    }

    private async Task LoadTvdbSeasonScopesAndPreviewAsync()
    {
        if (SelectedTvdbSeries is null) return;

        try
        {
            var provider = NormalizeLookupProvider(RenameLookupProvider);
            if (!ValidateRenameProviderConfigured(provider, log: true))
            {
                return;
            }

            _cts?.Cancel();
            _cts = new CancellationTokenSource();
            IsBusy = true;
            RenameStatusText = $"Loading {provider} scopes...";
            RenameLog($"Loading available scopes for {SelectedTvdbSeries.DisplayName} from {provider}...");

            var episodes = await LoadAllTvdbEpisodesForSelectedSeriesAsync(_cts.Token, forceReload: true);
            RebuildTvdbSeasonScopeOptions(episodes);
            RenameLog($"{provider} scopes loaded: {string.Join(", ", TvdbSeasonScopeOptions.Select(o => o.DisplayName))}");
        }
        catch (Exception ex)
        {
            RenameStatusText = "Lookup scope load failed";
            RenameLog("Lookup scope load failed: " + ex.Message);
        }
        finally
        {
            IsBusy = false;
        }

        await BuildRenamePreview();
    }

    private async Task<List<TvdbEpisode>> LoadAllTvdbEpisodesForSelectedSeriesAsync(CancellationToken token, bool forceReload = false)
    {
        if (SelectedTvdbSeries is null) return new List<TvdbEpisode>();

        var language = string.IsNullOrWhiteSpace(TvdbLanguage) ? "eng" : TvdbLanguage.Trim();
        var provider = NormalizeLookupProvider(RenameLookupProvider);
        if (!forceReload
            && _cachedTvdbSeriesId == SelectedTvdbSeries.Id
            && string.Equals(_cachedTvdbLanguage, language, StringComparison.OrdinalIgnoreCase)
            && string.Equals(_cachedLookupProvider, provider, StringComparison.OrdinalIgnoreCase)
            && _cachedTvdbEpisodes.Count > 0)
        {
            return _cachedTvdbEpisodes;
        }

        var settings = BuildCurrentSettingsSnapshot();
        var metadataProvider = GetRenameMetadataProvider(provider);
        IReadOnlyList<TvdbEpisode> loaded = await metadataProvider.GetEpisodesAsync(SelectedTvdbSeries, language, settings, token);

        _cachedTvdbEpisodes = OrderEpisodesForProvider(loaded, provider).ToList();
        _cachedTvdbSeriesId = SelectedTvdbSeries.Id;
        _cachedTvdbLanguage = language;
        _cachedLookupProvider = provider;
        return _cachedTvdbEpisodes;
    }

    private void RebuildTvdbSeasonScopeOptions(IReadOnlyList<TvdbEpisode> episodes)
    {
        TvdbSeasonScopeOptions.Clear();
        EpisodeScopeSummary = string.Empty;
        if (episodes.Count == 0) return;

        if (IsSelectedMetadataResultMovie())
        {
            AddTvdbSeasonScopeOption(new TvdbSeasonScopeOption
            {
                DisplayName = "N/A",
                SeasonNumber = 1,
                ScopeName = "Movie",
                IsSelected = true
            });
            UpdateEpisodeScopeSummary();
            return;
        }

        RebuildSeasonNativeScopeOptions(episodes);
    }

    private void RebuildSeasonNativeScopeOptions(IReadOnlyList<TvdbEpisode> episodes)
    {
        var seasons = episodes.Select(e => e.SeasonNumber).Distinct().OrderBy(s => s).ToList();
        var hasRegularSeasons = seasons.Any(s => s > 0);

        if (hasRegularSeasons)
        {
            AddTvdbSeasonScopeOption(new TvdbSeasonScopeOption
            {
                DisplayName = "All seasons",
                IsAllRegularSeasonsOption = true,
                SeasonNumber = null,
                ScopeName = "AllRegular",
                IsSelected = true
            });
        }

        AddTvdbSeasonScopeOption(new TvdbSeasonScopeOption
        {
            DisplayName = "All seasons + specials",
            IsAllOption = true,
            SeasonNumber = null,
            ScopeName = "All",
            IsSelected = false
        });

        foreach (var season in seasons.Where(s => s > 0))
        {
            var count = episodes.Count(e => e.SeasonNumber == season);
            AddTvdbSeasonScopeOption(new TvdbSeasonScopeOption
            {
                DisplayName = $"Season {season} ({count} episodes)",
                SeasonNumber = season,
                ScopeName = $"Season {season}",
                IsSelected = true
            });
        }

        if (seasons.Contains(0))
        {
            var count = episodes.Count(e => e.SeasonNumber == 0);
            AddTvdbSeasonScopeOption(new TvdbSeasonScopeOption
            {
                DisplayName = $"Specials ({count})",
                SeasonNumber = 0,
                ScopeName = "Specials",
                IsSelected = false
            });
        }

        _suppressScopeSelectionCascade = true;
        try
        {
            SyncParentScopeSelections();
        }
        finally
        {
            _suppressScopeSelectionCascade = false;
        }

        UpdateEpisodeScopeSummary();
    }


    private void AddTvdbSeasonScopeOption(TvdbSeasonScopeOption option)
    {
        option.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(TvdbSeasonScopeOption.IsSelected))
            {
                HandleTvdbSeasonScopeSelectionChanged(option);
            }
        };
        TvdbSeasonScopeOptions.Add(option);
    }

    private void HandleTvdbSeasonScopeSelectionChanged(TvdbSeasonScopeOption changed)
    {
        if (_suppressScopeSelectionCascade) return;

        _suppressScopeSelectionCascade = true;
        try
        {
            if (changed.IsSelected && changed.IsAllOption)
            {
                // Visual shortcut: All seasons + specials selects every concrete scope
                // and clears the All seasons shortcut so only the broader parent stays active.
                foreach (var option in TvdbSeasonScopeOptions)
                {
                    if (ReferenceEquals(option, changed))
                    {
                        option.IsSelected = true;
                    }
                    else if (option.IsAllRegularSeasonsOption)
                    {
                        option.IsSelected = false;
                    }
                    else
                    {
                        option.IsSelected = option.SeasonNumber.HasValue;
                    }
                }

                RenameStatusText = "Episode scopes: all seasons and specials selected.";
            }
            else if (changed.IsSelected && changed.IsAllRegularSeasonsOption)
            {
                // Visual shortcut: All seasons selects only regular seasons,
                // clears All seasons + specials, and clears Specials.
                foreach (var option in TvdbSeasonScopeOptions)
                {
                    if (ReferenceEquals(option, changed))
                    {
                        option.IsSelected = true;
                    }
                    else if (option.IsAllOption)
                    {
                        option.IsSelected = false;
                    }
                    else if (option.SeasonNumber == 0 || string.Equals(option.ScopeName, "Specials", StringComparison.OrdinalIgnoreCase))
                    {
                        option.IsSelected = false;
                    }
                    else
                    {
                        option.IsSelected = option.SeasonNumber.HasValue && option.SeasonNumber.Value > 0;
                    }
                }

                RenameStatusText = "Episode scopes: all regular seasons selected.";
            }
            else if (!changed.IsAllOption && !changed.IsAllRegularSeasonsOption)
            {
                SyncParentScopeSelections();
            }
            else if (!changed.IsSelected)
            {
                SyncParentScopeSelections();
            }
        }
        finally
        {
            _suppressScopeSelectionCascade = false;
        }

        UpdateEpisodeScopeSummary();
    }

    private void SyncParentScopeSelections()
    {
        var allOption = TvdbSeasonScopeOptions.FirstOrDefault(o => o.IsAllOption);
        var allRegularOption = TvdbSeasonScopeOptions.FirstOrDefault(o => o.IsAllRegularSeasonsOption);
        var regularSeasonOptions = TvdbSeasonScopeOptions
            .Where(o => !o.IsAllOption && !o.IsAllRegularSeasonsOption && o.SeasonNumber.HasValue && o.SeasonNumber.Value > 0)
            .ToList();
        var specialOptions = TvdbSeasonScopeOptions
            .Where(o => !o.IsAllOption && !o.IsAllRegularSeasonsOption && o.SeasonNumber.HasValue && o.SeasonNumber.Value == 0)
            .ToList();

        var hasRegularSeasons = regularSeasonOptions.Count > 0;
        var allRegularSelected = hasRegularSeasons && regularSeasonOptions.All(o => o.IsSelected);
        var hasSpecials = specialOptions.Count > 0;
        var allSpecialsSelected = hasSpecials && specialOptions.All(o => o.IsSelected);

        if (allOption is not null)
        {
            allOption.IsSelected = allRegularSelected && (!hasSpecials || allSpecialsSelected);
        }

        if (allRegularOption is not null)
        {
            allRegularOption.IsSelected = allRegularSelected && !(hasSpecials && allSpecialsSelected);
        }
    }


    private void UpdateEpisodeScopeSummary()
    {
        if (TvdbSeasonScopeOptions.Count == 0)
        {
            EpisodeScopeSummary = string.Empty;
            return;
        }

        var allOption = TvdbSeasonScopeOptions.FirstOrDefault(o => o.IsAllOption);
        var allRegularOption = TvdbSeasonScopeOptions.FirstOrDefault(o => o.IsAllRegularSeasonsOption);

        if (allOption?.IsSelected == true)
        {
            EpisodeScopeSummary = allOption.DisplayName;
            return;
        }

        if (allRegularOption?.IsSelected == true)
        {
            EpisodeScopeSummary = allRegularOption.DisplayName;
            return;
        }

        var selected = TvdbSeasonScopeOptions
            .Where(o => o.IsSelected && !o.IsAllOption && !o.IsAllRegularSeasonsOption)
            .Select(o => SimplifyScopeDisplayName(o.DisplayName))
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .ToList();

        EpisodeScopeSummary = selected.Count switch
        {
            0 => string.Empty,
            <= 4 => string.Join(", ", selected),
            _ => $"{selected.Count} scopes selected"
        };
    }

    private static string SimplifyScopeDisplayName(string displayName)
    {
        if (string.IsNullOrWhiteSpace(displayName)) return string.Empty;

        var parenIndex = displayName.IndexOf('(');
        return parenIndex > 0
            ? displayName[..parenIndex].Trim()
            : displayName.Trim();
    }

    private List<TvdbEpisode> FilterEpisodesByCheckedScopes(IReadOnlyList<TvdbEpisode> allEpisodes)
    {
        if (allEpisodes.Count == 0) return new List<TvdbEpisode>();

        var provider = NormalizeLookupProvider(RenameLookupProvider);
        var selectedScopes = TvdbSeasonScopeOptions.Where(o => o.IsSelected).ToList();
        if (selectedScopes.Count == 0 || selectedScopes.Any(o => o.IsAllOption))
        {
            return OrderEpisodesForProvider(allEpisodes, provider).ToList();
        }

        var includeAllRegularSeasons = selectedScopes.Any(o => o.IsAllRegularSeasonsOption);
        var selectedSeasons = selectedScopes.Where(o => o.SeasonNumber.HasValue).Select(o => o.SeasonNumber!.Value).ToHashSet();
        var selectedScopeNames = selectedScopes
            .Where(o => !string.IsNullOrWhiteSpace(o.ScopeName))
            .Select(o => o.ScopeName)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var filtered = allEpisodes.Where(e =>
            (includeAllRegularSeasons && e.SeasonNumber > 0)
            || selectedSeasons.Contains(e.SeasonNumber)
            || (!string.IsNullOrWhiteSpace(e.ScopeName) && selectedScopeNames.Contains(e.ScopeName)));

        return OrderEpisodesForProvider(filtered, provider).ToList();
    }

    private static IEnumerable<TvdbEpisode> OrderEpisodesForProvider(IEnumerable<TvdbEpisode> episodes, string provider)
    {
        // TV-style providers preserve provider-native season numbering, with specials after regular seasons for rename flow.
        return episodes
            .OrderBy(e => e.SeasonNumber == 0 ? int.MaxValue : e.SeasonNumber)
            .ThenBy(e => e.EpisodeNumber)
            .ThenBy(e => e.Name, StringComparer.OrdinalIgnoreCase);
    }



    [RelayCommand]
    private async Task TestApiProviderConnection()
    {
        var provider = NormalizeLookupProvider(RenameLookupProvider);
        if (!ValidateRenameProviderConfigured(provider, log: false))
        {
            return;
        }

        _cts?.Cancel();
        _cts = new CancellationTokenSource();
        IsBusy = true;
        ApiProviderStatusText = $"Testing {provider} connection...";
        RenameStatusText = ApiProviderStatusText;

        try
        {
            var settings = BuildCurrentSettingsSnapshot();
            var metadataProvider = GetRenameMetadataProvider(provider);
            var results = await metadataProvider.SearchSeriesAsync("test", TvdbLanguage, settings, _cts.Token);
            ApiProviderStatusText = $"{provider} API connection successful. Results returned: {results.Count}";
            RenameStatusText = ApiProviderStatusText;
            RenameLog(ApiProviderStatusText);
        }
        catch (Exception ex)
        {
            ApiProviderStatusText = $"{provider} API connection failed: {ex.Message}";
            RenameStatusText = $"{provider} API connection failed";
            RenameLog(ApiProviderStatusText);
        }
        finally
        {
            IsBusy = false;
        }
    }

    private bool ValidateRenameProviderConfigured(string provider, bool log)
    {
        provider = NormalizeLookupProvider(provider);
        if (IsRenameProviderConfigured(provider))
        {
            return true;
        }

        var message = provider switch
        {
            "TMDB" => "TMDB API key is required. Add your own TMDB API key in Settings > API Providers.",
            _ => "TVDB API key is required. Add your own TVDB API key in Settings > API Providers."
        };

        ApiProviderStatusText = message;
        RenameStatusText = message;
        if (log) RenameLog(message);
        return false;
    }

    private bool IsRenameProviderConfigured(string? provider)
    {
        return NormalizeLookupProvider(provider) switch
        {
            "TMDB" => !string.IsNullOrWhiteSpace(TmdbApiKey),
            _ => !string.IsNullOrWhiteSpace(TvdbApiKey)
        };
    }

    private IRenameMetadataProvider GetRenameMetadataProvider(string provider)
    {
        var key = NormalizeLookupProvider(provider);
        return _renameMetadataProviders.TryGetValue(key, out var metadataProvider)
            ? metadataProvider
            : _renameMetadataProviders["TVDB"];
    }

    private AppSettings BuildCurrentSettingsSnapshot()
    {
        return new AppSettings
        {
            RootFolderPath = RootFolderPath?.Trim() ?? string.Empty,
            MkvToolNixDirectory = CrossPlatformRuntime.NormalizeUserPath(MkvToolNixDirectory),
            FfmpegDirectory = CrossPlatformRuntime.NormalizeUserPath(FfmpegDirectory),
            FfProbePath = string.Empty,
            TvdbApiKey = TvdbApiKey?.Trim() ?? string.Empty,
            TvdbPin = TvdbPin?.Trim() ?? string.Empty,
            TmdbApiKey = TmdbApiKey?.Trim() ?? string.Empty,
            TvdbLanguage = string.IsNullOrWhiteSpace(TvdbLanguage) ? "eng" : TvdbLanguage.Trim(),
            RenameLookupProvider = NormalizeLookupProvider(RenameLookupProvider),
            IgnoredScanFolderNames = ParseIgnoredScanFolderNames(IgnoredScanFolderNameText).ToList()
        };
    }

    private void ClearRenameProviderCache()
    {
        _cachedTvdbEpisodes.Clear();
        _cachedTvdbSeriesId = null;
        _cachedTvdbLanguage = string.Empty;
        _cachedLookupProvider = string.Empty;
    }

    private static string NormalizeLookupProvider(string? provider)
    {
        var value = (provider ?? string.Empty).Trim();
        if (value.Equals("TMDB", StringComparison.OrdinalIgnoreCase) || value.Equals("TheMovieDB", StringComparison.OrdinalIgnoreCase)) return "TMDB";
        return "TVDB";
    }

    [RelayCommand]
    private async Task ApplyRenamePreview()
    {
        var selected = RenameItems.Where(i => i.Selected)
            .OrderBy(i => i.Season ?? int.MaxValue)
            .ThenBy(i => i.Episode ?? int.MaxValue)
            .ThenBy(i => i.CurrentFileName, StringComparer.OrdinalIgnoreCase)
            .ToList();
        if (selected.Count == 0)
        {
            RenameLog("No rename rows selected.");
            return;
        }

        var jobByItem = selected.ToDictionary(
            item => item,
            item => CreateExecutionJob("mkvrename", item.FilePath, $"Rename to {item.NewFileName}"));
        BeginExecutionWorkflow("mkvrename", jobByItem.Values);

        var conflictChecks = selected
            .Where(item => !string.IsNullOrWhiteSpace(item.NewFileName))
            .Select(item =>
            {
                var directory = Path.GetDirectoryName(item.FilePath) ?? string.Empty;
                var targetPath = Path.Combine(directory, item.NewFileName);
                return new { Item = item, TargetPath = targetPath };
            })
            .Where(x => !string.Equals(x.Item.FilePath, x.TargetPath, StringComparison.OrdinalIgnoreCase))
            .Select(x => new ExecutionConflictCheck(jobByItem[x.Item], x.Item.FilePath, x.TargetPath, RenameCheck: true))
            .ToList();

        if (!await ConfirmOrCancelForConflictsAsync(conflictChecks, RenameLog))
        {
            RenameStatusText = "Rename canceled because conflicts were detected.";
            CompleteExecutionWorkflow(RenameStatusText);
            return;
        }

        var renamed = 0;
        var skipped = 0;
        var batchEntries = new List<RenameBatchEntry>();

        foreach (var item in selected)
        {
            var job = jobByItem[item];
            if (job.Status == ExecutionJobStatus.Skipped)
            {
                item.Status = "Skipped - file conflict";
                skipped++;
                continue;
            }

            _executionQueue.MarkRunning(job);
            RefreshExecutionSummary();

            if (string.IsNullOrWhiteSpace(item.NewFileName))
            {
                item.Status = "Skipped - no preview filename";
                _executionQueue.Skip(job, item.Status);
                skipped++;
                RefreshExecutionSummary();
                continue;
            }

            var directory = Path.GetDirectoryName(item.FilePath) ?? string.Empty;
            var targetPath = Path.Combine(directory, item.NewFileName);

            if (string.Equals(item.FilePath, targetPath, StringComparison.OrdinalIgnoreCase))
            {
                item.Status = "No change";
                _executionQueue.Skip(job, item.Status);
                skipped++;
                RefreshExecutionSummary();
                continue;
            }

            try
            {
                var oldPath = item.FilePath;
                var oldName = item.CurrentFileName;
                File.Move(oldPath, targetPath);
                RenameLog($"Renamed: {oldName} -> {Path.GetFileName(targetPath)}");
                var dashboardFile = Files.FirstOrDefault(f => string.Equals(f.FilePath, oldPath, StringComparison.OrdinalIgnoreCase));
                if (dashboardFile is not null)
                {
                    MoveDashboardCacheEntryAfterRename(dashboardFile, oldPath, targetPath);
                    AppState.UpdateDashboardFilePath(dashboardFile, targetPath);
                    dashboardFile.Status = "Renamed";
                }

                if (AppState.SelectedFile is not null && string.Equals(AppState.SelectedFile.FilePath, oldPath, StringComparison.OrdinalIgnoreCase))
                {
                    AppState.SelectedFile = dashboardFile;
                }

                if (string.Equals(PropEditTemplateFilePath, oldPath, StringComparison.OrdinalIgnoreCase))
                {
                    PropEditTemplateFilePath = targetPath;
                }

                item.FilePath = targetPath;
                item.RefreshFileName();
                item.Status = "Renamed";
                batchEntries.Add(new RenameBatchEntry
                {
                    OriginalPath = oldPath,
                    RenamedPath = targetPath
                });
                _executionQueue.Complete(job, $"Renamed to {Path.GetFileName(targetPath)}");
                renamed++;
            }
            catch (Exception ex)
            {
                item.Status = "Failed";
                RenameLog($"Rename failed for {item.CurrentFileName}: {ex.Message}");
                _executionQueue.Fail(job, ex.Message);
                skipped++;
            }

            RefreshExecutionSummary();
        }

        RenameStatusText = $"Rename complete: {renamed} renamed, {skipped} skipped";
        if (batchEntries.Count > 0)
        {
            _renameBatchHistory.RecordBatch(new RenameBatchRecord
            {
                CreatedAt = DateTime.Now,
                Provider = NormalizeLookupProvider(RenameLookupProvider),
                Template = RenameTemplate,
                TotalFiles = batchEntries.Count,
                Entries = batchEntries
            });
            RenameLog($"Recorded undo batch for {batchEntries.Count} renamed file(s).");
        }

        RenameLog(RenameStatusText);
        CompleteExecutionWorkflow(RenameStatusText);
        SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: false);
    }

    private RenameBatchUndoPreview PreviewRenameBatchUndo(RenameBatchRecord batch)
        => _renameBatchHistory.PreviewUndoBatch(batch);

    private Task<RenameBatchUndoResult> UndoRenameBatchAsync(RenameBatchRecord batch)
    {
        var result = _renameBatchHistory.UndoBatch(batch);
        RenameConsoleLines.Clear();
        foreach (var line in result.Lines)
        {
            RenameConsoleLines.Add(line);
        }

        RefreshDashboardPathsAfterRenameUndo(batch);
        RenameStatusText = $"Undo batch complete: {result.Renamed} restored, {result.Skipped} skipped";
        CompleteExecutionWorkflow(RenameStatusText);
        SyncRenameFromDashboardSelection(preserveSearchTitle: true, writeLog: false);
        return Task.FromResult(result);
    }

    private void RefreshDashboardPathsAfterRenameUndo(RenameBatchRecord batch)
    {
        foreach (var entry in batch.Entries)
        {
            if (!File.Exists(entry.OriginalPath) || File.Exists(entry.RenamedPath)) continue;

            var dashboardFile = Files.FirstOrDefault(file => string.Equals(file.FilePath, entry.RenamedPath, StringComparison.OrdinalIgnoreCase));
            if (dashboardFile is not null)
            {
                MoveDashboardCacheEntryAfterRename(dashboardFile, entry.RenamedPath, entry.OriginalPath);
                AppState.UpdateDashboardFilePath(dashboardFile, entry.OriginalPath);
                dashboardFile.Status = "Rename undone";
            }

            if (AppState.SelectedFile is not null && string.Equals(AppState.SelectedFile.FilePath, entry.RenamedPath, StringComparison.OrdinalIgnoreCase))
            {
                AppState.SelectedFile = dashboardFile;
            }

            if (string.Equals(PropEditTemplateFilePath, entry.RenamedPath, StringComparison.OrdinalIgnoreCase))
            {
                PropEditTemplateFilePath = entry.OriginalPath;
            }
        }
    }

    private void MoveDashboardCacheEntryAfterRename(MkvFileItem dashboardFile, string oldPath, string newPath)
    {
        var media = dashboardFile.ToMediaFile();
        media.FilePath = newPath;
        media.OriginalFileName = Path.GetFileName(newPath);

        _mediaCache.Remove(oldPath);
        _tempMediaCache.Remove(oldPath);

        if (IsPathUnderAnyWatchFolder(newPath)) _mediaCache.Upsert(media);
        else _tempMediaCache.Upsert(media);
    }

    private string BuildRenameFileName(RenamePreviewItem item, TvdbEpisode episode)
    {
        return RenameFileNameBuilder.Build(
            item.FilePath,
            item.SeriesTitle,
            item.SeriesYear,
            episode,
            RenameTemplate,
            IsSelectedMetadataResultMovie());
    }

    private bool IsSelectedMetadataResultMovie()
    {
        return SelectedTvdbSeries?.Format.Equals("Movie", StringComparison.OrdinalIgnoreCase) == true;
    }

    private string GuessRenameSearchTitle()
    {
        var candidates = RenameItems
            .Select(i => i.SeriesTitle)
            .Where(s => !string.IsNullOrWhiteSpace(s))
            .GroupBy(s => s, StringComparer.OrdinalIgnoreCase)
            .OrderByDescending(g => g.Count())
            .ThenBy(g => g.Key)
            .Select(g => g.Key)
            .ToList();

        if (candidates.Count > 0) return candidates[0];
        if (!string.IsNullOrWhiteSpace(FolderPath)) return CleanSeriesTitle(Path.GetFileName(FolderPath));
        return string.Empty;
    }

    private static RenameDetection DetectEpisodeInfo(string filePath)
    {
        var fileName = Path.GetFileNameWithoutExtension(filePath);
        var match = Regex.Match(fileName, @"(?<title>.*?)(?:[\s._\-\[]+)S(?<season>\d{1,3})\s*E(?<episode>\d{1,4})", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        if (match.Success)
        {
            return new RenameDetection(
                CleanSeriesTitle(match.Groups["title"].Value),
                ParseSortNumber(match.Groups["season"].Value, 0),
                ParseSortNumber(match.Groups["episode"].Value, 0),
                null);
        }

        var absolute = Regex.Match(fileName, @"(?<title>.*?)(?:[\s._\-\[]+)(?<episode>\d{1,4})(?:v\d+)?$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        if (absolute.Success)
        {
            var episodeNumber = ParseSortNumber(absolute.Groups["episode"].Value, 0);
            return new RenameDetection(CleanSeriesTitle(absolute.Groups["title"].Value), 1, episodeNumber, episodeNumber);
        }

        return new RenameDetection(CleanSeriesTitle(fileName), null, null, null);
    }

    private static string CleanSeriesTitle(string value)
    {
        value = value.Replace('.', ' ').Replace('_', ' ');
        value = Regex.Replace(value, @"\[[^\]]*\]|\([^\)]*\)", " ");
        value = Regex.Replace(value, @"\b(1080p|720p|2160p|480p|bluray|web[- ]?dl|webrip|x264|x265|hevc|avc|aac|flac|opus|10bit|8bit)\b", " ", RegexOptions.IgnoreCase);
        value = Regex.Replace(value, @"\s+", " ").Trim(' ', '-', '.');
        return value;
    }

    private sealed record RenameDetection(string SeriesTitle, int? Season, int? Episode, int? AbsoluteEpisode);
}
