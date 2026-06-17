using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using MKVOrchestrator.App.ViewModels;

namespace MKVOrchestrator.App.Views;

public partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        LoadWindowIcon();
        DataContextChanged += (_, _) => ConfigureViewModel();
        Opened += async (_, _) =>
        {
            if (DataContext is MainWindowViewModel vm)
            {
                await vm.InitializeAfterUiReadyAsync();
            }
        };
    }

    private void ConfigureViewModel()
    {
        if (DataContext is not MainWindowViewModel vm) return;

        vm.ConfirmSkipConflictsAsync = async conflicts =>
        {
            var dialog = new ExecutionConflictDialog(conflicts);
            var result = await dialog.ShowDialog<bool?>(this);
            return result == true;
        };

        vm.ShowOutputWindow = (title, lines) =>
        {
            var dialog = new OutputWindow(title, lines);
            _ = dialog.ShowDialog(this);
        };
    }

    private void LoadWindowIcon()
    {
        var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "MKVO_icon.ico");
        if (!File.Exists(iconPath)) return;

        using var stream = File.OpenRead(iconPath);
        Icon = new WindowIcon(stream);
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
