# Versioning and Migrations

## Product version

Release tags use `vMAJOR.MINOR.PATCH`, such as `v1.1.0`. The version now lives
in three places: `version` under `[workspace.package]` in the root `Cargo.toml`,
`version` in `apps/desktop/src-tauri/tauri.conf.json` (which sets the installer
and window metadata), and `web/package.json` with its lock file.

These currently disagree -- the Rust workspace and Tauri host say `0.1.0` while
the web package still says `1.1.0`, a leftover of the .NET tree carrying the
product version in `Directory.Build.props` before the cutover removed it. Settle
on one number before the first Tauri release and set all three together.

## Settings schema

`AppSettings.SettingsSchemaVersion` tracks the JSON settings schema.

Current version:

```text
1
```

The settings loader calls a migration hook before returning settings. Future schema changes should add migration steps in `AppSettingsService.Migrate`.

## Metadata cache schema

`MetadataCacheDatabase.CurrentCacheSchemaVersion` tracks the SQLite cache schema.

Current version:

```text
1
```

The cache database stores the version in:

```sql
cache_metadata(key, value)
```

Future cache changes should add migration logic in `MetadataCacheDatabase.EnsureSchema`.

If a cache is detected from a newer app version, MKVO clears cached media rows rather than risking incompatible reads. This is safe because the metadata cache is rebuildable.
