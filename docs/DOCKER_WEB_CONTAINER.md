# MKVO Docker Web Container

The Docker web build runs MKV Orchestrator as one container:

- ASP.NET Core hosts the API and serves the web app.
- React, TypeScript, Vite, Tailwind CSS, TanStack Query, React Router, and lucide-react provide the browser UI.
- MKVToolNix and FFmpeg are installed inside the runtime image.

## Run

```powershell
docker compose up --build
```

Open:

```text
http://localhost:8080
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

An Unraid Docker template is available at:

```text
unraid/mkvo.xml
```

The template uses generic defaults:

```text
/mnt/user/media           -> /media
/mnt/user/downloads/media -> /downloads
/mnt/user/appdata/mkvo    -> /config
```

Adjust those host paths in Unraid before starting the container. The template does not include API keys, SMB credentials, or server-specific paths. TVDB/TMDB keys can be entered in the MKVO Settings page after first launch, or by using the masked optional environment variables in the Unraid template.

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

This is a single-container web release. Dashboard scanning, rename preview/apply/undo, mux/remux planning, track property edits, library overview, settings, and logs are exposed through the same ASP.NET Core host. The processing logic should continue moving through shared core services instead of being duplicated in the React UI.
