# MKVO Migration Baseline

Captured: 2026-08-04  
Purpose: establish the behavior that the staged Tauri/Rust replacement must
match before the .NET implementation can be retired.

## Build baseline

```powershell
dotnet build MKVOrchestrator.sln --configuration Release
```

Result: **passed**, 0 warnings and 0 errors.

## Existing behavior harness

```powershell
dotnet run --project tests/MKVOrchestrator.Tests/MKVOrchestrator.Tests.csproj `
  --configuration Release --no-build
```

Result: **22/22 checks passed**.

Covered behavior:

- action and property-edit planning;
- rename sanitization, conflicts, token replacement, movie templates, specials,
  absolute numbering, and episode matching;
- natural sorting and cross-platform path handling;
- media-type and codec normalization;
- MKV-first scanning behavior;
- subtitle sidecar muxing;
- MP4 read-only handling and lossless conversion planning;
- application-state selection signals;
- execution-queue lifecycle ownership.

## Frontend baseline

```powershell
npm.cmd --prefix web run build
```

Result: **passed** after the transport-neutral `BackendClient` migration. The
HTTP browser client remains the default outside a Tauri runtime, and Tauri
modules are emitted as lazy chunks.

## Rust baseline

Dependency resolution produced `Cargo.lock`. The first partial check found and
fixed an ordering constraint on `MetadataProvider`; the foundational domain,
contracts, process, and watcher crates then passed `cargo check`. Full workspace
results are tracked in `MIGRATION_STATUS.md` as stages land.

