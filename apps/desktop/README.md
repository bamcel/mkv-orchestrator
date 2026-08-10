# MKVO Desktop

This is the Tauri delivery adapter for the shared MKVO Rust application core and
React interface. It owns native window setup, IPC commands, application
composition, and desktop-only permissions. Media workflows belong in the Rust
workspace crates and must remain reusable by `mkvo-server`.

At startup the host creates platform-specific app-data and app-config
directories, opens the shared runtime database, and authorizes the user's Videos
directory as the initial writable media root. If a Videos directory is not
available, MKVO creates an isolated `media` directory under its app-data
directory. The config directory's `tools` folder is searched before the normal
system tool locations.

Development commands are run from the repository root. Install the web packages
once, then start the Tauri development host:

```powershell
npm --prefix web install
cargo tauri dev --config apps/desktop/src-tauri/tauri.conf.json
```

Build a release bundle with:

```powershell
cargo tauri build --config apps/desktop/src-tauri/tauri.conf.json
```

The React client talks only to the snake-case commands registered in
`src-tauri/src/commands.rs`. Each command forwards to `mkvo-runtime`; the desktop
host does not duplicate scanning, planning, persistence, or mutation logic.
Long-running commands return a job snapshot immediately and publish
`mkvo-job-progress` events when the host has an updated snapshot. The React
client also polls, so progress remains reliable if an event is missed.

The main window has only Tauri core permissions. Filesystem access is performed
inside the Rust runtime and constrained to its authorized media roots; no broad
frontend filesystem, shell, dialog, or opener capability is granted.

The existing Avalonia desktop remains available during staged parity testing.
