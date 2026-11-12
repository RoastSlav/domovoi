# 02 — Design pack validation

Each finding: pack reference → repository evidence → problem → consequence if unresolved →
correction → confidence. Classes: **Confirmed, Partial, Incorrect, Missing, Unnecessary,
Unverified**. Evidence letters refer to `investigations/` (A backend, B frontend, C persistence,
D protocol, E security, F testing/ops, G lessons).

## Summary

| Class | Count | Most important |
|---|---|---|
| Confirmed | 9 | SQLite-only, no vector/graph DB, knowledge/policy/experience split, Operation ≠ Incident, human-only approval is a sound base |
| Partial | 12 | capability descriptor ownership, phase model, policy precedence, experience store, UI graph, agent enforcement |
| Incorrect | 6 | provider-declared risk, "dispatched → failed" framing, plan-hash binding, gap analysis claims, lessons count/coverage, motion durations |
| Missing | 11 | transactional writes, outcome-unknown, idempotency, approval freshness, step-up auth, approval rendering, live-event replay, protocol version rule, prompt total cap, hub-side wait for agent, a durable operation state model |
| Unnecessary (for v1) | 5 | offline permits, L3 in v1, four presentation profiles up front, generic entities table, customizable dashboards |
| Unverified | 4 | skill install from UI, real prompt sizes, graph library viability under CSP/bundle, Docker name escaping |

## Confirmed

| # | Pack ref | Evidence | Note | Conf. |
|---|---|---|---|---|
| C1 | 07, D-09 SQLite-first | C: 20 STRICT tables, WAL, scale ~10^4 nodes, <1M rows/yr | keep one file | High |
| C2 | 07, Q2.1/Q2.2 no vector/graph DB | C: FTS5 under-built, not proven insufficient | build lexical search properly first | High |
| C3 | 06, D-07 separation | A §9: none of the three exists in typed form yet | separation is cheap to keep from day one | High |
| C4 | D-24 Operation ≠ Incident | A §4: no aggregate models remediation; triage is timeline strings | RW-04 | High |
| C5 | 08 "strong safety foundation" | E §0, F §1: writer + SQL guard, sealed `HumanApproval`, gates | preserve | High |
| C6 | D-16 no raw shell/SQL | runner argv-only, closed ReadTool (A §6, F §1) | keep as gate | High |
| C7 | D-23 Operator = one feature | B: chat is sessionStorage + single POST | needs durable conversations (RW-20) | High |
| C8 | 05 "knowledge is not authority" | no code path converts history to permission today (E §2) | add gate when suggestions land | High |
| C9 | 10 v0.1 before v1 | F, E: current product has release-relevant defects | see RW-02 | High |

## Partially confirmed — direction right, details wrong or missing

| # | Pack ref | Evidence | Problem | Consequence | Correction | Conf. |
|---|---|---|---|---|---|---|
| P1 | 03 descriptor fields on provider | D §4, verified `triage/tools.rs:149-158` | provider declares its own risk/scope | buggy/compromised provider defines its own authority | hub-owned contracts; agents advertise availability; hub floor on skills (RW-07) | High |
| P2 | 04 phases | A §4–5 | phases presented as lifecycle but also "not rigid"; no state model for waiting/resume | ambiguous resume, rigid playbook by accident | coarse status + typed steps (RW-05) | High |
| P3 | 05 precedence | E §5.2 | "most specific wins" undefined across host vs capability axes; multiple allows undefined; default "approval or deny by class" has no table | non-deterministic policy outcomes | RW-10 precedence + intersection + default table | High |
| P4 | 06/07 experience_events | C, A §9 | a third log beside `incident_events` and `audit_log` → dual writes | drift between logs | journal = experience; audit stays security (RW-15) | Med-High |
| P5 | 07 entities table | C | duplicates baselines' container/unit/port inventory | two inventories drift | typed subject refs + small entities table for user concepts (RW-14) | Med |
| P6 | 07 fact `confidence` | C | model confidence is not calibrated (pack says so itself) | false precision | source + status only | High |
| P7 | 05/D-12 agent enforcement | E §2/5.5: `RunSkill` has no authorization proof | agent cannot tell why it is asked | autonomy on current wire = silent compromise path | `authorization_kind` + local ceiling (RW-12) | High |
| P8 | 14/15 orchestration graph dominant | B §5–7: bundle headroom ~15 kB, no inline style rule, no graph primitive | graph-first delays the whole Operations area | v1 UI blocked on a spike | step timeline first, graph second (RW-21) | Med-High |
| P9 | 12/16 presentation profiles | B §10 | profile ↔ P0–P3 mapping undefined | 4× test matrix per page (~192 cases) | defer; when added, hide/collapse only with a P0/audit-present test | Med |
| P10 | 02 "notifications derive from committed state" | C, G: notify state saved before delivery; audit outside tx | stated as principle, violated today | duplicate/missing cards and audit | unit of work + delivery outbox (RW-03) | High |
| P11 | 18 lesson "capability policy across every consumer" | G: wrapper must be handed in; `server.link()` public, no arch rule | holds today by convention only | next consumer bypasses it | single execution boundary + arch rule (RW-08) | High |
| P12 | 13 page map / approvals | B §6 | approvals appear in 4 places with no canonical home | inconsistent actions | one approval card component, canonical home = operation detail; others link/embed it | Med |

