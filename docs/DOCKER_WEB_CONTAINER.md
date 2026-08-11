# MKVO Docker Web Container

The Docker web build runs MKV Orchestrator as one container:

- The Rust/Axum server hosts the API and serves the web app.
- React, TypeScript, Vite, Tailwind CSS, TanStack Query, React Router, and lucide-react provide the browser UI.
- MKVToolNix and FFmpeg are installed inside the runtime image.

## Run

```powershell
docker compose up --build
```

Open:

```text
http://localhost:8886
```

## Volumes

The default compose file uses local bind mounts:

```text
./tmp/docker-media     -> /media
./tmp/docker-downloads -> /downloads
./tmp/docker-config    -> /config
```

The web app browses container paths. With the default compose file, `/media` and `/downloads` point at local folders under `./tmp`.

Set these values in a local `.env` file to customize bind mounts:

```text
MKVO_MEDIA_PATH=D:/Media
MKVO_DOWNLOADS_PATH=D:/Downloads/Media
MKVO_CONFIG_PATH=./tmp/docker-config
MKVO_SOURCE_ROOTS=downloads=/downloads
```

## Environment Variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `MKVO_MEDIA_ROOT` | `/media` | Default root shown by the Dashboard browser. |
| `MKVO_SOURCE_ROOTS` | `downloads=/downloads` | Additional named source roots (`name=/path`, comma separated). |
| `MKVO_SCAN_WORKERS` | `6` | Maximum concurrent metadata scans (clamped 1-8). |
| `MKVO_EDIT_WORKERS` | `2` | Maximum concurrent mkvpropedit edits (clamped 1-6). |
| `PUID` / `PGID` | `0` / `0` | Run the app as this user/group so files written to shares are not root-owned. Unraid users typically want `99` / `100`. |
| `UMASK` | unset | File creation mask applied at startup. |
| `MKVO_AUTH_MODE` | `disabled` in the supplied container definitions; `auto` in the server | `disabled` allows trusted-LAN access without a login. `basic` requires credentials. `auto` refuses an unauthenticated non-loopback bind. |
| `MKVO_AUTH_USERNAME` / `MKVO_AUTH_PASSWORD` | unset | Required together when `MKVO_AUTH_MODE=basic`. The browser shows a native login prompt. `/api/health` stays open for the container healthcheck. |
| `MKVO_TVDB_API_KEY`, `MKVO_TVDB_PIN`, `MKVO_TMDB_API_KEY` | unset | Optional provider credentials; Settings-page values take effect otherwise. |

## Security Notes

- Filesystem browsing through the web UI is limited to the configured source
  roots, watch folders, and enabled media-server library paths. Arbitrary
  container paths are rejected.
- The supplied Docker and Unraid configurations use `MKVO_AUTH_MODE=disabled`
  for convenient trusted-LAN access. Switch to `basic`, set both credentials,
  and use HTTPS or an authenticating reverse proxy before exposing it beyond a
  trusted network.

## Long-Running Operations

Mux/remux and track property applies run as background jobs inside the
container. The web UI polls job progress (per-file percent for remuxes), can
cancel a running job, and survives page refreshes without dropping the
operation. Results are also written to the Logs page.

## Watch Folders

Watch folders configured in Settings are monitored live inside the container
when **Enable live watch-folder monitoring** is on: new or changed media files
are re-scanned into the metadata cache automatically and deleted files are
pruned. Cache entries not refreshed for 30 days are removed at startup.

## NAS / SMB Shares

For a NAS share, copy `.env.example` to `.env`, edit the share paths, then run Docker Compose with the NAS example override:

```powershell
docker compose -f docker-compose.yml -f docker-compose.nas.example.yml up --build
```

The NAS example uses Docker's local CIFS volume driver:

```yaml
volumes:
  mkvo-media:
    driver: local
    driver_opts:
      type: cifs
      device: ${MKVO_MEDIA_SHARE}
      o: ${MKVO_CIFS_OPTIONS}
```

If your share requires credentials, put them only in your local `.env`.

Example:

```text
MKVO_MEDIA_SHARE=//server/media
MKVO_DOWNLOADS_SHARE=//server/downloads/media
MKVO_CIFS_OPTIONS=username=myuser,password=mypassword,vers=3.0
```

Do not commit `.env`, SMB usernames, SMB passwords, API keys, or server-specific paths.

## Unraid Template

The canonical Community Applications template is:

```text
templates/my-mkv-orchestrator.xml
```

The lowercase `my-mkv-orchestrator.xml` filename follows the Unraid template
naming convention. The display name remains `MKV Orchestrator`, while the image
and repository slug remain `mkv-orchestrator`. Environment variables use the
existing `MKVO_` prefix. `ca_profile.xml` in the repository root supplies the
required Community Applications repository profile. The legacy
`unraid/my-MKV-Orchestrator.xml` path remains as a compatibility copy.

The template uses generic defaults:

```text
/mnt/user/media           -> /media
/mnt/user/downloads/media -> /downloads
/mnt/user/appdata/mkv-orchestrator -> /config
```

Adjust those host paths in Unraid before starting the container. Existing users
can keep an earlier `/mnt/user/appdata/mkvo` mapping to retain their data. The
template does not include API keys, SMB credentials, or server-specific paths.
TVDB/TMDB keys can be entered in Settings after first launch, or through the
masked optional environment variables. Choose `disabled` for no login on a
trusted LAN, or `basic` and fill in both credential fields.

Before submitting or updating the template, run the Community Applications
**Validate** and **Scan** checks after every meaningful XML change.

## Included Tools

The image installs:

- `mkvmerge`
- `mkvpropedit`
- `mkvextract`
- `mkvinfo`
- `ffmpeg`
- `ffprobe`

The Settings page reports the resolved tool paths and version strings from inside the container.

## Current Scope

This is a single-container web release. Dashboard scanning, rename preview/apply/undo, mux/remux planning, lossless MP4 to MKV conversion, track property edits, library overview, settings, and logs are exposed through the same Rust host. Processing logic remains in shared core services instead of being duplicated in the React UI.
