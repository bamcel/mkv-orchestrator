using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using MKVOrchestrator.App.Services;
using MKVOrchestrator.App.ViewModels;
using MKVOrchestrator.App.Views;

namespace MKVOrchestrator.App;

public partial class App : Application
{
    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            var window = new MainWindow();
            var interaction = new AvaloniaUserInteractionService(window);
            window.DataContext = new MainWindowViewModel(
                MainWindowViewModelDependencies.CreateDefault(interaction));
            desktop.MainWindow = window;
        }
        base.OnFrameworkInitializationCompleted();
    }
}
