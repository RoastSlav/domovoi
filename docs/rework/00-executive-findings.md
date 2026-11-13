# 00 — Executive findings

Baseline `64117f0`.
- `docs/design-pack/` is the **product-intent** reference.
- `docs/rework/` is the validated **technical direction**: decisions in `03` (RW-01…41), contracts in
  `04`, decision register in `99` (D-01…D-29, open O-30, research R-1…R-8).

Files: `01` repository facts · `02` pack validation · `03` decisions · `04` target architecture ·
`05` migration · `06` roadmap, readiness, deferred list · `07` prompt index (draft) · `99` register ·
`investigations/` evidence snapshots.

## Direction

Evolve the existing workspace (RW-01), foundation first inside the rework (D-01):

| Milestone | Scope |
|---|---|
| F1 | atomic writes + audit chain |
| F2 | recovery, approval freshness, delivery durability |
| F3 | exact protocol rule, authenticated durable `Incompatible` state (no wire change) |
| F4 | LLM and ops reliability; approval cards bound to the v1 hash |
| M1 | execution journal, plan hash v2, the single coordinated protocol bump 1.1 → 1.2 |
| M2 | hub-owned contracts and registry |
| M3 | Operation aggregate and journal-backed live stream |
| M4 / M5 / M6 | investigation / knowledge and experience / Operator |
| M7 | policy in shadow |
| P1 | package backend (wizard in U4) |
| H1 | unprivileged agent + root-owned helper (D-29) |
| M8 | L2 autonomy |
| M9 | firewall containment |
| U1–U4 | UI track |

## v1 scope

| Class | Items |
|---|---|
| **v1 commitments** | operations with steps, timeline and graph (D-08); dynamic investigation; knowledge and experience search (D-02); durable Operator; Detailed/Standard/Essential profiles (D-10); policy editor, simulator, locked Deny (D-26); signed package install from the UI, first-party + owner keys (D-11, D-16); audit chain (D-14); configurable retention (D-21); exact protocol compatibility (D-20) |
| **v1 but gated by safety prerequisites** | L2 unattended execution (M8: needs M4, M7, H1, host ceiling); `container.restart` unattended (needs the Docker helper ADR, D-24); L3 `firewall.block_source` (M9: kernel timeout, 30 s confirmation, management paths D-28, drills D-22); historical graph playback (last in U2, D-25) |
| **Planned after v1** | owner-signed approvals and policy bundles; offline emergency permits; `container.pause` / `unit.stop_temporary` and other containment candidates; host tags; multi-user/RBAC; advanced audit anchoring; more integrations; customizable widgets — each in `06` "Deferred and after-v1" with conditions |
| **Intentionally unsupported** | older protocol generations (D-20); agent self-update (INV-3); arbitrary third-party registries in v1; general Docker socket write for the agent; exactly-once external delivery; vector/graph databases without measured need |
| **Open owner decision** | O-30: skills with `run_as` another user after D-29 (recommended: unsupported) |
| **Open research** | R-1 graph library; R-2 nftables containment behaviour; R-3 helper delivery on supported platforms; R-4 Discord nonce; R-5 prompt sizes; R-6 fsync cost; R-8 systemd effective-config identity (R-7 reframed into a test) |

## Key architecture choices

- **One owner per aggregate.** Execution is a typed Step owned by the operation writer.
  - One dispatch attempt per execution; a retry is a new step.
  - `execution_id` is `ActionId` before M3 and the step id after, and never changes once dispatched
    (RW-36).
- **One journal.** A global sequence assigned inside the single-writer transaction. It is the
  experience record, the change stream and the source for SSE replay.
  - Audit is a separate hash-chained table in the same transaction; legacy rows are anchored by a
    digest.
  - Effects use leases with at-least-once delivery (RW-35).
- **Policy.**
  - Locked Deny is a hard prohibition, then the most specific rule wins, even over a broader Deny.
  - Ties go to Deny; incomparable conflicts resolve Deny > RequireApproval > Allow.
  - Hard limits come last.
  - Deny is never offered for approval.
  - Publication validation and the simulator use the evaluator.
