# MKVO Server

The Rust HTTP delivery adapter hosts the shared React UI and MKVO application
services for Docker, NAS, and remote-browser deployments.

Configuration is environment-based:

- `MKVO_BIND` (default `127.0.0.1:8080`)
- `MKVO_MEDIA_ROOT` (default `/media`)
- `MKVO_SOURCE_ROOTS` (`label=/path` entries separated by commas or semicolons)
- `MKVO_CONFIG_DIR` (default `/config`)
- `MKVO_UI_DIR` (default `web/dist`)
- `MKVO_AUTH_ENABLED` (true/false; standalone runtime default true, supplied container defaults false)
- `MKVO_USERNAME` (default `admin`) and `MKVO_PASSWORD` (blank generates `admin-password.txt` under `MKVO_CONFIG_DIR`)
- `MKVO_SECURE_COOKIES` (default false; enable for HTTPS)
- Legacy `MKVO_AUTH_MODE`, `MKVO_AUTH_USERNAME`, and `MKVO_AUTH_PASSWORD` remain supported when the corresponding new settings are absent.
- `MKVO_REQUEST_BODY_LIMIT_BYTES` (default `16777216`)
- `MKVO_GRACEFUL_SHUTDOWN_SECONDS` (default `15`)

`/api/health` and auth status/login/logout remain public. The UI loads the
sign-in gate publicly; all other APIs are protected by server-side cookie
sessions when login is enabled. False allows all connections, including
reverse-proxy visitors, without login. Use HTTPS and network restrictions.
See [authentication notes](../../docs/AUTHENTICATION.md) for migration,
session expiry, and the explicit local-network bypass warning.

The server preserves the React client's polling endpoints and also exposes
Server-Sent Events at `/api/scans/{id}/events` and
`/api/operations/{id}/events`. Unknown `/api` paths always return typed JSON
errors; only non-API navigation falls back to the React `index.html`.
