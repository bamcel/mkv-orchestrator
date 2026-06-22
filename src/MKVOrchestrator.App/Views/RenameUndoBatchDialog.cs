using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Templates;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using MKVOrchestrator.Core.Models;

namespace MKVOrchestrator.App.Views;

public sealed class RenameUndoBatchDialog : Window
{
    private readonly List<RenameBatchRecord> _batches;
    private readonly Action _clearBatches;
    private readonly Func<RenameBatchRecord, RenameBatchUndoPreview> _previewUndoBatch;
    private readonly Func<RenameBatchRecord, Task<RenameBatchUndoResult>> _undoBatchAsync;
    private readonly ListBox _batchList = new();
    private readonly ItemsControl _entryList = new();
    private readonly ItemsControl _summaryList = new();
    private readonly Button _undoButton = new();
    private string _pendingProceedBatchId = string.Empty;

    public RenameUndoBatchDialog(
        IReadOnlyList<RenameBatchRecord> batches,
        Action clearBatches,
        Func<RenameBatchRecord, RenameBatchUndoPreview> previewUndoBatch,
        Func<RenameBatchRecord, Task<RenameBatchUndoResult>> undoBatchAsync)
    {
        _batches = batches.ToList();
        _clearBatches = clearBatches;
        _previewUndoBatch = previewUndoBatch;
        _undoBatchAsync = undoBatchAsync;

        Title = "Undo Rename Batch";
        Width = 980;
        Height = 620;
        MinWidth = 820;
        MinHeight = 520;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        CanResize = true;
        SystemDecorations = SystemDecorations.None;
        Background = ResourceBrush("ThemeCardBackgroundBrush", "#282A36");
        Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2");

        Content = BuildContent();
        LoadBatches();
    }

    private Control BuildContent()
    {
        var root = new Border
        {
            BorderBrush = ResourceBrush("ThemeWindowBackgroundBrush", "#1E1F29"),
            BorderThickness = new Thickness(2),
            Child = new Grid
            {
                RowDefinitions = new RowDefinitions("36,*")
            }
        };

        var grid = (Grid)root.Child;

        var titleBar = BuildTitleBar();
        Grid.SetRow(titleBar, 0);
        grid.Children.Add(titleBar);

        var body = BuildDialogBody();
        Grid.SetRow(body, 1);
        grid.Children.Add(body);

        return root;
    }

    private Control BuildTitleBar()
    {
        var titleBar = new Border
        {
            Background = ResourceBrush("ThemeWindowBackgroundBrush", "#1E1F29"),
            BorderBrush = ResourceBrush("ThemeBorderBrush", "#343746"),
            BorderThickness = new Thickness(0, 0, 0, 1)
        };
        titleBar.PointerPressed += TitleBar_PointerPressed;

        var grid = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto"),
            Margin = new Thickness(10, 0, 0, 0)
        };

        var title = new TextBlock
        {
            Text = "Undo Rename Batch",
            Foreground = ResourceBrush("ThemeMutedTextBrush", "#CFCFEA"),
            FontSize = 12,
            VerticalAlignment = VerticalAlignment.Center
        };
        Grid.SetColumn(title, 0);
        grid.Children.Add(title);

