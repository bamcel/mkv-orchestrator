# MKVO Re-Graph Remediation

This is the ordered remediation register produced from the post-migration
architecture and code-path review. Status reflects the repository after the
first repair pass.

## Priority order

| Order | Priority | Problem | Status | Resolution or next action |
|---:|:---:|---|:---:|---|
| 1 | P0 | The server could bind remotely without authentication | Fixed | Loopback is the default; `auto` rejects an unauthenticated non-loopback bind. Container users may explicitly select `disabled` for a trusted LAN or `basic` with credentials. |
| 2 | P0 | Remote settings could expand filesystem authorization | Fixed | Confined hosts validate against host-provided roots and cannot grant new roots. Desktop browsing remains intentionally unrestricted. |
| 3 | P0 | Media-server requests could forward credentials through redirects or changed URLs | Fixed | Redirects are disabled, URLs are validated, and a changed URL requires the key to be entered again. |
| 4 | P0 | Legacy migration was not guaranteed to run with every secret-store composition | Fixed | Both desktop and server run migration before accepting work. |
| 5 | P0 | Startup recovery existed but was not part of host startup | Fixed | Both hosts run and report recovery before watchers or commands start. |
| 6 | P0 | Settings, compatibility data, watcher state, and secrets could partially commit | Fixed | Presets and mux defaults now live in `AppSettings`; repository, secrets, and watcher changes are serialized and compensated on failure. |
| 7 | P0 | Watcher overflow was silently discarded and native watches lacked periodic reconciliation | Fixed | Overflow produces a rescan event; native and polling modes reconcile on the saved interval. |
| 8 | P1 | Saved watcher values did not drive the active watcher | Fixed | The backend consumes the complete saved watch configuration and refreshes ignored folders. |
| 9 | P1 | Scan workers and quick-hash preferences were hard-coded | Mostly fixed | Scan worker changes apply to new scans. Quick-hash is loaded at composition time; making it hot-reloadable remains optional because changing fingerprint strategy invalidates cache assumptions. |
| 10 | P1 | Tool-directory changes were saved but ignored until restart | Fixed | The shared process-tool registry is reconfigured after a successful settings save. |
| 11 | P1 | Theme and compact-preview choices diverged between browser and backend | Fixed | Backend themes hydrate the browser theme store and compact-view changes persist. |
| 12 | P1 | Selection writes raced and failures were hidden | Fixed | Writes are serialized, backend state is authoritative, and failures are visible to the user. |
| 13 | P1 | Completed jobs and scan results accumulated indefinitely | Fixed | Scan results use durable job snapshots; terminal live jobs expire after a short subscription grace period. |
| 14 | P1 | Removed media servers left API keys behind | Fixed | Both current and legacy credential aliases are cleared transactionally. |
| 15 | P1 | Provider requests could wait indefinitely | Fixed | All provider clients have connection and total request deadlines and refuse redirects. |
| 16 | P1 | Generated TypeScript schemas existed but transports trusted unchecked casts | Fixed | HTTP, Tauri commands, and native progress events validate generated contracts at runtime. |
| 17 | P1 | Frontend tests hid asynchronous updates and malformed Windows fixtures | Fixed | Tests now isolate background writes, use real paths, and run without React/key warnings. |
| 18 | P1 | Product versions and migration documentation disagreed | Fixed | Rust, Tauri, and web metadata use `0.1.0`; the guide describes the Rust/SQLite implementation. |
| 19 | P2 | Edit/remux worker settings implied concurrency that mutation journals could not safely represent | Fixed | Journals now persist per-item outcomes and both mutation engines use the normalized saved worker limits with bounded concurrency. |
| 20 | P2 | Compatibility settings lived in a separate JSON file | Fixed | Schema version 2 imports the legacy JSON once, archives it, and persists all five fields in SQLite-backed `AppSettings`. |
| 21 | P2 | Recovery could not enumerate journal rows that had no corresponding job | Fixed | SQLite enumerates incomplete journals and startup recovery classifies clean-retry versus manual-review orphans with their per-item outcomes. |
| 22 | P2 | Rename search/scope wrapper DTOs were defined in transport adapters | Fixed | Shared compatibility contracts now generate and validate the same wrapper for HTTP and Tauri. |
| 23 | P2 | Runtime and workflow modules mixed unrelated responsibilities | Fixed | Runtime features now have settings, browsing, metadata, recovery, media-server, scan-state, and host-operation boundaries. Workflows expose separate planning and execution facades, with response mapping, presentation policy, and execution support isolated behind them. Public host APIs are unchanged. |

## Recommended remaining sequence

All identified remediation items are complete. Future refactors should be driven
by new behavior or measured maintenance cost rather than file-size targets alone.

The application should remain a modular monolith. The remaining problems are
data-boundary and maintainability problems; microservices or a distributed job
system would add failure modes without improving this desktop/self-hosted use
case.