- **Authorization.**
  - `HumanApproval` is unchanged; plan and session binding are stored on the execution.
  - The approve request must carry the displayed plan hash.
  - Plan hash v2 binds contract, implementation and **resolved target identity** (container id,
    unit file hash, canonical network prefix).
  - Constructor restrictions are CI controls, not runtime security.
- **Privilege boundary (D-29).** The agent runs as `domovoi-agent`. Privileged operations go only
  through one root-owned helper with an owner-edited allowlist, installed by the OS path, never by
  packages. Hosts still running the agent as root get no unattended execution.
- **Host-local limits.**
  - The `[unattended]` ceiling applies to `policy`-labelled runs.
  - The `[mutation_limits]` rate limiter applies to every mutation. It is journal-backed, so a hub
    restart cannot reset it.
  - A compromised hub can label a run `human` and bypass the ceiling: an accepted v1 limitation.
- **Recovery.** The agent journal (epoch, fsync before reply) produces `NotRun` only for `not_found`
  with a matching epoch. Every crash window that leaves an `accepted` record without a result yields
  `OutcomeUnknown` (04 §15.2).

## Previous pass

- **Decisions applied:** D-16…D-28.
- **HumanApproval and plan binding:** the field mismatch between RW-11 and 04 §8.1 is resolved.
- **Packages:** the circular package-signature format is replaced by detached signatures with key
  rotation, revocation and a rollback rule.
- **Execution identity:** M1/M3 identity sequencing is defined (RW-36).
- **Crash windows:** specified.
- **Audit genesis:** unified (RW-27).
- **Outcome vocabulary and target identity:** unified (04 §3.2, RW-34).
- **Step graph:** typed edges (RW-40).
- **Resolution summaries:** per-field provenance (RW-41).
- **Effect delivery:** semantics defined (RW-35).
- **Containment:** narrowed to firewall-only, with a kernel-timeout watchdog.
- **Privileged helpers:** an H1 milestone for them (RW-39).
- **Durability:** `synchronous=FULL` moved deliberately into F1.
- **Roadmap:** readiness matrix and deferred list added.

## This pass

Corrected: authenticated, durable `Incompatible` state and agent idle state (RW-17); release floors
removed; one coordinated wire bump in M1 (RW-42); unprivileged agent and root-owned helper (D-29,
RW-39) with installer migration; approval stages F4 (hash v1) → M1 (v2) → M3 (steps); resolution
summaries owned by the operation writer (RW-41); equal-scope conflicts in publish validation;
effect lease fencing (RW-35); reboot-safe rate limiter (RW-38); management-path ⊆ agent-exclusion
invariant (RW-37); atomic nftables TTL replacement; package import, signer record, archive limits,
valid TOML (validated with Python `tomllib`); systemd identity over fragment + drop-ins (RW-34).
Root `package-lock.json` removed.

Source behaviour that still has to be implemented to match: protocol check order and exact rule
(`hub/src/agent_server/handshake.rs:165`, `proto/src/version.rs:22`), durable incompatibility
(`registry.rs:142` is in memory), agent exit on final reject (`agent/src/link/mod.rs:163`), release
floors, root agent unit (`install.sh:97`), and everything listed per milestone in `06`.

## What is now safe to turn into implementation prompts, and what is not yet safe

**Safe now:** F1; F3.

**Safe once predecessors merge:** F2 and F4 after F1; M1 after F2, F3 and F4; M2 after M1; M3 after
M1 and M2; M4, M5, M6, M7, U1 after M3; P1 backend after M2; U3 after U1; U4 pages after their backend
(install wizard after P1 and U3).

**Blocked:**
- **H1** (unprivileged agent + helper): O-30, R-3 and the helper ADR.
- **M8**: H1 and R-8.
- **M9**: M8, M5, H1 and R-2. nftables containment is not production-ready until R-2.
- **U2 graph**: R-1 spike first.

**Assessment.** Consistent and ready for review, but not fully implementation-ready: one owner
decision (O-30) and the helper/nftables research (R-2, R-3, R-8) sit on the critical path to L2 and
L3 autonomy and to the privilege boundary itself.
