# ADR 0003: Journaled and Revalidated Media Mutations

- Status: Accepted
- Date: 2026-08-04

## Context

Rename, remux, conversion, extraction, and property editing can alter valuable
media. Crashes, retries, stale previews, watcher races, or partial tool output
must not cause duplicate or ambiguous changes.

## Decision

All mutations use immutable plans containing path and file fingerprints,
settings and tool versions, an expiry, and an idempotency key. Apply acquires
resource leases, revalidates the plan, journals every durable step, stages
outputs where possible, validates them, and atomically promotes them.

## Consequences

The implementation is more deliberate than directly executing command-line
arguments, but supports safe retries, startup recovery, meaningful undo, and
auditable failures.

