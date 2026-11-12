# 99 — Owner decisions and remaining open items

## A. Decided by the owner (authoritative; do not reopen)

| ID | Decision | Where applied |
|---|---|---|
| D-01 | No separate v0.1 release; former M0 becomes foundation slices F1–F4 inside the rework | RW-02, 06 |
| D-02 | Hub-owned typed contracts; Operation aggregate ≠ Incident; coarse status + typed steps; unit of work; experience = journal (no third store); past operations easy to retrieve (typed journal linked to subjects, resolution summary per operation, FTS, host-scoped `search_experience`, similar past operations in UI, provenance in reasoning); past approvals/successes are evidence, never authorization | RW-03/04/05/07/15/25 |
| D-03 | Host-local unattended ceiling on the agent; hub cannot modify or widen; UI read-only; hub policy may only narrow; human vs policy authorization distinguishable; agent enforces independently; owner-signed policy bundles after v1 | RW-12, 04 §8 |
| D-04 | L3 containment in v1 as M9 under a general `Contain` class, not limited to firewall; offline emergency permits after v1 | RW-13, 04 §9 |
| D-05 | Most specific rule wins, including over a broader Deny; equal specificity → Deny; host ids in v1, tags after v1 | RW-10, 04 §7 |
| D-06 | `Destructive` never policy-allowed in v1 (evaluator hard limit); human approval with typed confirmation remains | RW-10, 04 §7.3 |
| D-07 | Unattended targets: pinned subject + targets named by the matching rule | RW-23 |
| D-08 | Both step timeline and interactive graph in v1; timeline first; shared frozen trace schema | RW-21, U1/U2 |
| D-09 | All retention values configurable | RW-28, 04 §14 |
| D-10 | Detailed/Standard/Essential profiles in v1 | RW-21, U3 |
| D-11 | Capability install/deploy from the UI in v1 | RW-26, P1 |
| D-12 | Longer motion allowed for visualization/transitions; reduced motion mandatory | RW-21, `web/DESIGN.md` |
| D-13 | Single owner through v1; multi-user after v1; identities ready for roles | RW-29 |
| D-14 | Audit hash chain in v1 | RW-27 |
| D-15 | `ARCHITECTURE.md` updated once per milestone; each prompt names its sections | 06, 07 |

## B. Still open — each has a recommendation and is not assumed in code until answered

| # | Question | Alternatives and consequences | Recommendation | Blocks |
|---|---|---|---|---|
| O-16 | Package trust in v1 | (a) first-party signed only; (b) (a) + owner-added keys on the agent; (c) arbitrary third-party registries. (c) makes every registry author able to ship code that runs as the agent user once the owner clicks Install. | (b) | P1 |
| O-17 | First containment set in M9 | `firewall.block_source`, `container.pause`, `unit.stop_temporary` (04 §9); alternatives add container network isolation, SSH session kill, account lock (weaker recovery stories) | the three listed | M9 |
| O-18 | Scope of the host-local ceiling | (a) capability ids only; (b) + explicit targets + `max_per_hour`; (c) (b) + declared-constrainable args + `max_active`/`max_ttl` for Contain + exclusions | (c), with (b) the minimum for M8 | M8, M9 |
| O-19 | Incomparable conflicting rules | Deny > RequireApproval > Allow (conservative), or "first-created rule wins" (non-deterministic to users), or reject the policy at publish time with an error | Deny > RequireApproval > Allow, plus a publish-time warning listing incomparable conflicts | M7 |
| O-20 | Old agent support window | current + previous two minors until 1.0 (then same major); or only the previous minor; or indefinite (untestable) | current + previous two minors | F3 |
| O-21 | Retention defaults | proposed: signals 30 d (current), incident timeline 90 d (current), operation journal events 180 d, raw execution outputs 30 d, operations + resolution summaries kept, audit kept (checkpoint pruning only if configured), retracted fact content purged after 30 d, conversations 180 d, notifications 30 d, nightly backups 7 (current), pre-migrate backups last 3 | as proposed | F1 config, M3, M5 |
| O-22 | Watchdog / dead-man for Contain contracts that can affect connectivity | required for all such contracts (agent self-reverts if hub confirmation does not arrive within 30 s), or only for `firewall.block_source` | required for all connectivity-affecting Contain contracts | M9 |
| O-23 | Policy `Deny` and the owner: may the owner still approve a step that policy denies? | (a) no — Deny means not offered, owner edits policy; (b) yes with typed confirmation | (a); keeps Deny meaningful and visible | M7 |
| O-24 | Docker socket write access for `container.restart`/`container.pause` | grant the agent write access to the socket (root-equivalent on the host, ADR 0018) or ship a narrow first-party helper that only performs restart/pause of named containers | narrow helper via package; ADR in M8 | M8 |
| O-25 | Historical playback of an operation on the graph in v1 | ship last in U2 after the live graph / defer to v1.x | ship last in U2; the timeline already provides history | U2 |
| O-26 | Should the owner be able to mark a Deny rule as **locked** (not overridable by narrower rules)? | (a) no — D-05 as is; families that must never run are covered only by hard limits (Destructive); (b) yes — an explicit `locked` flag, default off, shown prominently | (b); it keeps D-05 as the default while letting "never on this host" stay true | M7 |
| O-27 | Agent-side `max_per_hour` also for human-approved runs | (a) yes — bounds a compromised hub that labels runs "human"; may block a burst of real approvals; (b) unattended only | (a), with a clear "host rate limit reached" refusal shown on the approval card | M1, M8 |

Technical questions that do not need the owner (decided in 03/04 as PROPOSAL): journal sequence
source, unit-of-work shape, Execution-as-typed-Step, model-call retry rules, tolerant decoding,
audit chain format, legacy-action projection, startup order, profile preference storage (hub-side, single owner).
