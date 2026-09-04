# Administrator login

MKVO ports the authentication components from PosterView revision
`6e8621b70ce43ddb14f20e95ad3a5488af4c5ad7`, adapted to MKVO branding,
theme tokens, storage names, and existing routes. This is single-administrator
authentication, not multi-user accounts, MFA, password recovery, or login throttling.

## Container setup and compatibility

New Unraid installs show Require Login (false), Administrator Username (admin),
then the masked Administrator Password. False grants full access to everyone
who can reach the service, including visitors through a reverse proxy.
Changing environment settings requires recreating the container.

Set `MKVO_AUTH_ENABLED=true` to require login. `MKVO_USERNAME` is case-sensitive;
blank uses admin. A blank `MKVO_PASSWORD` generates a password stored privately
in `/config/admin-password.txt`. Retrieve it from the file, not logs. Generated
password files use owner-only permissions on Unix. Keep the config volume private
and backed up; never publish the password file. Credentials are retained while
login is disabled. Use `MKVO_SECURE_COOKIES=true` for HTTPS, not plain HTTP.

No media or configuration mounts change: `/media` and `/config` remain in use.
The reference's data-directory setting maps to existing `MKVO_CONFIG_DIR`.
Existing `MKVO_AUTH_MODE=disabled` remains disabled; basic/auto credentials
are reused by the new sign-in form. New nonblank credential variables override
their legacy equivalents; an explicitly set true/false `MKVO_AUTH_ENABLED`
overrides the legacy mode. Compose leaves the new flag empty by default so
existing basic-auth deployments are not silently disabled. Its legacy fallback
is disabled. With no authentication variables at all, the standalone server
requires login and generates a password.

The native Tauri desktop app continues to use the operating-system account;
the new login gate and server security preferences apply to the HTTP app only.
The existing desktop layout and zoom behavior are unchanged. Sign out sits
above the sidebar status panel, with header access at narrow browser widths.

## Sessions and preferences

The public login, logout, and status endpoints use same-origin requests.
All other APIs except health are protected when a password is required.
The configured username is intentionally public for prefill; passwords are not.
The opaque session cookie is named `mkvo_session`, HttpOnly, SameSite=Strict,
Path=/, and optionally Secure. Sessions stay in memory and end on restart.

Privacy / Security persists timeout and LAN-bypass preferences atomically in
`/config/security-settings.json`. Inactivity logout defaults to disabled;
enabled timeouts range from 1 to 1440 minutes. The server independently enforces
expiry. Only activity requests extend sessions, not background API polling.
Username remembering is browser-local, defaults on, and saves immediately.
No password or session token is stored in localStorage or sessionStorage.

Local-network bypass defaults off and uses only the actual TCP peer address,
not Host or forwarded headers. Missing peer information fails closed.

> Warning: Anyone whose connection appears local gets full access without a password.
> A reverse proxy or Docker networking can make remote visitors appear local too,
> including visitors using your public domain. Enable only if you accept this risk.

Global login disabled overrides bypass and inactivity settings. Password-free
connections have no sign-out control or inactivity lock. Keep TLS, network
restrictions, and reverse-proxy protections in place as appropriate.

## Verification for this port

- 137 frontend tests and 32 server tests pass; frontend production build passes.
- Server formatting and strict server-only Clippy pass. The workspace-wide
  formatting check and dependency-inclusive strict Clippy still report existing
  formatting / collapsible-if issues outside the authentication changes.
- Unraid XML parses and Docker Compose validates.
- The actual server entry point was exercised over loopback TCP for login,
  logout, and opt-in local-peer bypass.
- The login screen was inspected at desktop and 390px phone width; Security
  settings were inspected at desktop width. The full mobile console was not
  redesigned or visually certified. Linux container execution and Unix password
  permissions still require a Linux/container verification run.
