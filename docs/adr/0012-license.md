# 0012. License: MIT OR Apache-2.0

- Status: Accepted
- Date: 2026-10-31

## Context

Domovoi is meant for wide use and contribution, and to be packaged by distributions and homelab app stores.

## Decision

Dual-license under MIT OR Apache-2.0, the Rust ecosystem convention. Contributions are accepted under the same terms (see `CONTRIBUTING.md`). `cargo deny` allows only licenses compatible with this choice.

## Consequences

- Maximum compatibility for packagers and companies; no copyleft protection against hosted forks.

## Alternatives considered

- AGPL-3.0: protects against closed hosted forks but deters some contributors and packagers.
