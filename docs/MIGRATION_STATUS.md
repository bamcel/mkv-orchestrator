# Tauri/Rust Migration Status

This file is the executable cutover ledger for the staged MKVO migration. A
feature is not considered migrated merely because a Rust module exists; its
parity gate must pass against the current .NET implementation.

## Stage gates

| Stage | Scope | Status | Exit evidence |
| --- | --- | --- | --- |
| 0 | Baseline and decisions | Complete | Architecture plan, ADRs, 50-case fixture inventory, .NET 22/22 baseline, performance budgets |
| 1 | Rust domain and contracts | Complete | 142 Rust and 15 web tests, strict Clippy, generated TypeScript contracts with a CI drift check |
| 2 | Processes, cache, and jobs | Complete | Live scan of real MKV/MP4 media: 3/3 probed, 0 failed, second scan reports `cached: 3` |
| 3 | Dashboard slice | Complete | `mkvo-server` serves status/browse/scan end to end. The Tauri desktop app launches, renders all seven sections, and completed an IPC round trip: the status panel reports the backend-resolved media root and `browse_file_system` returned real directory contents |
| 4 | Rename | Provider path verified; TVDB/TMDB unverified | Search, scope loading, and episode listing ran against the live AniList and AniDB APIs. TVDB and TMDB clients have never made a real call because no credentials are available |
| 5 | Remux and Track Properties | Complete | Property edit mutates a real file, replays idempotently, and refuses a tampered fingerprint. All four remux modes executed and their outputs structurally verified with `mkvmerge -J` |
| 6 | Library, watchers, servers, settings, logs | Complete | Watchers start at boot and refresh the cache on a real file drop; library audit reports real grouping and mismatch issues; media-server discovery verified over HTTP against a stub for all three vendors; logs and export return real entries; desktop secrets round-trip through the OS credential store |
| 7 | Rust HTTP/Docker host | Complete | Image builds at 827MB, healthcheck reports healthy, all six tools resolve inside the container, a scan of mounted media returns correct tracks, the SPA is served, and PUID/PGID runs the process as 1000:1000 |
| 8 | Cutover | Pending | Side-by-side beta, migration validation, rollback window |

"Built, unverified" means the code compiles, is covered by unit tests, and has a
transport wired, but has never processed a real file. The first live run of the
scan path found four defects that every unit test had passed over, so this
distinction is deliberate: only a row citing a live run counts as evidence.

## Verified live behavior

Recorded 2026-08-05 on Windows 11 with MKVToolNix v88.0 and FFmpeg 8.1.2,
against generated MKV/MP4 fixtures.

- All six external tools resolve and report versions.
- Folder browse and recursive scan return correct containers, resolutions,
  track counts, and per-track languages.
- A repeat scan reuses cache entries instead of re-probing.
- Property-edit preview produces an immutable fingerprinted plan; apply mutates
  the real file; replay is idempotent; a tampered fingerprint is refused.
- The legacy `settings.json` importer leaves the original byte-identical and
  writes only a sibling `.mkvo-backup`.

## Defects found by live running

Each was invisible to the unit suite because every test supplied
already-normalized inputs or fixture payloads.

| Defect | Effect | Fix |
| --- | --- | --- |
| Legacy importer stored `""` as configured tool paths | An explicit path never falls back to `PATH` search, so `mkvmerge`, `mkvpropedit`, and `ffprobe` were permanently unresolvable | Blank values import as unset; `AppSettings::normalized` also drops blank paths |
| Version probe used `--version` for every tool | `ffmpeg --version` exits 8 and `ffprobe --version` exits 1, so both read as present but unusable | Version flag is per tool kind |
| `display_unit` typed as a string | Real `mkvmerge -J` emits an integer, so every MKV with a video track failed to parse while MP4s succeeded | Correct typing, plus lenient string properties so future schema drift degrades to "absent" rather than a skipped file |
| Plan-time root check used raw configured paths | Scanned paths are canonical (`\\?\` on Windows) and never prefix-matched, so every mutating plan reported its inputs unauthorized | Planners share one `path_key`/`path_contains`; roots come from the live authorization service |
| Watch folders were never started or consumed | The backend was constructed but only started from a settings save, and nothing subscribed to its events, so live monitoring did nothing at all | A watch supervisor starts watchers at boot and turns changes into cache updates, reconciling on lag |
| Cache keyed canonical paths while watch events reported plain ones | A deletion never matched a cache row, so pruning silently did nothing | One path-identity definition in `mkvo-domain`, shared by the cache key and the UI display helper |
| `LogQuery` derived `Default` | `#[serde(default = "…")]` applies only when deserializing, so `limit` was 0 and every caller asked the store for zero rows — the Logs page was permanently empty | Hand-written `Default` that agrees with the wire default |
| Scans never wrote to the operation log | Only rename, mux, and propedit logged, so the most-run operation left no trace even once the query was fixed | Scan completion logs its summary, correlated to the job that produced it |
| Every port failure mapped to `Internal` | An unconfigured provider returned 500 "internal error", sending users to file a bug instead of opening Settings | Port failures keep an actionable code and status |
| AniList `seasonYear` never deserialized | The struct lacked the camelCase rename, so every search result silently lost its year | Rename plus a test asserting the real wire field names |
| The frontend hardcoded the ignored-folder list | The constant was sent with every scan, silently overriding the folders configured in Settings | Seeded from saved settings, without clobbering an edit in progress |
| Verbatim `\\?\` paths reached the UI | Canonicalized paths are not what a user recognizes or can paste back into a folder field | Normalized at the DTO boundary; process arguments deliberately keep the prefix for long-path support |

## Cutover rules

- The Avalonia and ASP.NET projects remain releasable until their replacement
  stage passes its gate.
- New Rust workflows operate on test fixtures or explicitly selected files until
  mutation parity and recovery tests pass.
- Rename, remux, conversion, extraction, and property edits always preview an
  immutable plan and revalidate it before apply.
- A failure in the replacement does not delete or rewrite legacy settings,
  history, or cache databases.
- Stage 8 requires every row of the feature-parity matrix in
  `TAURI_RUST_ARCHITECTURE_PLAN.md` to have automated or documented evidence.

## Initial performance budgets

These are regression gates, not marketing targets. They can be revised through
an ADR after measurements from representative libraries are recorded.

| Measure | Initial budget |
| --- | --- |
| Warm desktop startup to interactive shell | 2 seconds on the reference Windows machine |
| Cancellation acknowledgement | 250 ms, excluding an uninterruptible external-tool step |
| UI progress update cadence | At least every 500 ms while measurable work advances |
| Idle CPU with watchers enabled | Below 1% average on the reference library |
| Scan result drift | Zero semantic differences for golden fixtures |
| Mutation-plan drift | Zero argument or destination differences for golden fixtures |

## Worktree protection

The migration is being added alongside an existing uncommitted Avalonia
refactor. Migration work must not reset, overwrite, or silently absorb those
changes. Cross-cutting edits require an explicit diff review.
