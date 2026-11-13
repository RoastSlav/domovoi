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
| D-18 [O-18] | Ceiling scope: exact capability id, explicit targets, max rate, declared constrainable args, max TTL and max active for containment, mandatory management-path exclusions (held in the root-owned helper `[safety]` section and enforced by the helper, RW-37). No entry → no policy-authorized execution | RW-12, 04 §8.3 |
| D-19 [O-19] | Incomparable conflicting maximal rules → Deny > RequireApproval > Allow. Publication shows the conflicts and requires acknowledgement | RW-10, 04 §7.3–7.6 |
| D-20 [O-20] | Exact protocol major.minor match; no backward or forward window; same-protocol patch releases compatible; mismatched agent is `Incompatible` (persisted only after authentication, survives hub restart, history kept), operations on it are `Unavailable(agent_incompatible)`; no endless reconnect loop; coordinated hub-then-agents upgrade; application release floors are not a compatibility dimension; no agent self-update | RW-17, RW-42, 04 §12, 05 §4 |
| D-21 [O-21] | Retention defaults (configurable): signals 30 d; incident timeline raw 90 d; operation journal raw 180 d; raw execution outputs 30 d; operations and summaries retained; audit retained (checkpoint pruning only if configured); retracted fact content purged after 30 d; conversations 180 d; notifications and notification state 30 d; nightly backups 7; pre-migrate backups last 3 | RW-28, 04 §14 |
| D-22 [O-22] | Every containment expires independently of the main agent. Connectivity-affecting containment also needs an independent watchdog. Firewall uses a kernel timeout. No unattended enablement until expiry, recovery, watchdog, outcome-unknown handling and drills exist | RW-13, 04 §9 |
| D-23 [O-23] | Policy `Deny` = not offered for approval; the owner must change the policy. No approval bypass | RW-10, 04 §7.3 |
| D-24 [O-24] | No general Docker socket write access for the agent. A narrow first-party helper with its own ADR. Docker write capabilities unavailable for unattended use until then | RW-39 |
| D-25 [O-25] | Historical graph playback ships last in U2. The timeline and history stay fully useful without it | RW-21, 06 U2 |
| D-26 [O-26] | Locked Deny (default off): an owner-declared prohibition that narrower rules cannot override. Visible in editor, simulator, explanation and trace. Step-up to change | RW-10, 04 §7 |
| D-27 [O-27] | Agent-side mutation rate limits apply to all mutating executions, human- and policy-labelled. Separate from the ceiling | RW-38 |
| D-28 [Q0.5] | Unattended containment on a host requires owner-confirmed, current management paths. Unknown, stale, conflicting or unverifiable → refused. Mandatory exclusions live in the root-owned helper `[safety]` and are enforced by the helper; the hub cannot remove or widen them; a path can be confirmed only when a verified helper attestation shows it is covered. UI shows paths, coverage, provenance, freshness and what is blocked. No effect on monitoring or other human-approved work | RW-37 |
| D-29 | The control agent runs as a dedicated unprivileged OS user; skill and package execution uses a separate identity with no access to the helper socket or agent-private state; privileged host operations go only through one narrow, root-owned helper installed and updated by the owner-controlled OS path, never by capability packages. The helper allowlist bounds operations; it is not execution authorization | RW-39, RW-37, RW-12, RW-26 |

## B. Open owner decisions

| ID | Question | Alternatives and consequences | Recommendation | Blocks |
|---|---|---|---|---|
| O-30 | Skills whose manifest sets `run_as` to a user other than the skill identity `domovoi-skill`, after D-29 | (a) unsupported: reported `Unavailable(run_as_unsupported)`; owner rewrites the skill or runs it outside Domovoi; (b) the skill runner or helper switches to allowlisted (skill, user) pairs — requires a root-capable component that executes package-defined programs, reopening the root path D-29 closes; (c) owner sudoers rules used inside skills — gives `domovoi-skill` root-equivalent reach and bypasses the helper allowlist | (a): keeps every privileged effect behind the typed helper; (b) and (c) let package content run as other users or root | H1 implementation (skill runner refusal rule, selftest report), P1 package guidance |

Technical details under approved directions are PROPOSALs in `03`/`04`.

## C. Research and evidence still needed (not owner decisions)

| ID | Question | Needed before | How resolved |
|---|---|---|---|
| R-1 | Graph library that works under the CSP, the no-inline-style rule and the bundle budget | U2 prompts | spike (prompt 33) |
| R-2 | nftables containment on supported platforms: existing base tables, chains, hooks and priorities; whether packets actually reach the `inet domovoi` set; coexistence with firewalld, ufw, Docker and common host networking; set size and resource limits; TTL validation; atomic delete + add replacement and failure recovery; exclusion enforcement in the helper | M9 prompts | spike + helper ADR; containment is not production-ready until verified |
| R-3 | Helper delivery on supported platforms: systemd socket activation, `SO_PEERCRED`, sandboxing options, container deployments (socket mount), Unraid/no-systemd supervisors; Docker API access from the helper | H1 prompts | spike + helper ADR |
| R-8 | Effective-configuration identity for systemd units (fragment + drop-ins) across supported systemd versions | M8 `unit.restart` | test on supported distros |
| R-9 | Skill-runner isolation on supported platforms: systemd socket activation with `Accept=yes`, `User=`, `InaccessiblePaths=`, `RuntimeMaxSec=`, behaviour on supported systemd versions; container deployments and hosts without systemd (Unraid); verification that `domovoi-skill` cannot reach the helper or agent state; the effective filesystem view of a skill process: attempts to list and read `/etc/domovoi/*`, `/var/lib/domovoi-agent`, `/var/lib/domovoi-helper`, `/run` sockets, other secret-bearing paths, and writes to package/skill sources; feasibility of per-package narrowing | H1 implementation | spike + tests; until done those platforms run without skills or keep the labelled root agent |
| R-10 | Helper safety attestation: canonical encoding of the effective set, key storage and generation at install, fingerprint enrollment UX, nonce lifetime, performance of recomputing the set per request, behaviour across helper restarts and reboots | M9 (unattended containment) and the H1 safety part | spike + tests (`04` §9, RW-37 test list); until done, unattended containment stays blocked |
| R-4 | Discord `nonce` / `enforce_nonce` deduplication behaviour | F2 delivery prompt (affects wording only; at-least-once holds either way) | read docs + test against a test guild |
| R-5 | Real prompt sizes, to set the total prompt cap | F4 LLM prompt | measure on dev hub with a busy fixture |
| R-6 | fsync cost of `synchronous=FULL` and the agent journal on slow storage (SD card) | F1 / M1 | benchmark; documented, not blocking |
| R-7 | (Reframed) Container names are only used for resolution and are validated against Docker's name grammar; all helper operations take ids (RW-34). Remaining work is a test, not research: `container_name_grammar_enforced` | — | covered by RW-34 tests |
