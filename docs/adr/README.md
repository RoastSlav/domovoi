# Architecture decision records

An ADR records one decision that shapes the codebase: the context, the decision and its consequences. Rules and invariants in [`ARCHITECTURE.md`](../../ARCHITECTURE.md) change only through a new ADR.

## When you need one

- Changing a dependency rule, crate boundary or invariant (CI enforces this: a change to `xtask/src/arch/rules.rs` without a new ADR fails `arch-check`).
- Adding a crate, a new external service, a new persistence technology or a new wire format.
- Reversing an earlier ADR.

## How

1. Copy [`0000-template.md`](0000-template.md) to the next number with a short slug.
2. Set status `Proposed`, open a PR, discuss.
3. On merge, status becomes `Accepted`. A later ADR that replaces it marks it `Superseded by NNNN`.

## Index

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-hexagonal-workspace.md) | Hexagonal workspace with enforced dependency rules | Accepted |
| [0003](0003-hub-agent-split.md) | Hub and agent roles; agent dials out over mutually authenticated TLS | Accepted |
| [0004](0004-runner-is-the-only-process-spawner.md) | Only `domovoi-runner` spawns processes; argv only, never a shell | Accepted |
| [0005](0005-sqlite-with-sqlx.md) | SQLite via sqlx, runtime-checked queries, append-only migrations | Accepted |
| [0006](0006-embedded-react-spa.md) | React SPA embedded in the hub; OpenAPI-generated client | Accepted |
| [0007](0007-human-only-approvals.md) | Only humans approve actions | Accepted |
| [0008](0008-release-pipeline.md) | release-plz, native musl builds, GHCR images; crates.io deferred | Accepted |
| [0009](0009-llm-provider-port.md) | LLM providers behind one port; CLI provider via injected executor | Accepted |
| [0010](0010-discord-first.md) | Discord is the first interactive chat gateway | Accepted |
| [0011](0011-single-writer-incident-manager.md) | Incident state has a single writer | Accepted |
| [0012](0012-license.md) | License: MIT OR Apache-2.0 | Accepted |
| [0013](0013-config-error-paths.md) | Config errors carry their TOML path via serde_path_to_error | Accepted |
| [0014](0014-rehydrating-stored-approvals.md) | Only the store may rebuild a stored approval | Accepted |
| [0015](0015-user-editable-message-templates.md) | Let users edit routine message templates | Proposed |
| [0016](0016-improvement-suggestions.md) | Suggest improvements from history, never apply them | Proposed |
