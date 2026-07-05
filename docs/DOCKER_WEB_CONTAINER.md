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

The default compose file mounts:

```text
//192.168.1.79/media -> /media
./tmp/docker-config   -> /config
```

The web app browses container paths. With the default compose file, `/media` points at the SMB share `\\192.168.1.79\media`.

Change the `device` value if you want to test with a different SMB share, or replace the volume with a normal bind mount for a local folder.

The default NAS mount uses Docker's local CIFS volume driver:

```yaml
volumes:
  mkvo-media:
    driver: local
    driver_opts:
      type: cifs
      device: //192.168.1.79/media
      o: guest,vers=3.0
```

If your share requires credentials, add them through your own local override file rather than committing them.

Example:

```yaml
volumes:
  - D:/Media:/media
  - ./tmp/docker-config:/config
```

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

This is the first single-container web baseline. Dashboard scanning is wired to the same `MKVOrchestrator.Core` scanner used by the desktop app. Additional desktop workflows should be ported behind API endpoints in later passes so the processing logic remains shared instead of duplicated in the React UI.
