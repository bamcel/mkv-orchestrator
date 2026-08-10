# ADR 0002: Contract-First Tauri and HTTP Transports

- Status: Accepted
- Date: 2026-08-04

## Context

The React client must work unchanged with both Tauri IPC and HTTP. Handwritten
models on each side can drift in field names, optionality, and error behavior.

## Decision

Rust serializable contracts are authoritative. Tauri and HTTP expose the same
request, response, error, plan, and job-event shapes. TypeScript declarations
and runtime validation are generated or snapshot-checked in CI. The frontend
selects a `BackendClient` adapter at runtime.

## Consequences

Contract changes require compatibility review and tests. Transport adapters may
translate framing and event delivery, but may not redefine business semantics.

