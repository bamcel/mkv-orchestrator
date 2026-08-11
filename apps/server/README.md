# MKVO Server

The Rust HTTP delivery adapter hosts the shared React UI and MKVO application
services for Docker, NAS, and remote-browser deployments.

Configuration is environment-based:

- `MKVO_BIND` (default `127.0.0.1:8080`)
- `MKVO_MEDIA_ROOT` (default `/media`)
- `MKVO_SOURCE_ROOTS` (`label=/path` entries separated by commas or semicolons)
- `MKVO_CONFIG_DIR` (default `/config`)
- `MKVO_UI_DIR` (default `web/dist`)
- `MKVO_AUTH_MODE` (`auto`, `basic`, or `disabled`; default `auto`)
- `MKVO_AUTH_USERNAME` and `MKVO_AUTH_PASSWORD` (both or neither)
- `MKVO_REQUEST_BODY_LIMIT_BYTES` (default `16777216`)
- `MKVO_GRACEFUL_SHUTDOWN_SECONDS` (default `15`)

`/api/health` remains public for container health checks. All other routes and
the UI are protected in `basic` mode. The default `auto` mode refuses an
unauthenticated non-loopback bind. Set `disabled` explicitly only for a trusted
LAN. Basic authentication must be placed behind HTTPS when traffic leaves the
local machine.

The server preserves the React client's polling endpoints and also exposes
Server-Sent Events at `/api/scans/{id}/events` and
`/api/operations/{id}/events`. Unknown `/api` paths always return typed JSON
errors; only non-API navigation falls back to the React `index.html`.
