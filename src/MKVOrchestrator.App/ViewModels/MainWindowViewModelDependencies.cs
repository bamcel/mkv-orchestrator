using MKVOrchestrator.App.Services;
using MKVOrchestrator.App.Workflows;
using MKVOrchestrator.Core.Services;
using MKVOrchestrator.Core.Services.Audit;
using MKVOrchestrator.Core.Services.Cache;
using MKVOrchestrator.Core.Services.Library;
using MKVOrchestrator.Core.Services.Metadata;
using MKVOrchestrator.Core.Services.Operations;
using MKVOrchestrator.Core.Services.Pipeline;
using MKVOrchestrator.Core.Services.State;

namespace MKVOrchestrator.App.ViewModels;

public sealed class MainWindowViewModelDependencies
{
    public required MkvScannerService Scanner { get; init; }
    public required MkvScannerService TempScanner { get; init; }
    public required MetadataCacheServiceAdapter MediaCache { get; init; }
    public required MetadataCacheServiceAdapter TempMediaCache { get; init; }
    public required MediaLibraryService MediaLibrary { get; init; }
    public required MediaLibraryService TempMediaLibrary { get; init; }
    public required LibraryAuditService LibraryAudit { get; init; }
    public required MkvMergeService MkvMerge { get; init; }
    public required ScanPipeline ScanPipeline { get; init; }
    public required ScanPipeline TempScanPipeline { get; init; }
    public required AppStateService AppState { get; init; }
    public required ActionPlanner Planner { get; init; }
    public required MkvPropEditService PropEdit { get; init; }
    public required MkvPropEditCommandBuilder PropEditCommandBuilder { get; init; }
    public required GlobalOperationStatusService OperationStatus { get; init; }
    public required SettingsWorkflowCoordinator SettingsWorkflow { get; init; }
    public required ExecutionWorkflowCoordinator ExecutionWorkflow { get; init; }
    public required RenameBatchHistoryService RenameBatchHistory { get; init; }
    public required RenameMetadataCoordinator RenameMetadata { get; init; }
    public required WatchFolderCoordinator WatchFolders { get; init; }
    public required MediaServerDiscoveryService MediaServerDiscovery { get; init; }
    public required IUserInteractionService UserInteraction { get; init; }

    public static MainWindowViewModelDependencies CreateDefault(IUserInteractionService? userInteraction = null)
    {
        var scanner = new MkvScannerService(new MetadataCacheDatabase("metadata_cache.db"));
        var tempScanner = new MkvScannerService(new MetadataCacheDatabase("metadata_cache_temp.db"));
        var mediaCache = new MetadataCacheServiceAdapter(scanner.Cache);
        var tempMediaCache = new MetadataCacheServiceAdapter(tempScanner.Cache);
        var mediaLibrary = new MediaLibraryService(new MkvScannerServiceAdapter(scanner), mediaCache);
        var tempMediaLibrary = new MediaLibraryService(new MkvScannerServiceAdapter(tempScanner), tempMediaCache);
        var interaction = userInteraction ?? NullUserInteractionService.Instance;
        var providers = new Dictionary<string, IRenameMetadataProvider>(StringComparer.OrdinalIgnoreCase)
        {
            ["TVDB"] = new TvdbRenameMetadataProvider(),
            ["TMDB"] = new TmdbRenameMetadataProvider()
        };

        return new MainWindowViewModelDependencies
        {
            Scanner = scanner,
            TempScanner = tempScanner,
            MediaCache = mediaCache,
            TempMediaCache = tempMediaCache,
            MediaLibrary = mediaLibrary,
            TempMediaLibrary = tempMediaLibrary,
            LibraryAudit = new LibraryAuditService(mediaCache),
            MkvMerge = new MkvMergeService(),
            ScanPipeline = new ScanPipeline(mediaLibrary),
            TempScanPipeline = new ScanPipeline(tempMediaLibrary),
            AppState = new AppStateService(),
            Planner = new ActionPlanner(),
            PropEdit = new MkvPropEditService(),
            PropEditCommandBuilder = new MkvPropEditCommandBuilder(),
            OperationStatus = new GlobalOperationStatusService(),
            SettingsWorkflow = new SettingsWorkflowCoordinator(new AppSettingsService()),
            ExecutionWorkflow = new ExecutionWorkflowCoordinator(
                new ExecutionQueueService(),
                new FileConflictService(),
                interaction),
            RenameBatchHistory = new RenameBatchHistoryService(),
            RenameMetadata = new RenameMetadataCoordinator(providers),
            WatchFolders = new WatchFolderCoordinator(),
            MediaServerDiscovery = new MediaServerDiscoveryService(),
            UserInteraction = interaction
        };
    }
}
