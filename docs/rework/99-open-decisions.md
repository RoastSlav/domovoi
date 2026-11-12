# 99 — Open decisions that need you

Ordered by how much they block. Each has my recommendation; nothing here is decided until you say so.

| # | Decision | Options | Recommendation | Blocks |
|---|---|---|---|---|
| O-1 | Release v0.1 before the rework? (RW-02) | (a) M0 then tag v0.1, then rework; (b) rework and release later; (c) tag now | (a) | everything |
| O-2 | Approve the core corrections: hub-owned capability contracts (RW-07), Operation as aggregate with coarse status + typed steps (RW-04/05), journal-as-experience with no third log (RW-15), unit of work (RW-03) | accept / amend | accept | M0–M3 |
| O-3 | How autonomy is anchored on the host (RW-12) | (a) agent-local `unattended` list set on the host, read-only in UI; (b) owner-signed policy bundles; (c) trust hub | (a) for v1 | M8 |
| O-4 | L3 containment and offline permits in v1? (RW-13) | defer past v1 / one narrow L3 in v1 | defer | M8, v1 scope |
| O-5 | Policy precedence details (RW-10) | host tags in v1 or host ids only; confirm deny-wins + most-specific + allow-intersection + default table | ids only in v1; confirm the rest | M7 |
| O-6 | May `Destructive` ever be policy-allowed? | never in v1 (enforced in evaluator) / configurable | never in v1 | M7 |
| O-7 | Should model-proposed targets ever be autonomous, or only the operation's pinned subject (RW-23)? | pinned only / rule may list extra targets / any in scope | pinned, plus targets a rule names explicitly | M8 |
| O-8 | Graph canvas in v1, or step timeline in v1 and graph in v1.x (RW-21)? | timeline-first / graph-first | timeline first, graph after spike, still aiming for v1 | U1/U2 |
| O-9 | Retention: incident timeline is deleted at 90 days while docs say incidents are kept; operation step outputs vs summaries | keep summaries forever + raw 90 d / configurable / keep all | summaries kept, raw configurable (default 90 d) | M3 |
| O-10 | Presentation profiles (Detailed/Standard/Essential) in v1? | defer / v1 | defer to after core pages | U3 |
| O-11 | Capability install/deploy to agents from the UI in v1? (needs an ADR on file transfer, integrity, INV-3) | v1 / v1.x | v1.x; v1 shows inventory and enable/disable | U2 |
| O-12 | Motion: keep DESIGN.md 120–180 ms or adopt doc 15's longer durations | keep / amend DESIGN.md | keep | U1 |
| O-13 | Multi-user/roles: stay single-owner through v1? (affects "permission-filtered search", who may edit policy) | single owner / basic roles | single owner, step-up for sensitive edits | M7, search |
| O-14 | Audit hash chain in v1 (tamper evidence) | v1 / later | v1, it is small once audit is in the tx | M0/M1 |
| O-15 | The repo's `ARCHITECTURE.md` is the contract for coding agents; the target changes INV-1 wording and adds aggregates. Update it per milestone (each prompt names the section) or once up front? | per milestone / up front | per milestone | all prompts |
