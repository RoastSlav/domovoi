# 02 — Design pack validation

Each finding: pack reference → repository evidence → problem → correction → status. Evidence letters
refer to `investigations/` (A backend, B frontend, C persistence, D protocol, E security, F testing,
G lessons). Owner decisions D-NN are in `99`; where a decision overrides the original pack or the
first review, that is stated. The pack documents have been updated to match (see `03`/`04` for the
technical contracts).

## Confirmed

| # | Pack ref | Evidence | Note |
|---|---|---|---|
| C1 | 07, D-09 SQLite-first | C: 20 STRICT tables, WAL, scale ~10^4 nodes, <1M rows/yr | one file (RW-16) |
| C2 | 07 no vector/graph DB | C: FTS5 under-built, not proven insufficient | build lexical search properly first |
| C3 | 06 knowledge/policy/experience separation | A §9: none of the three exists in typed form yet | kept from day one |
| C4 | D-24 Operation ≠ Incident | A §4: no aggregate models remediation | RW-04 (D-02) |
| C5 | 08 "strong safety foundation" | FACT: single action writer + SQL status guard (ADR 0019); `HumanApproval` without `Deserialize`, call sites restricted by an arch-check text scan (not compile-time sealing, RW-11); plan hash on hub and agent; gate tests | preserve and strengthen (RW-11) |
| C6 | D-16 no raw shell/SQL | runner argv-only, closed ReadTool (A §6) | kept as gates |
| C7 | D-23 Operator = one feature | B: chat is sessionStorage + single POST | RW-20 |
| C8 | 05 knowledge is not authority | no code path converts history to permission (E §2) | RW-25 gate |

## Partially confirmed — corrected

| # | Pack ref | Evidence | Problem | Correction |
|---|---|---|---|---|
| P1 | 03 descriptor fields set by provider | FACT `engine/src/triage/tools.rs:149-158`: hub trusts agent-declared `risk` | provider defines own authority | hub-owned contracts (RW-07, D-02) |
| P2 | 04 phases as lifecycle | A §4–5 | rigid vs dynamic; no waiting/resume model | coarse status + immutable steps (RW-05, D-02) |
| P3 | 05 precedence | E §5.2 | specificity undefined across dimensions; multiple allows undefined | partial-order specificity, intersection, defaults, hard limits (RW-10). **Owner changed the principle** from absolute deny-wins to most-specific-wins (D-05) |
| P4 | 06/07 `experience_events` | C, A §9 | third log beside `incident_events`, `audit_log` | one journal; audit separate (RW-15, D-02) |
| P5 | 07 generic `entities` | C | duplicates baselines' inventory | typed subject refs; entities only for user-defined subjects (RW-14) |
| P6 | 07 numeric `confidence` | C | false precision | source + status only |
| P7 | 05/D-12 agent enforcement | FACT `proto/src/messages/work.rs:120-131`: no authorization info on dispatch | agent cannot distinguish authorization kinds | host-local ceiling + `authorization_kind` (RW-12, D-03); not a cryptographic proof |
| P8 | 14/15 graph dominant | B §5–7 | no graph primitive; CSP/inline-style/bundle constraints | both timeline and graph in v1, timeline first, spike first (D-08) |
| P9 | 12/16 profiles | B §10 | category ↔ profile mapping undefined | category-to-visibility table, P0 always visible, in v1 (D-10) |
| P10 | 02 notifications from committed state | FACT: notify state saved before delivery; audit outside tx (G) | principle violated today | unit of work + journal-backed SSE (RW-03/15) |
| P11 | 18 capability policy across consumers | G: wrapper must be handed in; `server.link()` public | holds by convention | single execution boundary + read boundary arch rules (RW-08) |
| P12 | 13 approvals in four places | B §6 | no canonical home | one component, canonical home operation detail (RW-22) |
| P13 | 14 §10 capability install | FACT: hub cannot write files on hosts; skills are owner-placed | install button without a trust model would be hub RCE | signed packages, agent-local keys and opt-in, install ≠ enable ≠ authorize (RW-26, D-11) |
| P14 | 10 L3 "may ship in v1 if gates pass" | D §9: firewall actions can cut the agent link | no first set, no dead-man | L3 in v1 as M9 with a defined first set and watchdog (RW-13, D-04) |

## Incorrect