## Incorrect

| # | Pack ref | Evidence | What is wrong | Correction | Conf. |
|---|---|---|---|---|---|
| I1 | 08 "agent dials out over mutually authenticated TLS" | D §1: agent pins hub cert, no client cert; identity by ed25519 challenge bound to the channel | not mTLS (security property is similar, mechanism differs) | describe accurately; matters for any "signed bundle" design | High |
| I2 | 08 action lifecycle "Proposed → … → verify" as robust | A §5, F: verify timer in memory, lost on restart; dispatched at restart → `Failed` | overstates durability | treat verification and outcome-unknown as gaps (RW-09, timers) | High |
| I3 | 18 / 08 plan binding | E §5, G: hash covers manifest not script bytes; approve API carries no hash | overstated binding | plan hash v2 (RW-08) | High |
| I4 | 18 coverage | G: 113 closed PRs, not 110; #112/#113 missed; #108 is the merged v0.0.10 release | index incomplete | use G-lessons.md as the corrected index | High |
| I5 | 18 "enforced in CI" claims | G: mutation-feedback lint is file-level; unsaved-change guard only in notes editor; upgrade is integrity-checked, not "checksum-verified" | overstated safeguards | rework must add real checks, not assume them | Med-High |
| I6 | 15 motion up to 500 ms | B §6: DESIGN.md caps 120–180 ms | contradicts the existing design rulebook | keep DESIGN.md tokens unless you decide otherwise (O-12) | High |

## Missing — not in the pack, required

| # | Concern | Evidence | Required decision |
|---|---|---|---|
| M1 | Atomic state + event + audit | C, A §3 | RW-03 |
| M2 | `OutcomeUnknown` state and agent idempotency journal | D §2, F | RW-09 |
| M3 | Approval expiry and freshness at dispatch, incl. restart catch-up | E risk 6, `executor.rs:88-105` | RW-08 |
| M4 | Restart catch-up runs before agents reconnect → valid approvals rejected | G, verified `executor.rs:48,329-332` | `WaitingAgent` state (04 §3) |
| M5 | Approval surfaces show model prose, not the plan; Discord buttons unlabeled | E risk 1 | RW-22 |
| M6 | Step-up auth for settings that execute programs (LLM CLI) or edit policy | E risk 3, verified `settings/patch.rs:109-116` | RW-24 |
| M7 | Live-update replay (event ids, Last-Event-ID) | B §2 | RW-18 |
| M8 | Protocol: newer minor refused permanently; unknown enum variants fail frames | D §1, §8 | RW-17 |
| M9 | Total prompt-size cap; per-call hard deadline; admission fail-closed and persisted | A §6, G | 04 §4 |
| M10 | How a model-chosen target relates to autonomy (prompt injection → rule trigger) | E risk 4 | RW-23 pinned targets |
| M11 | Durable conversation model | B §3 | RW-20 |

## Unnecessary for v1 (defer, keep the extension point)

| # | Pack ref | Why defer | Keep |
|---|---|---|---|
| U1 | 05 offline emergency permits | clock/replay/revocation/lockout machinery for a rare homelab case (E §5.6) | `authorization_kind`, local ceiling |
| U2 | 10 "first L3 in v1 if gates pass" | firewall actions can sever the agent link (D §9); needs trusted-path model | `Contain` effect class (RW-13) |
| U3 | 12/16 Detailed/Standard/Essential/Custom up front | multiplies page tests; no user data yet | P0–P3 annotations in specs |
| U4 | 07 generic `entities` for everything | duplicates baselines | RW-14 |
| U5 | 09/14 customizable Control Room widgets | persisted layouts add state with unvalidated demand | one good default layout |

## Unverified — needs a spike or evidence

| # | Item | Needed evidence |
|---|---|---|
| V1 | Capability install/deploy from UI (pack 14 §10) | ADR on file transfer to agents, integrity, INV-3 interaction |
| V2 | Real prompt sizes in production (A §6 estimates up to ~170 KiB) | measure with the dev hub and a busy fixture |
| V3 | Graph library under CSP, no-inline-style rule and bundle budget | 1–2 day spike |
| V4 | Docker client escaping of container names (E §3) | read bollard usage / test |

## Design-pack internal contradictions, resolved

| Contradiction | Resolution |
|---|---|
| 04 says phases are "not rigid" but defines them as the lifecycle diagram | phases are step labels; status is coarse (RW-05) |
| 03 provider-registered descriptor vs 05 "provider must not grant its own authority" | hub-owned contracts (RW-07) |
| 05 D-12 local enforcement vs D-27 UI manages everything | local ceiling is a deliberate external-only control, shown read-only in UI (RW-12, decision O-3) |
| 10 "L3 may ship in v1" vs 09 "do not implement L2/L3 while P0 questions open" | L3 deferred (RW-13, O-4) |
| 15 motion durations vs DESIGN.md | DESIGN.md wins unless you decide otherwise (O-12) |
| 13 approvals in four places | one component, canonical home on operation detail |
| 12 Detailed default vs B recommendation Standard default for new users | Detailed only until profiles exist (U3); revisit then |
| 06 "deletion removes from all indexes" vs nightly/pre-migrate backups | document backups as the exception; retention on backups (RW-14) |
