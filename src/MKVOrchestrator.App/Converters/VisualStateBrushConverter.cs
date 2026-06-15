using System;
using System.Globalization;
using Avalonia;
using Avalonia.Data.Converters;
using Avalonia.Media;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.App.Converters;

public sealed class VisualStateBrushConverter : IValueConverter
{
    public IBrush NormalBrush { get; set; } = Brush.Parse("#F8F8F2");
    public IBrush WarningBrush { get; set; } = Brushes.Orange;
    public IBrush TemplateBrush { get; set; } = Brush.Parse("#CFCFEA");

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        var normalBrush = ResourceBrush("ThemeTextBrush", NormalBrush);
        var warningBrush = ResourceBrush("ThemeWarningBrush", WarningBrush);
        var templateBrush = ResourceBrush("ThemeMutedTextBrush", TemplateBrush);

        if (parameter is string mode && mode.Equals("TemplateOnly", StringComparison.OrdinalIgnoreCase))
        {
            return value is VisualState.Template ? templateBrush : normalBrush;
        }

        return value switch
        {
            VisualState.Warning => warningBrush,
            VisualState.Template => templateBrush,
            _ => normalBrush
        };
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        throw new NotSupportedException();
    }

    private static IBrush ResourceBrush(string key, IBrush fallback)
    {
        if (Application.Current?.Resources.TryGetResource(key, null, out var resource) == true && resource is IBrush brush)
        {
            return brush;
        }

        return fallback;
    }
}
