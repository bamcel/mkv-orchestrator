# Versioning and Migrations

## Product version

Release tags use `vMAJOR.MINOR.PATCH`, such as `v1.1.0`. The version now lives
in three places: `version` under `[workspace.package]` in the root `Cargo.toml`,
`version` in `apps/desktop/src-tauri/tauri.conf.json` (which sets the installer
and window metadata), and `web/package.json` with its lock file.

All three currently use `0.1.0`. Update them together for every release.

## Settings schema

`AppSettings::schema_version` tracks the persisted settings document schema.

Current version:

```text
2
```

Future settings-schema changes must add a Rust migration step before returning
the document and a fixture covering the previous persisted representation.

## Metadata cache schema

SQLite migrations in `mkvo-infra-sqlite` track the metadata cache schema.

Current version:

```text
1
```

The database stores its current migration number in:

```sql
PRAGMA user_version
```

Future cache changes should add a numbered migration in `mkvo-infra-sqlite`.
Destructive cache rebuilds are acceptable only for rebuildable scan metadata;
settings, plans, journals, job history, and secrets must be migrated without
silent data loss.
