# syntax=docker/dockerfile:1

FROM node:22-bookworm-slim AS web-build
WORKDIR /src/web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.88-bookworm AS rust-build
WORKDIR /src
# libdbus is needed to build, not to work: the keyring crate reaches the Linux
# secret service over D-Bus. A container has no session bus, so the store
# reports itself unusable at runtime and callers fall back -- but the crate is
# still compiled and linked, so the library has to be here.
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY .cargo/ .cargo/
COPY crates/ crates/
COPY apps/ apps/
COPY tools/ tools/
RUN cargo build --locked --release --package mkvo-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg mkvtoolnix ca-certificates curl gosu passwd libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust-build /src/target/release/mkvo-server /app/mkvo-server
COPY --from=web-build /src/web/dist/ /app/web/
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh \
    && mkdir -p /media /config
ENV MKVO_BIND=0.0.0.0:8080 \
    MKVO_MEDIA_ROOT=/media \
    MKVO_CONFIG_DIR=/config \
    MKVO_UI_DIR=/app/web \
    HOME=/config \
    XDG_CONFIG_HOME=/config \
    XDG_DATA_HOME=/config/.local/share
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD curl -fsS http://localhost:8080/api/health || exit 1
ENTRYPOINT ["/entrypoint.sh"]
