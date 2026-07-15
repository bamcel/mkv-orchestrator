# MKV Orchestrator

MKV Orchestrator, or MKVO, is a desktop media operations console for scanning media folders, reviewing track metadata, matching rename metadata from TVDB or TMDB, previewing safe file renames, planning mux/remux work, and editing MKV track properties.

The app is built with Avalonia and is currently maintained as a desktop application.

A Docker web build is also available for server or NAS-style access. The desktop app remains the primary product, while the web container is maintained as a single-container companion that shares MKVO core processing logic.

## What MKVO Does

- Scans folders for MKV and MP4 files.
- Displays file, video, audio, and subtitle track details.
- Compares files against a selected template file.
- Looks up TV or movie metadata from TVDB or TMDB for rename previews.
- Supports rename templates for TV episodes and movies.
- Records rename batches so recent rename operations can be reviewed and undone.
- Plans MKV mux/remux operations with MKVToolNix.
- Muxes matching external subtitle sidecars into MKV files.
- Edits MKV container title, track names, languages, default flags, and forced flags.
- Builds and manages a local metadata cache for watch folders.
- Supports user-editable GUI themes.

## Requirements

### Required For Running From Source

- Windows desktop environment.
- .NET 10 SDK.
- Git, if you are cloning from GitHub.

### Required Media Tools

Install these separately. MKVO does not bundle them.

#### MKVToolNix

MKVToolNix is required for MKV analysis, remuxing, extraction, and metadata editing.

MKVO expects access to these executables:

- `mkvmerge`
- `mkvpropedit`
- `mkvextract`
- `mkvinfo`

On Windows, install MKVToolNix from:

https://mkvtoolnix.download/

Then configure the install folder in:

```text
Settings > General > MKVToolNix Paths
```

Use the folder that contains the tools, for example:

```text
C:\Program Files\MKVToolNix
```

You can also use **Auto Find** if MKVToolNix is installed in a common location or available on PATH.

#### FFmpeg And ffprobe

FFmpeg and ffprobe are used for additional media inspection and MP4 readability support.

MKVO expects access to:

- `ffmpeg`
- `ffprobe`

Install FFmpeg from:

https://ffmpeg.org/

Then configure the FFmpeg `bin` folder in:

```text
Settings > General > FFmpeg Directory
```

Example:

```text
C:\ffmpeg\bin
```

You can also use **Auto Find** if FFmpeg is installed in a common location or available on PATH.

## Metadata Provider API Keys

MKVO does not ship shared TVDB or TMDB production API keys.

Each user must provide their own API credentials for rename metadata lookup.

Configure provider credentials in:

```text
Settings > API Providers
```

Supported providers:

- TVDB
- TMDB

TVDB is used for TV and movie metadata lookup through TheTVDB.

TMDB is used for TV and movie metadata lookup through The Movie Database.

The app masks key fields, stores keys locally in the user settings file, and does not write API keys to logs.

Provider setup links are shown inside the app under Settings.

## First-Run Setup

1. Install MKVToolNix.
2. Install FFmpeg.
3. Open MKVO.
4. Go to `Settings > General`.
5. Set the MKVToolNix folder or click **Auto Find**.
6. Set the FFmpeg folder or click **Auto Find**.
7. Go to `Settings > API Providers`.
8. Enter your own TVDB and/or TMDB API key.
9. Click **Test Selected Provider** to confirm lookup access.
10. Go to Dashboard and scan one or more folders.

## Rename Workflow

1. Scan files from the Dashboard.
2. Go to Rename Files.
3. Search for the show or movie title.
4. Select the correct TVDB or TMDB result.
5. Confirm the episode scope or movie mode.
6. Choose a naming template.
7. Click **Preview**.
8. Review the Rename Preview table and Preview Summary.
9. Click **Apply** only when the preview is correct.

Rename batches are recorded locally. Use **Undo Batch** in Rename Options to review recent rename jobs and restore files when possible.

## Subtitle Mux Filename Format

External subtitle sidecars should be placed in the same folder as the matching MKV file.

Expected format:

```text
base_name.language.tag.ext
```

Example:

```text
Episode 01.mkv
Episode 01.eng.Dialogue.ass
Episode 01.eng.Signs & Songs.ass
Episode 01.jpn.Dialogue.ass
```

The language token is read from the filename. The tag token becomes the subtitle track name.

## Local Data And Privacy

MKVO stores user settings, local metadata cache files, and rename history locally on the machine.

Do not commit or publish local runtime files such as:

- API keys
- `settings.json`
- `metadata_cache*.db`
- local logs
- local publish output

The repository `.gitignore` excludes the common local runtime files.

## Build From Source

Restore and build:

```powershell
dotnet build MKVOrchestrator.sln
```

Run the desktop app:

```powershell
dotnet run --project src\MKVOrchestrator.App\MKVOrchestrator.App.csproj
```

Run the test harness:

```powershell
dotnet run --project tests\MKVOrchestrator.Tests\MKVOrchestrator.Tests.csproj
```

## Docker Web Container

The Docker build runs as one container. It serves the React web UI and ASP.NET Core API from the same process and installs MKVToolNix plus FFmpeg inside the image.

Build and run:

```powershell
docker compose up --build
```

Open:

```text
http://localhost:8080
```

Default local volume mounts:

```text
./tmp/docker-media     -> /media
./tmp/docker-downloads -> /downloads
./tmp/docker-config    -> /config
```

The web app browses container paths. With the default compose file, `/media` and `/downloads` are local bind mounts under `./tmp`.

For a NAS or SMB share, copy `.env.example` to `.env`, edit the share paths and CIFS options, then run:

```powershell
docker compose -f docker-compose.yml -f docker-compose.nas.example.yml up --build
```

Keep `.env` local. Do not commit API keys, SMB usernames, SMB passwords, or server-specific paths.

Optional container settings (see `docs/DOCKER_WEB_CONTAINER.md` for the full list):

- `PUID` / `PGID` / `UMASK` run the app as a specific user so files written to shares are not root-owned.
- `MKVO_AUTH_USERNAME` / `MKVO_AUTH_PASSWORD` enable HTTP basic auth for the web UI and API.
- `MKVO_SCAN_WORKERS` and `MKVO_EDIT_WORKERS` tune scan and mkvpropedit concurrency.

The web container wires Dashboard, Rename, Mux / Remux, Track Properties, Library, Settings, and Logs through the single ASP.NET Core host. The web UI and desktop app should continue sharing processing behavior through `MKVOrchestrator.Core`.

Publish a Windows test build:

```powershell
dotnet publish src\MKVOrchestrator.App\MKVOrchestrator.App.csproj -c Release -r win-x64 --self-contained true -o .\artifacts\publish\MKVO
```

Adjust the output folder as needed.

## Documentation

Additional notes are available in:

- `docs/API_PROVIDER_KEYS.md`
- `docs/ATTRIBUTION_AND_LOGOS.md`
- `docs/DOCKER_WEB_CONTAINER.md`
- `docs/VERSIONING_AND_MIGRATIONS.md`

## Attribution

MKVO uses external tools and metadata providers selected or configured by the user.

- MKVToolNix is used for MKV analysis, remuxing, extraction, and metadata editing.
- FFmpeg and ffprobe are used for media metadata inspection.
- This product uses the TMDB API but is not endorsed or certified by TMDB.
- Metadata may be provided by TheTVDB.
