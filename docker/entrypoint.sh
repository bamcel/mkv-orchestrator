#!/bin/sh
set -e

# Optional Unraid/LinuxServer-style user mapping. When PUID/PGID are provided,
# run the app as that user so files written to mounted shares are not root-owned.
PUID="${PUID:-0}"
PGID="${PGID:-0}"

if [ -n "${UMASK:-}" ]; then
    umask "${UMASK}"
fi

if [ "${PUID}" = "0" ] && [ "${PGID}" = "0" ]; then
    exec dotnet /app/MKVOrchestrator.WebHost.dll "$@"
fi

if ! getent group mkvo >/dev/null 2>&1; then
    groupadd --gid "${PGID}" mkvo 2>/dev/null || groupmod --gid "${PGID}" "$(getent group "${PGID}" | cut -d: -f1)" >/dev/null 2>&1 || true
fi

if ! id mkvo >/dev/null 2>&1; then
    useradd --uid "${PUID}" --gid "${PGID}" --no-create-home --home-dir /config --shell /usr/sbin/nologin mkvo 2>/dev/null || true
fi

# Config must stay writable for settings, caches, and rename history.
chown -R "${PUID}:${PGID}" /config 2>/dev/null || true

exec gosu "${PUID}:${PGID}" dotnet /app/MKVOrchestrator.WebHost.dll "$@"
