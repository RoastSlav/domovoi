# 0001. Record architecture decisions

- Status: Accepted
- Date: 2026-10-31

## Context

Domovoi is built largely by coding agents working one prompt at a time. Without written decisions, each session re-derives the architecture and the codebase drifts.

## Decision

Record every decision that shapes boundaries, dependencies, wire formats or security invariants as an ADR in `docs/adr`. `ARCHITECTURE.md` is the current summary; ADRs are the history and the reasons.

## Consequences

- Agents and contributors read `ARCHITECTURE.md` before changing code and write an ADR when they need to change a rule.
- `xtask arch-check --base <ref>` fails when `xtask/src/arch/rules.rs` changes without a new ADR file in the same change.

## Alternatives considered

- Wiki pages: drift from the code and are invisible to agents working in the repository.
