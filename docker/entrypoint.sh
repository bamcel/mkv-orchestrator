#!/bin/sh
set -eu

# Optional Unraid/LinuxServer-style user mapping. When PUID/PGID are provided,
# run MKVO as that user so outputs on mounted shares are not root-owned.
PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

if [ -n "${UMASK:-}" ]; then
    umask "${UMASK}"
fi

if [ "${PUID}" = "0" ] && [ "${PGID}" = "0" ]; then
    exec /app/mkvo-server "$@"
fi

if ! getent group mkvo >/dev/null 2>&1; then
    groupadd --gid "${PGID}" mkvo 2>/dev/null \
        || groupmod --gid "${PGID}" "$(getent group "${PGID}" | cut -d: -f1)" >/dev/null 2>&1 \
        || true
fi

if ! id mkvo >/dev/null 2>&1; then
    useradd --uid "${PUID}" --gid "${PGID}" --no-create-home --home-dir /config --shell /usr/sbin/nologin mkvo 2>/dev/null \
        || true
fi

chown -R "${PUID}:${PGID}" /config 2>/dev/null || true
exec gosu "${PUID}:${PGID}" /app/mkvo-server "$@"
