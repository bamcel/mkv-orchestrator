# ADR 0001: Shared Rust Modular Monolith

- Status: Accepted
- Date: 2026-08-04

## Context

MKVO needs a native desktop host and a Docker/NAS browser host with identical
media behavior. The current delivery surfaces have accumulated separate state,
job, and transport models.

## Decision

Implement media behavior as a Rust modular monolith. Domain, application, and
infrastructure crates are shared by thin Tauri IPC and Axum HTTP adapters. Do
not introduce networked microservices or a distributed queue.

## Consequences

Feature behavior and tests have one owner, while desktop and server deployment
remain independent. Crate boundaries are compile-time architecture boundaries;
they must not become circular or transport-aware.

