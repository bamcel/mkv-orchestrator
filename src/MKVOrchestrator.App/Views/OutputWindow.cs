using System.Collections.Generic;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Layout;
using Avalonia.Media;

namespace MKVOrchestrator.App.Views;

public sealed class OutputWindow : Window
{
    public OutputWindow(string title, IReadOnlyList<string> lines)
    {
        Title = title;
        Width = 980;
        Height = 680;
        MinWidth = 760;
        MinHeight = 420;
        Background = ResourceBrush("ThemeCardBackgroundBrush", "#282A36");
        Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2");

        var header = new TextBlock
        {
            Text = title,
            FontSize = 18,
            FontWeight = FontWeight.SemiBold,
            Margin = new Thickness(0, 0, 0, 10)
        };

        var output = new TextBox
        {
            Text = string.Join(System.Environment.NewLine, lines),
            IsReadOnly = true,
            AcceptsReturn = true,
            TextWrapping = TextWrapping.NoWrap,
            FontFamily = new FontFamily("Consolas"),
            FontSize = 12,
            Background = ResourceBrush("ThemeInputBackgroundBrush", "#21222C"),
            Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2"),
            BorderBrush = ResourceBrush("ThemeButtonBackgroundBrush", "#44475A"),
            BorderThickness = new Thickness(1),
            Padding = new Thickness(10)
        };

        var outputHost = new ScrollViewer
        {
            Content = output
        };

        var closeButton = new Button
        {
            Content = "Close",
            HorizontalAlignment = HorizontalAlignment.Right,
            MinWidth = 120,
            Margin = new Thickness(0, 10, 0, 0)
        };
        closeButton.Click += (_, _) => Close();

        Content = new Grid
        {
            Margin = new Thickness(14),
            RowDefinitions = new RowDefinitions("Auto,*,Auto"),
            Children =
            {
                header,
                outputHost,
                closeButton
            }
        };

        Grid.SetRow(header, 0);
        Grid.SetRow(outputHost, 1);
        Grid.SetRow(closeButton, 2);
    }

    private static IBrush ResourceBrush(string key, string fallback)
    {
        if (Application.Current?.Resources.TryGetResource(key, null, out var resource) == true && resource is IBrush brush)
        {
            return brush;
        }

        return Brush.Parse(fallback);
    }
}
