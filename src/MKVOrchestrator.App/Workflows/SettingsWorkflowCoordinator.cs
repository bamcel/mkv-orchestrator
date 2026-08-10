using MKVOrchestrator.Core.Models;
using MKVOrchestrator.Core.Services;

namespace MKVOrchestrator.App.Workflows;

public sealed class SettingsWorkflowCoordinator(AppSettingsService settingsService)
{
    public string SettingsPath => settingsService.SettingsPath;
    public AppSettings Load() => settingsService.Load();
    public void Save(AppSettings settings) => settingsService.Save(settings);
}
