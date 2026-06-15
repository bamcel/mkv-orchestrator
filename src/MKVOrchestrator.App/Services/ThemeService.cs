using Avalonia;
using Avalonia.Media;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.App.Services;

public static class ThemeService
{
    public static IReadOnlyList<ThemeDefinition> BuiltInThemes { get; } = new[]
    {
        Create("Midnight", new()
        {
            ["WindowBackground"] = "#1E1F29",
            ["CardBackground"] = "#282A36",
            ["PanelBackground"] = "#2B2E3A",
            ["SidebarBackground"] = "#232631",
            ["InputBackground"] = "#282A36",
            ["InputHoverBackground"] = "#2F3140",
            ["ButtonBackground"] = "#44475A",
            ["ButtonHoverBackground"] = "#BD93F9",
            ["SelectedBackground"] = "#3A3D4F",
            ["Border"] = "#343746",
            ["BorderStrong"] = "#44475A",
            ["Text"] = "#F8F8F2",
            ["MutedText"] = "#CFCFEA",
            ["SubtleText"] = "#8B93A7",
            ["Accent"] = "#BD93F9",
            ["AppTitle"] = "#BD93F9",
            ["Success"] = "#50FA7B",
            ["Warning"] = "#FFA500",
            ["DisabledText"] = "#6272A4"
        }),
        Create("Dark", new()
        {
            ["WindowBackground"] = "#15171C",
            ["CardBackground"] = "#20232A",
            ["PanelBackground"] = "#252932",
            ["SidebarBackground"] = "#1B1E25",
            ["InputBackground"] = "#1D2028",
            ["InputHoverBackground"] = "#292E38",
            ["ButtonBackground"] = "#3B4252",
            ["ButtonHoverBackground"] = "#88C0D0",
            ["SelectedBackground"] = "#2E3440",
            ["Border"] = "#3B4252",
            ["BorderStrong"] = "#4C566A",
            ["Text"] = "#ECEFF4",
            ["MutedText"] = "#D8DEE9",
            ["SubtleText"] = "#A7B0C0",
            ["Accent"] = "#BD93F9",
            ["AppTitle"] = "#BD93F9",
            ["Success"] = "#50FA7B",
            ["Warning"] = "#EBCB8B",
            ["DisabledText"] = "#7D8797"
        }),
        Create("Light", new()
        {
            ["WindowBackground"] = "#F5F6FA",
            ["CardBackground"] = "#E8ECF4",
            ["PanelBackground"] = "#EEF1F7",
            ["SidebarBackground"] = "#E8ECF4",
            ["InputBackground"] = "#E8ECF4",
            ["InputHoverBackground"] = "#F1F4FA",
            ["ButtonBackground"] = "#DCE3EF",
            ["ButtonHoverBackground"] = "#6D5BD0",
            ["SelectedBackground"] = "#D9DDF0",
            ["Border"] = "#CAD2E0",
            ["BorderStrong"] = "#9DA8BA",
            ["Text"] = "#1C2430",
            ["MutedText"] = "#46556A",
            ["SubtleText"] = "#66758A",
            ["Accent"] = "#6D5BD0",
            ["AppTitle"] = "#6D5BD0",
            ["Success"] = "#17803D",
            ["Warning"] = "#A15C00",
            ["DisabledText"] = "#8792A3"
        })
    };

    public static ThemeDefinition GetTheme(string? name, IEnumerable<ThemeDefinition>? customThemes)
    {
        var all = BuiltInThemes.Concat(customThemes ?? Enumerable.Empty<ThemeDefinition>());
        var theme = Clone(all.FirstOrDefault(theme => string.Equals(theme.Name, name, StringComparison.OrdinalIgnoreCase))
                          ?? BuiltInThemes[0]);
        theme.Colors = BuildEffectiveColors(theme);
        return theme;
    }

    public static bool IsBuiltInTheme(string? name)
        => BuiltInThemes.Any(theme => string.Equals(theme.Name, name, StringComparison.OrdinalIgnoreCase));

    public static ThemeDefinition Clone(ThemeDefinition theme)
        => new()
        {
            Name = theme.Name,
            Colors = new Dictionary<string, string>(theme.Colors, StringComparer.OrdinalIgnoreCase)
        };

