# MKVO C# to Rust parity fixtures

This directory contains deterministic, sanitized golden fixtures for the staged MKVO migration. The fixtures separate two kinds of acceptance criteria:

- `legacy_parity` reproduces observable behavior from the current C# core or WebHost.
- `target_policy` locks in safety behavior introduced by the Tauri/Rust architecture plan where the legacy application has no durable equivalent.

Every JSON fixture has `schemaVersion: 1`, a stable `fixtureId`, source references, comparison rules, and one or more named cases. `manifest.json` is the canonical inventory.

## Portable values

Fixtures never contain a real user path, credential, random identifier, or current timestamp.

- `${ROOT}` represents an isolated media-fixture root.
- `${TOOLS}` represents an isolated tool-fixture root.
- `${SECRET:NAME}` represents a fake secret token supplied by a test harness. It must never be written to a public settings snapshot or test log.
- POSIX-style `/` separators are canonical in expected JSON. A platform adapter may translate them before invoking native code, then normalize output back to `/` before comparison.
- UUIDs and timestamps are fixed test values.

Raw `mkvmerge` and `ffprobe` payloads are data only. No fixture command should launch an external process.

## Comparison rules

Unless a case overrides the rule:

- JSON object property order is irrelevant.
- `expected` objects are recursive listed-field projections unless a fixture sets `exactObjectShape: true`; richer target DTOs may contain additional fields.
- Array order is significant.
- Paths are compared after separator normalization only; case is preserved.
- Omitted optional values and explicit `null` are distinct.
- Error codes are stable contract values. Human-readable messages may be checked separately when listed in `expected`.

The parser fixtures include the exact legacy projection used by the current UI. A Rust parser may use a richer internal model, but its compatibility DTO must reproduce these golden values during staged cutover.

## Suggested harness flow

1. Load `manifest.json` and reject unsupported fixture-set schema versions.
2. Resolve only the documented path and secret tokens into an isolated temporary tree.
3. Run the named pure parser/planner/importer operation.
4. Normalize returned paths to `/`.
5. Compare the result with `expected` according to the fixture's `comparison` block.
6. Run every `legacy_parity` case against both implementations until the C# backend is retired.
7. Run every `target_policy` case against Rust before enabling mutations.

## Syntax validation

From the repository root in PowerShell:

```powershell
Get-ChildItem tests/parity-fixtures -Filter *.json | ForEach-Object {
    Get-Content -Raw $_.FullName | ConvertFrom-Json | Out-Null
}
```

These fixtures intentionally avoid binary media. They validate parsing and planning from captured, synthetic metadata and virtual filesystem descriptions.
