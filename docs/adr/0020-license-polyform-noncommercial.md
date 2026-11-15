# 0020. License: PolyForm Noncommercial 1.0.0

- Status: Accepted
- Date: 2026-11-15

## Context

ADR 0012 chose MIT OR Apache-2.0 for maximum reach. That choice also lets anyone run Domovoi inside a company, bundle it into a paid product or resell it as a hosted service, without anything coming back to the project that has to fund its development. Domovoi is aimed at homelabs and self-hosters, who should keep getting it for free, and at businesses, which can afford to pay for the product they run their operations on.

## Decision

- The project is licensed under the PolyForm Noncommercial License 1.0.0, in [`LICENSE`](../../LICENSE). Personal, hobby, study, research and noncommercial-organisation use are permitted purposes; commercial use needs a separate license from the maintainer.
- As an additional permission, commercial use is free while the user and their company together earned under €100,000 in the last twelve months, with no limit on hosts. The test is revenue, not profit. A commercial license is needed above that revenue, and at any size for offering Domovoi or a product built on it to other organisations as a hosted or managed service. Crossing the threshold covers the rest of that calendar year, and nothing is retroactive.
- The SPDX identifier is `PolyForm-Noncommercial-1.0.0`, the same string in `[workspace.package]`, `deny.toml`, the container labels and the generated API document. The file carries a `Required Notice:` line, which the license obliges redistributors to keep.
- Contributions are accepted under [`docs/CLA.md`](../CLA.md): the contributor keeps their copyright and grants the maintainer a broad copyright and patent license, including the right to license the contribution on other terms. Without that, past contributions would be stuck under the noncommercial terms and could not be part of a commercial license.
- Dependency licenses stay permissive. `cargo deny` keeps an allowlist of permissive licenses, so nothing copyleft or noncommercial enters the dependency graph.

## Consequences

- Domovoi is source-available, not OSI open source. Packaging in distributions, Homebrew, Nixpkgs and similar repositories may be refused or flagged, and hosted forks must buy a license.
- Commercial use is a sale the maintainer makes directly; the terms of each commercial license live outside this repository.
- Compliance is the user's own check, not the project's: there is no telemetry, host count or audit, so the threshold has to stay legible, and the price above it small enough that paying is easier than avoiding it.
- Reverting to a permissive license later needs the agreement of every contributor, which is why the CLA exists and why it should be signed before a change is merged.
- The `Required Notice` line, the container labels and the API document name the project as the licensor; a fork that drops them is in breach of the notices clause.

## Alternatives considered

- Keep MIT OR Apache-2.0: maximum reach and no friction with packagers, but nothing prevents a closed hosted fork from taking the homelab market this project is built for. Rejected.
- BSL 1.1: free for non-production use, converts to Apache-2.0 after four years, but hobbyists read it as "not free" and the conversion date adds a second set of terms to explain. Rejected.
- FSL-1.1-MIT: allows almost all commercial use short of a competing product and converts to MIT after two years; too permissive to fund development through commercial licenses. Rejected.
- AGPL-3.0-only with a commercial license: a real open-source license that keeps internal business use free, so it does not make commercial use paid. Rejected.
