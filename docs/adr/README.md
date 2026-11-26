# Architecture decision records

An ADR records one decision that shapes the codebase: the context, the decision and its consequences. Rules and invariants in [`ARCHITECTURE.md`](../../ARCHITECTURE.md) change only through a new ADR.

## When you need one

- Changing a dependency rule, crate boundary or invariant (CI enforces this: a change to `xtask/src/arch/rules.rs` without a new ADR fails `arch-check`).
- Adding a crate, a new external service, a new persistence technology or a new wire format.
- Reversing an earlier ADR.

## How

1. Copy [`0000-template.md`](0000-template.md) to the next number with a short slug.
2. Open a PR with status `Accepted` and discuss it there; the ADR merges only when the decision is agreed.
3. To reverse a decision, rewrite or delete its ADR in the same change as the code, and fix every link to it. An ADR is a record of what is in force, not of how it got there.

## Index

Numbers are never reused, and gaps are intentional: they are where an ADR was removed. Links and rule citations use the numbers.

| # | Decision |
| --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions as ADRs |
| [0002](0002-hexagonal-workspace.md) | Ports-and-adapters workspace; dependency rules enforced by `arch-check` |
| [0003](0003-hub-agent-split.md) | Hub and agent roles; the agent dials out over mutually authenticated TLS |
| [0004](0004-runner-is-the-only-process-spawner.md) | Only `domovoi-runner` spawns processes; argv only, never a shell |
| [0005](0005-sqlite-with-sqlx.md) | SQLite via sqlx, runtime-checked queries, append-only migrations |
| [0006](0006-embedded-react-spa.md) | React SPA embedded in the hub; OpenAPI-generated client |
| [0007](0007-human-only-approvals.md) | Only authenticated humans approve actions |
| [0008](0008-release-pipeline.md) | release-plz, native musl builds, GHCR images; crates.io deferred |
| [0009](0009-llm-provider-port.md) | LLM providers behind one port; the CLI provider runs through an injected executor |
| [0010](0010-discord-first.md) | Discord is the first interactive chat gateway |
| [0011](0011-single-writer-incident-manager.md) | Incident state has one writer, committed through the unit of work |
| [0013](0013-config-error-paths.md) | Config errors carry their TOML path via `serde_path_to_error` |
| [0014](0014-rehydrating-stored-approvals.md) | Only the store may rebuild a stored approval |
| [0017](0017-settings-managed-from-the-web-ui.md) | Settings are managed from the web UI, layered under file and environment |
| [0018](0018-detect-the-container-runtime.md) | The agent detects the container runtime; the hub may switch it |
| [0020](0020-license-polyform-noncommercial.md) | License: PolyForm Noncommercial 1.0.0, with free commercial use below a revenue threshold |
| [0021](0021-plan-hash-and-authorization.md) | Plan hashes bind an approval; `Authorization` has two cases and guarded constructors |
| [0022](0022-single-execution-boundary.md) | One execution boundary and one read boundary, checked by `EXEC-1` to `EXEC-4` |
| [0023](0023-capability-objects-one-checker-bounded-evidence.md) | Capability objects, one syntax-aware checker and bounded evidence reads |
| [0024](0024-capability-registry.md) | The capability registry answers availability per host and contract |
| [0025](0025-operation-aggregate.md) | The operation aggregate: immutable steps, derived status, one writer |
| [0026](0026-use-case-layer.md) | The hub use-case layer, the `Caller` and the module-edge rules `MOD-1` to `MOD-4` |
| [0027](0027-policy-shadow-recording.md) | Policy shadow decisions are recorded by a sweep, never in the writer |
| [0029](0029-package-relay-boundary.md) | Package relay boundary: a closed call, a capability object, counted link operations |
| [0030](0030-three-identities.md) | Three identities: control agent, skill runner, root helper |
| [0031](0031-helper-and-skill-runner-delivery.md) | Helper and skill-runner units, platform tiers and self-test checks |
| [0032](0032-helper-safety-attestation.md) | Helper safety attestation: a signed report verified against a pinned key |
| [0033](0033-helper-crate-and-isolation-rules.md) | The helper crate and the isolation rules enforced by `arch-check` |
| [0034](0034-skill-runner-process.md) | The skill-runner process: placement, protocol and rules |
| [0035](0035-helper-attestation-boundary.md) | The hub-side attestation boundary: `helper_trust`, the relay capability and `ATTEST-1` to `ATTEST-10` |
| [0036](0036-helper-read-operations.md) | Helper read operations: typed requests, a read-only Docker client, fixed tool spawns |
| [0037](0037-permit-issuer-and-unit-restart.md) | The one permit issuer, the capability plan family and unit restart |
| [0038](0038-containment-boundary.md) | Containment boundary: a closed call, three phases under one plan hash, hub confirmation |
| [0039](0039-single-baseline-migration.md) | Start from a single baseline migration; migrations are immutable from the first release |
| [0040](0040-closed-model-tool-set.md) | The model is offered a closed set of tools |
| [0041](0041-audit-chain.md) | The audit log is a hash chain |
| [0042](0042-transport-command-handles.md) | Transports may not name the operation or package command handles |
