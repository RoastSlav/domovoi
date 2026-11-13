# 99 — Decision register

This register is authoritative. Other documents cite these IDs and must not restate a different
version. The earlier `O-NN` numbers are kept in brackets for traceability. The design pack's own principle
ledger (`docs/design-pack/01`) also numbers its rows D-NN; cite those as "pack D-NN" and these as
"owner D-NN" where both could be meant.

## A. Owner decisions

| ID | Decision | Applied in |
|---|---|---|
| D-01 | No separate v0.1 release; former M0 becomes foundation slices F1–F4 | RW-02, 06 |
| D-02 | Hub-owned typed contracts. Operation aggregate distinct from Incident; coarse status + typed steps. Unit of work. Experience = journal (no third store). Past operations easy to retrieve (typed journal, per-operation resolution summary, FTS, host-scoped `search_experience`, similar operations in UI, provenance). Past approvals are evidence, never authorization | RW-03/04/05/07/15/25/41 |
| D-03 | Host-local unattended ceiling on the agent. Hub cannot modify or widen it; UI shows it read-only; hub policy may only narrow it; agent enforces it. Owner-signed bundles after v1 | RW-12, 04 §8 |
| D-04 | L3 containment in v1 (M9) under a general `Contain` class. Offline emergency permits after v1 | RW-13, 04 §9 |
| D-05 | Ordinary rules: most specific wins, including over a broader Deny; equal specificity → Deny. Host ids in v1, tags after v1 | RW-10, 04 §7 |
| D-06 | `Destructive` never policy-allowed in v1 (evaluator hard limit). Human approval with typed confirmation remains | RW-10, 04 §7.3 |
| D-07 | Unattended targets: pinned subject + targets named by the matching rule | RW-23, RW-34 |
| D-08 | Step timeline and interactive graph both in v1; timeline first; shared frozen trace schema | RW-21, RW-40 |
| D-09 | All retention configurable | RW-28 |
| D-10 | Detailed/Standard/Essential profiles in v1 | RW-21, U3 |
| D-11 | Capability install/deploy from the UI in v1 | RW-26, P1 |
| D-12 | Longer motion for visualization/transitions; reduced motion mandatory | RW-21, `web/DESIGN.md` |
| D-13 | Single owner through v1; identities ready for roles | RW-29 |
| D-14 | Audit hash chain in v1 | RW-27 |
| D-15 | `ARCHITECTURE.md` updated once per milestone; each prompt names its sections | 06, 07 |
| D-16 [O-16] | Package trust in v1: first-party release key + owner keys installed locally on the agent. No third-party registries. Hub cannot add or replace keys | RW-26, 04 §11 |
| D-17 [O-17] | First containment capability: `firewall.block_source` only. `container.pause` and `unit.stop_temporary` deferred until independent expiry and recovery are proven | RW-13, 04 §9 |
| D-18 [O-18] | Ceiling scope: exact capability id, explicit targets, max rate, declared constrainable args, max TTL and max active for containment, mandatory management-path exclusions. No entry → no policy-authorized execution | RW-12, 04 §8.3 |
| D-19 [O-19] | Incomparable conflicting maximal rules → Deny > RequireApproval > Allow. Publication shows the conflicts and requires acknowledgement | RW-10, 04 §7.3–7.6 |
| D-20 [O-20] | Exact protocol major.minor match. Older and newer minors rejected; same-protocol patch releases compatible. No legacy adapter. Coordinated hub-then-agents upgrade. `Incompatible` host state is visible and keeps history. No agent self-update | RW-17, 04 §12, 05 §4 |
| D-21 [O-21] | Retention defaults (configurable): signals 30 d; incident timeline raw 90 d; operation journal raw 180 d; raw execution outputs 30 d; operations and summaries retained; audit retained (checkpoint pruning only if configured); retracted fact content purged after 30 d; conversations 180 d; notifications and notification state 30 d; nightly backups 7; pre-migrate backups last 3 | RW-28, 04 §14 |
| D-22 [O-22] | Every containment expires independently of the main agent. Connectivity-affecting containment also needs an independent watchdog. Firewall uses a kernel timeout. No unattended enablement until expiry, recovery, watchdog, outcome-unknown handling and drills exist | RW-13, 04 §9 |
| D-23 [O-23] | Policy `Deny` = not offered for approval; the owner must change the policy. No approval bypass | RW-10, 04 §7.3 |
| D-24 [O-24] | No general Docker socket write access for the agent. A narrow first-party helper with its own ADR. Docker write capabilities unavailable for unattended use until then | RW-39 |
| D-25 [O-25] | Historical graph playback ships last in U2. The timeline and history stay fully useful without it | RW-21, 06 U2 |
| D-26 [O-26] | Locked Deny (default off): an owner-declared prohibition that narrower rules cannot override. Visible in editor, simulator, explanation and trace. Step-up to change | RW-10, 04 §7 |
| D-27 [O-27] | Agent-side mutation rate limits apply to all mutating executions, human- and policy-labelled. Separate from the ceiling | RW-38 |
| D-28 [Q0.5] | Unattended containment on a host requires owner-confirmed, current management paths. Unknown, stale or conflicting → refused. Agent enforces local exclusions independently. UI shows paths, provenance, freshness and what is blocked. No effect on monitoring or other human-approved work | RW-37 |

## B. Open owner decisions

None at this time. Technical details under approved directions are PROPOSALs in `03`/`04` and may
be refined in review without an owner decision unless they change behaviour the owner sees.

## C. Research and evidence still needed (not owner decisions)

| ID | Question | Needed before | How resolved |
|---|---|---|---|
| R-1 | Graph library that works under the CSP, the no-inline-style rule and the bundle budget | U2 prompts | spike (prompt 33) |
| R-2 | nftables availability, the `inet domovoi` table approach and helper privilege path (sudoers or file capability) on supported distros, Docker images and Unraid | M9 prompts | spike + ADR (RW-39) |
| R-3 | Docker helper privilege path on the same platforms | M8 Docker contracts | spike + ADR (RW-39) |
| R-4 | Discord `nonce` / `enforce_nonce` deduplication behaviour | F2 delivery prompt (affects wording only; at-least-once holds either way) | read docs + test against a test guild |
| R-5 | Real prompt sizes, to set the total prompt cap | F4 LLM prompt | measure on dev hub with a busy fixture |
| R-6 | fsync cost of `synchronous=FULL` and the agent journal on slow storage (SD card) | F1 / M1 | benchmark; documented, not blocking |
| R-7 | Docker client escaping of container names (E §3) | M8 | read bollard usage / test |