| # | Pack ref | Evidence | Correction |
|---|---|---|---|
| I1 | 08 "mutually authenticated TLS" | D §1: agent pins hub cert, no client cert; identity by ed25519 challenge bound to the channel | described accurately |
| I2 | 08 action lifecycle as durable | FACT: verify sleep in memory (`recheck.rs:65`); dispatched-at-restart → `Failed` | gaps handled by F2/M1 |
| I3 | 18/08 plan binding | E §5: manifest only, no hash on approve | plan hash v2 (RW-08) |
| I4 | 18 coverage | G: 113 closed PRs; #112/#113 missing; #108 is the merged v0.0.10 release | doc 18 corrected |
| I5 | 18 enforcement claims | G: mutation-feedback lint is file-level; leave guard only in notes editor; upgrade is a same-origin checksum | doc 18 corrected; per-call-site checks added to plan (RW-33) |

**Withdrawn from the first review:** "doc 15 motion contradicts DESIGN.md". DESIGN.md's 120/180 ms
tokens cover micro-interactions; doc 15's longer durations cover visualization and transitions, which
DESIGN.md did not address. DESIGN.md now defines both (D-12).

## Missing in the pack — now specified

| # | Concern | Evidence | Where |
|---|---|---|---|
| M1 | atomic state + journal + audit | C, A §3 | RW-03 |
| M2 | outcome-unknown, agent journal with epoch | D §2, F | RW-09 |
| M3 | approval expiry/freshness incl. restart | FACT `executor.rs:88-105` | RW-08, RW-30 |
| M4 | host availability unknown before connect | STATIC `executor.rs:48, 329-332` | RW-09, RW-30 |
| M5 | structured approval rendering | E risk 1 | RW-22 |
| M6 | step-up for settings that execute programs | FACT `settings/patch.rs:109-116` | RW-24 |
| M7 | live-update replay with sequence | B §2 | RW-15 |
| M8 | protocol version rule; value-preserving tolerant decoding | D §1, §8; FACT `serde(other)` sites | RW-17 |
| M9 | admission persistence, fail-closed, prompt cap, per-call deadline, attribution | A §6, G | 04 §16 |
| M10 | model-chosen targets vs autonomy | E risk 4 | RW-23 (D-07) |
| M11 | durable conversations, stale replies, scope | B §3, G | RW-20 |
| M12 | audit tamper evidence and its limits | E §4 | RW-27 (D-14) |
| M13 | schema compatibility, rollback vs restore | F §3 | RW-31 |
| M14 | legacy action handling | — | RW-32 |
| M15 | spool-before-state, notify-after-delivery | G | 04 §15 |
| M16 | actor identity ready for roles | — | RW-29 (D-13) |

## Deferred (decided)

| Item | Decision |
|---|---|
| Offline emergency permits | after v1 (D-04) |
| Owner-signed policy bundles | after v1 (D-03) |
| Host tags in policy | after v1 (D-05) |
| Multi-user and roles | after v1 (D-13) |
| Customizable Control Room widgets | not decided; recommended after v1 (one default layout in v1) |

## Unverified — needs a spike or evidence

| Item | Needed evidence |
|---|---|
| Graph library under CSP, no-inline-style rule and bundle budget | spike (prompt 33) |
| Real prompt sizes (A §6 estimates up to ~170 KiB) | measure on dev hub with a busy fixture |
| Docker client escaping of container names (E §3) | read bollard usage / test |
| nftables availability and privileges on supported distros for `firewall.block_source` | M9 spike |

## Pack contradictions and how they were resolved

| Contradiction | Resolution |
|---|---|
| 04 phases "not rigid" vs lifecycle diagram | phases are step labels (RW-05) |
| 03 provider descriptors vs 05 "provider must not grant own authority" | hub-owned contracts (RW-07) |
| D-12 local enforcement vs D-27 UI manages everything | host ceiling is a deliberate external-only control, shown read-only (D-03) |
| 05 "explicit deny always wins" vs owner's wish for broad defaults with exceptions | most specific wins, equal → deny; hard limits separate (D-05) |
| 10 "L3 may ship in v1" vs 09 "no L2/L3 while P0 questions open" | L3 in v1 as M9 after M8, with its own gates (D-04) |
| 13 approvals in four places | one component (RW-22) |
| 06 "deletion removes from all indexes" vs backups | backups documented as an exception, count-based retention (RW-28) |
| 14 permission-filtered search vs single owner | hooks kept, filtering after v1 (D-13) |
