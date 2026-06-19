using Avalonia.Controls;
using Avalonia.Input;
using MKVOrchestrator.App.ViewModels;

namespace MKVOrchestrator.App.Views.Rename;

public partial class RenamePanel : UserControl
{
    public RenamePanel()
    {
        InitializeComponent();
    }

    private void DatabaseUrlText_PointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (DataContext is not MainWindowViewModel viewModel) return;

        var properties = e.GetCurrentPoint(this).Properties;
        if (properties.IsLeftButtonPressed)
        {
            viewModel.OpenSelectedDatabaseUrlCommand.Execute(null);
            e.Handled = true;
            return;
        }

        if (properties.IsRightButtonPressed && TopLevel.GetTopLevel(this) is Window window)
        {
            viewModel.CopySelectedDatabaseUrlCommand.Execute(window);
            e.Handled = true;
        }
    }
}