        var tray = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 0
        };
        Grid.SetColumn(tray, 2);

        tray.Children.Add(BuildChromeButton("\uE921", MinimizeButton_Click));
        tray.Children.Add(BuildChromeButton("\uE922", MaximizeButton_Click));
        var closeButton = BuildChromeButton("\uE8BB", CloseButton_Click);
        closeButton.Classes.Add("close");
        tray.Children.Add(closeButton);

        grid.Children.Add(tray);
        titleBar.Child = grid;
        return titleBar;
    }

    private Control BuildDialogBody()
    {
        var root = new Grid
        {
            RowDefinitions = new RowDefinitions("Auto,*,Auto"),
            Margin = new Thickness(18),
            RowSpacing = 12
        };

        var heading = new StackPanel { Spacing = 4 };
        heading.Children.Add(new TextBlock
        {
            Text = "Undo Rename Batch",
            FontSize = 18,
            FontWeight = FontWeight.SemiBold
        });
        heading.Children.Add(new TextBlock
        {
            Text = "Select a previous rename batch, review the planned reverse moves, then undo. Files are skipped if the current renamed file is missing, locked, or the original path already exists.",
            TextWrapping = TextWrapping.Wrap,
            Opacity = 0.85
        });
        Grid.SetRow(heading, 0);
        root.Children.Add(heading);

        var body = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("300,*,310"),
            ColumnSpacing = 12
        };

        body.Children.Add(BuildSelectablePanel("Last 20 Batch Jobs", _batchList, 0));
        body.Children.Add(BuildReadOnlyPanel("Files To Restore", _entryList, 1));
        body.Children.Add(BuildReadOnlyPanel("Summary", _summaryList, 2));
        _entryList.ItemTemplate = BuildRestoreEntryTemplate();
        Grid.SetRow(body, 1);
        root.Children.Add(body);

        var footer = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto")
        };

        var actionButtons = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Right,
            Spacing = 10
        };

        var clearButton = new Button
        {
            Content = "Clear Batch List",
            MinWidth = 132
        };
        clearButton.Click += (_, _) => ClearBatchList();
        Grid.SetColumn(clearButton, 0);
        footer.Children.Add(clearButton);

        var closeButton = new Button
        {
            Content = "Close",
            Width = 96
        };
        closeButton.Click += (_, _) => Close();

        _undoButton.Content = "Undo Selected";
        _undoButton.MinWidth = 128;
        _undoButton.IsEnabled = false;
        _undoButton.Click += async (_, _) => await UndoSelectedAsync();

        actionButtons.Children.Add(_undoButton);
        actionButtons.Children.Add(closeButton);
        Grid.SetColumn(actionButtons, 2);
        footer.Children.Add(actionButtons);
        Grid.SetRow(footer, 2);
        root.Children.Add(footer);

        _batchList.SelectionChanged += (_, _) => ShowSelectedBatch();

        return root;
    }

    private Button BuildChromeButton(string icon, EventHandler<RoutedEventArgs> handler)
    {
        var button = new Button();
        button.Classes.Add("window-chrome-button");
        button.Click += handler;

        var text = new TextBlock { Text = icon };
        text.Classes.Add("window-chrome-icon");
        button.Content = text;
        return button;
    }

    private static Border BuildSelectablePanel(string title, ListBox list, int column)
    {
        var panel = BuildPanelShell(title, column, out var dock);

        ApplyListBoxChrome(list);
        list.ItemTemplate = BuildReadOnlyTextTemplate();
        dock.Children.Add(list);

        return panel;
    }

    private static Border BuildReadOnlyPanel(string title, ItemsControl list, int column)
    {
        var panel = BuildPanelShell(title, column, out var dock);

        list.FontFamily = new FontFamily("Consolas");
        list.FontSize = 12;
        list.Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2");
        list.ItemTemplate = BuildReadOnlyTextTemplate();

        var scrollViewer = new ScrollViewer
        {
            Background = ResourceBrush("ThemeInputBackgroundBrush", "#282A36"),
            BorderBrush = ResourceBrush("ThemeButtonBackgroundBrush", "#44475A"),
            BorderThickness = new Thickness(1),
            Padding = new Thickness(4),
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = list
        };
        dock.Children.Add(scrollViewer);

        return panel;
    }

    private static Border BuildPanelShell(string title, int column, out DockPanel dock)
    {
        var panel = new Border
        {
            Background = ResourceBrush("ThemePanelBackgroundBrush", "#2B2E3A"),
            BorderBrush = ResourceBrush("ThemeBorderStrongBrush", "#44475A"),
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(4),
            Padding = new Thickness(8)
        };

        dock = new DockPanel();
        dock.Children.Add(new TextBlock
        {
            Text = title,
            FontWeight = FontWeight.SemiBold,
            Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2"),
            Margin = new Thickness(0, 0, 0, 8)
        });
        DockPanel.SetDock(dock.Children[0], Dock.Top);

        panel.Child = dock;
        Grid.SetColumn(panel, column);
        return panel;
    }

    private static void ApplyListBoxChrome(ListBox list)
    {
        list.FontFamily = new FontFamily("Consolas");
        list.FontSize = 12;
        list.Background = ResourceBrush("ThemeInputBackgroundBrush", "#282A36");
        list.Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2");
        list.BorderBrush = ResourceBrush("ThemeButtonBackgroundBrush", "#44475A");
        list.BorderThickness = new Thickness(1);
        list.Padding = new Thickness(4);
    }

    private static FuncDataTemplate<string> BuildReadOnlyTextTemplate()
        => new((value, _) => new TextBlock
        {
            Text = value,
            TextWrapping = TextWrapping.Wrap,
            Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2"),
            FontFamily = new FontFamily("Consolas"),
            FontSize = 12,
            Margin = new Thickness(2, 1)
        }, true);

    private void LoadBatches()
    {
        _batchList.ItemsSource = _batches.Select(batch => batch.DisplayName).ToList();
        _summaryList.ItemsSource = _batches.Count == 0
            ? new[] { "No rename batches have been recorded yet." }
            : new[] { "Select a batch to review its restore plan." };
    }

    private void ClearBatchList()
    {
        _clearBatches();
        _batches.Clear();
        _pendingProceedBatchId = string.Empty;
        _undoButton.Content = "Undo Selected";
        _undoButton.IsEnabled = false;
        _entryList.ItemsSource = null;
        _summaryList.ItemsSource = new[] { "Rename undo batch history cleared." };
        LoadBatches();
    }

    private RenameBatchRecord? SelectedBatch
    {
        get
        {
            var index = _batchList.SelectedIndex;
            return index >= 0 && index < _batches.Count ? _batches[index] : null;
        }
    }

    private void ShowSelectedBatch()
    {
        var batch = SelectedBatch;
        _pendingProceedBatchId = string.Empty;
        _undoButton.Content = "Undo Selected";
        _undoButton.IsEnabled = batch is not null && !batch.IsUndone;

        if (batch is null)
        {
            _entryList.ItemsSource = null;
            _summaryList.ItemsSource = new[] { "Select a batch to review its restore plan." };
            return;
        }

        _entryList.ItemsSource = batch.Entries
            .Select((entry, index) => new RestoreEntryDisplay(
                index + 1,
                entry.RenamedFileName,
                entry.OriginalFileName))
            .ToList();

        _summaryList.ItemsSource = new[]
        {
            $"Created: {batch.CreatedAt:yyyy-MM-dd HH:mm:ss}",
            $"Provider: {EmptyAsNA(batch.Provider)}",
            $"Files: {batch.TotalFiles}",
            $"Status: {(batch.IsUndone ? "Already undone" : "Ready to undo")}"
        };
    }

    private async Task UndoSelectedAsync()
    {
        var batch = SelectedBatch;
        if (batch is null) return;

        var isProceedConfirmed = string.Equals(_pendingProceedBatchId, batch.Id, StringComparison.OrdinalIgnoreCase);
        if (!isProceedConfirmed)
        {
            var preview = _previewUndoBatch(batch);
            if (preview.HasSkippedFiles)
            {
                _summaryList.ItemsSource = preview.Lines
                    .Concat(new[]
                    {
                        string.Empty,
                        "Some files will be skipped. Click Proceed Anyway to restore the remaining files, or Close to cancel."
                    })
                    .ToList();
                _pendingProceedBatchId = batch.Id;
                _undoButton.Content = "Proceed Anyway";
                _undoButton.IsEnabled = preview.Restorable > 0;
                return;
            }
        }

        _undoButton.IsEnabled = false;
        _summaryList.ItemsSource = new[] { "Undo is running..." };

        var result = await _undoBatchAsync(batch);
        _summaryList.ItemsSource = result.Lines;
        _pendingProceedBatchId = string.Empty;
        _undoButton.Content = result.Skipped > 0 ? "Retry Undo" : "Undo Selected";
        _undoButton.IsEnabled = result.Skipped > 0;
    }

    private static string EmptyAsNA(string value)
        => string.IsNullOrWhiteSpace(value) ? "N/A" : value.Trim();

    private static FuncDataTemplate<RestoreEntryDisplay> BuildRestoreEntryTemplate()
        => new((item, _) =>
        {
            if (item is null) return new TextBlock();

            var container = new Border
            {
                BorderBrush = ResourceBrush("ThemeBorderBrush", "#343746"),
                BorderThickness = new Thickness(1),
                CornerRadius = new CornerRadius(4),
                Padding = new Thickness(8, 6),
                Margin = new Thickness(0, 0, 0, 8)
            };

            var stack = new StackPanel { Spacing = 3 };
            stack.Children.Add(new TextBlock
            {
                Text = $"{item.Number:00}",
                Foreground = ResourceBrush("ThemeMutedTextBrush", "#CFCFEA"),
                FontFamily = new FontFamily("Consolas"),
                FontSize = 11,
                FontWeight = FontWeight.SemiBold
            });
            stack.Children.Add(BuildRestoreLine("Current", item.CurrentFileName));
            stack.Children.Add(BuildRestoreLine("Restore To", item.RestoreFileName));

            container.Child = stack;
            return container;
        }, true);

    private static Grid BuildRestoreLine(string label, string value)
    {
        var grid = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("72,*"),
            ColumnSpacing = 6
        };

        var labelText = new TextBlock
        {
            Text = label,
            Foreground = ResourceBrush("ThemeMutedTextBrush", "#CFCFEA"),
            FontFamily = new FontFamily("Consolas"),
            FontSize = 11
        };
        Grid.SetColumn(labelText, 0);
        grid.Children.Add(labelText);

        var valueText = new TextBlock
        {
            Text = value,
            TextWrapping = TextWrapping.Wrap,
            Foreground = ResourceBrush("ThemeTextBrush", "#F8F8F2"),
            FontFamily = new FontFamily("Consolas"),
            FontSize = 12
        };
        Grid.SetColumn(valueText, 1);
        grid.Children.Add(valueText);

        return grid;
    }

    private sealed record RestoreEntryDisplay(int Number, string CurrentFileName, string RestoreFileName);

    private static IBrush ResourceBrush(string key, string fallback)
    {
        if (Application.Current?.Resources.TryGetResource(key, null, out var resource) == true && resource is IBrush brush)
        {
            return brush;
        }

        return Brush.Parse(fallback);
    }

    private void TitleBar_PointerPressed(object? sender, PointerPressedEventArgs e)
    {
        var point = e.GetCurrentPoint(this);
        if (!point.Properties.IsLeftButtonPressed) return;

        if (e.ClickCount == 2)
        {
            ToggleMaximized();
            return;
        }

        BeginMoveDrag(e);
    }

    private void MinimizeButton_Click(object? sender, RoutedEventArgs e)
    {
        WindowState = WindowState.Minimized;
    }

    private void MaximizeButton_Click(object? sender, RoutedEventArgs e)
    {
        ToggleMaximized();
    }

    private void CloseButton_Click(object? sender, RoutedEventArgs e)
    {
        Close();
    }

    private void ToggleMaximized()
    {
        WindowState = WindowState == WindowState.Maximized
            ? WindowState.Normal
            : WindowState.Maximized;
    }
}
