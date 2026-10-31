# 0002. Hexagonal workspace with enforced dependency rules

- Status: Accepted
- Date: 2026-10-31

## Context

The product spans a web API, a chat bot, an LLM agent loop, a database, a WebSocket protocol and process execution on hosts. Mixing these in a few large crates makes the safety-critical parts (approvals, execution) hard to reason about and hard to test.

## Decision

Use a Cargo workspace organised as ports and adapters:

- `domovoi-core` holds domain types, state machines and port traits, with no I/O.
- `domovoi-engine` holds application logic and depends only on core.
- Adapters (`store`, `llm`, `chat`, `checks`, `runner`) each depend only on core.
- Runtimes (`agent`, `hub`) wire adapters; the `domovoi` binary is the outer composition root.

The allowed edges are listed in `ARCHITECTURE.md` §4 and encoded in `xtask/src/arch/rules.rs`. `cargo xtask arch-check` runs in CI and fails on any violation.

## Consequences

- Engine logic is testable with fakes from `domovoi-testkit` and a fake clock.
- Adding a gateway or provider means adding an adapter, not touching the engine.
- More crates mean more `Cargo.toml` files; versions are centralised in `[workspace.dependencies]` to keep that manageable.

## Alternatives considered

- Single crate with modules: boundaries are not enforceable; agents would cross them.
- Layered crates without ports: the engine would depend on sqlx and reqwest directly, losing testability.
