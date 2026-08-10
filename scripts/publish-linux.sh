#!/usr/bin/env bash
# Builds the Linux desktop bundles (.deb and AppImage).
#
# The Tauri CLI drives the whole chain: it builds the React frontend the
# desktop host embeds, compiles the host in release, and bundles the result.
# Artifacts land under target/release/bundle.
#
# The webview toolchain has to be present. On Debian/Ubuntu:
#   sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev \
#     librsvg2-dev patchelf libgtk-3-dev libsoup-3.0-dev \
#     libjavascriptcoregtk-4.1-dev
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR/web"

[ -d node_modules ] || npm ci
npm run tauri -- build

echo "Bundles written to $ROOT_DIR/target/release/bundle"
