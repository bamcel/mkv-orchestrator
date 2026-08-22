# Generated MKVO contracts

`contracts.ts` is generated from the serde-annotated Rust DTOs in `crates/mkvo-contracts`. It currently contains 74 boundary DTOs and 26 referenced domain enums. It also includes zero-dependency runtime validators.

Generate bindings:

```powershell
./scripts/generate-contracts.ps1
```

Check drift without writing:

```powershell
./scripts/generate-contracts.ps1 -Check
# Cross-platform CI equivalent:
cargo run --locked -p mkvo-contract-gen -- --check
```

## Wire-shape rules

- serde `rename` and `rename_all` determine property and enum names.
- `Option<T>` is represented as `T | null`. A property is optional when serde can omit it while serializing. On `*Request` DTOs, option fields are also optional because serde accepts a missing option.
- serde `flatten` is emitted as a TypeScript intersection and validated against the same object.
- Date/time and ID newtypes are JSON strings; integer primitives are JSON numbers.
- Unknown JSON values remain the recursive `JsonValue` type.

## Intentional generation gaps

These Rust-only or domain-native types are deliberately excluded from the compatibility UI boundary:

- `ApiEnvelope` — Generic host envelope; current Tauri and HTTP compatibility adapters return the enclosed DTO directly.
- `ApiResult` — Rust control-flow alias (Result), not a JSON boundary DTO.
- `LibraryAuditDomainResponse` — Domain-native audit graph; React consumes LibraryAuditResponse compatibility rows.
- `PropertyEditPlanResponse` — Domain-native execution plan; React consumes preview DTOs plus plan identifiers.
- `RemuxPlanResponse` — Domain-native execution plan; React consumes preview DTOs plus plan identifiers.
- `RenamePlanResponse` — Domain-native execution plan; React consumes preview DTOs plus plan identifiers.
- `SaveSettingsRequest` — Domain-native AppSettings request; the compatibility UI uses WebSettingsRequest.
- `SettingsResponse` — Domain-native AppSettings response; the compatibility UI uses WebSettings.

Runtime adapter-only callback types (`BackendClient`, job-progress listener functions, unsubscribe handles, and transport selection) remain hand-maintained in `web/src/backend/client.ts`; they are TypeScript behavior, not serializable Rust DTOs.