    public static void Apply(ThemeDefinition theme)
    {
        if (Application.Current is null) return;

        var colors = BuildEffectiveColors(theme);
        foreach (var (name, colorText) in colors)
        {
            if (!Color.TryParse(colorText, out var color)) continue;
            SetBrush($"Theme{name}Brush", color);
            Application.Current.Resources[$"Theme{name}Color"] = color;
        }

        SetAlias("ThemeForegroundBrush", "Text");
        SetAlias("SystemControlForegroundBaseHighBrush", "Text");
        SetAlias("SystemControlForegroundBaseMediumHighBrush", "Text");
        SetAlias("SystemControlForegroundBaseMediumBrush", "Text");
        SetAlias("SystemControlForegroundBaseLowBrush", "MutedText");
        SetAlias("SystemControlForegroundChromeHighBrush", "Text");
        SetAlias("SystemControlForegroundChromeBlackHighBrush", "Text");
        SetAlias("SystemControlForegroundChromeWhiteBrush", "Text");
        SetAlias("SystemControlHighlightAltBaseHighBrush", "Text");
        SetAlias("SystemControlHighlightAltBaseMediumHighBrush", "Text");
        SetAlias("SystemControlHighlightAltBaseMediumLowBrush", "Text");
        SetAlias("SystemControlHighlightAltChromeWhiteBrush", "Text");
        SetAlias("DataGridRowSelectedBackgroundBrush", "SelectedBackground");
        SetAlias("DataGridRowSelectedUnfocusedBackgroundBrush", "SelectedBackground");
        SetAlias("DataGridCellSelectedBackgroundBrush", "SelectedBackground");
        SetAlias("DataGridCellSelectedUnfocusedBackgroundBrush", "SelectedBackground");
        SetAlias("DataGridRowSelectedForegroundBrush", "Text");
        SetAlias("DataGridRowSelectedUnfocusedForegroundBrush", "Text");
        SetAlias("DataGridCellSelectedForegroundBrush", "Text");
        SetAlias("DataGridCellSelectedUnfocusedForegroundBrush", "Text");
        SetAlias("DataGridColumnHeaderForegroundBrush", "Text");
        SetAlias("DataGridRowForegroundBrush", "Text");
        SetAlias("DataGridCellForegroundBrush", "Text");
        SetAlias("SystemControlHighlightAccentBrush", "SelectedBackground");
        SetAlias("SystemControlHighlightListAccentLowBrush", "SelectedBackground");
        SetAlias("SystemControlHighlightListAccentMediumBrush", "SelectedBackground");
        SetAlias("SystemControlHighlightListAccentHighBrush", "SelectedBackground");

        if (theme.Colors.TryGetValue("ButtonBackground", out var accentColor) && Color.TryParse(accentColor, out var accent))
        {
            Application.Current.Resources["SystemAccentColor"] = accent;
            SetBrush("SystemAccentColorBrush", accent);
            SetBrush("AccentFillColorDefaultBrush", accent);
        }
    }

    private static void SetBrush(string resourceKey, Color color)
    {
        if (Application.Current is null) return;

        if (Application.Current.Resources.TryGetResource(resourceKey, null, out var resource) &&
            resource is SolidColorBrush brush)
        {
            brush.Color = color;
            return;
        }

        Application.Current.Resources[resourceKey] = new SolidColorBrush(color);
    }

    private static void SetAlias(string resourceKey, string colorName)
    {
        if (Application.Current is null) return;
        if (!Application.Current.Resources.TryGetResource($"Theme{colorName}Brush", null, out var brush)) return;
        Application.Current.Resources[resourceKey] = brush;
    }

    private static Dictionary<string, string> BuildEffectiveColors(ThemeDefinition theme)
    {
        var colors = new Dictionary<string, string>(BuiltInThemes[0].Colors, StringComparer.OrdinalIgnoreCase);
        foreach (var (key, value) in theme.Colors)
        {
            colors[key] = value;
        }

        if (!colors.ContainsKey("AppTitle") && colors.TryGetValue("Accent", out var accent))
        {
            colors["AppTitle"] = accent;
        }

        return colors;
    }

    private static ThemeDefinition Create(string name, Dictionary<string, string> colors)
        => new() { Name = name, Colors = colors };
}
