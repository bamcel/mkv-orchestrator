namespace MKVOrchestrator.Core.Models;

public sealed class TvdbSeriesSearchResult
{
    public int Id { get; set; }
    public string Name { get; set; } = string.Empty;
    public string Year { get; set; } = string.Empty;
    public string Overview { get; set; } = string.Empty;
    public string Provider { get; set; } = "TVDB";
    public string Format { get; set; } = string.Empty;
    public string DatabaseUrl { get; set; } = string.Empty;
    public string DisplayName => string.IsNullOrWhiteSpace(Year) ? Name : $"{Name} ({Year})";
    public string ProviderDisplay => string.IsNullOrWhiteSpace(Format) ? Provider : $"{Provider} {Format}";

    public override string ToString() => DisplayName;
}

public sealed class TvdbEpisode
{
    public int Id { get; set; }
    public int SeasonNumber { get; set; }
    public int EpisodeNumber { get; set; }
    public string Name { get; set; } = string.Empty;
    public string Provider { get; set; } = "TVDB";

    // Provider-native grouping label used by mkvrename scope generation.
    // TVDB/TMDB use provider-native Season/Specials labels.
    public string ScopeName { get; set; } = string.Empty;
}


public sealed class TvdbSeasonScopeOption : System.ComponentModel.INotifyPropertyChanged
{
    private bool _isSelected;

    public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged;

    public string DisplayName { get; set; } = string.Empty;
    public int? SeasonNumber { get; set; }
    public string ScopeName { get; set; } = string.Empty;
    public bool IsAllOption { get; set; }
    public bool IsAllRegularSeasonsOption { get; set; }
    public bool IsSpecials => SeasonNumber == 0;

    public bool IsSelected
    {
        get => _isSelected;
        set
        {
            if (_isSelected == value) return;
            _isSelected = value;
            PropertyChanged?.Invoke(this, new System.ComponentModel.PropertyChangedEventArgs(nameof(IsSelected)));
        }
    }
}
