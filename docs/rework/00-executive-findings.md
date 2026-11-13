# 00 — Executive findings

Baseline `64117f0`.
- `docs/design-pack/` is the **product-intent** reference.
- `docs/rework/` is the validated **technical direction**: decisions in `03` (RW-01…41), contracts in
  `04`, decision register in `99` (D-01…D-28, research R-1…R-7).

Files: `01` repository facts · `02` pack validation · `03` decisions · `04` target architecture ·
`05` migration · `06` roadmap, readiness, deferred list · `07` prompt index (draft) · `99` register ·
`investigations/` evidence snapshots.

## Direction

Evolve the existing workspace (RW-01), foundation first inside the rework (D-01):

| Milestone | Scope |
|---|---|
| F1 | atomic writes + audit chain |
| F2 | recovery, approval freshness, delivery durability |
| F3 | exact protocol version |
| F4 | LLM and ops reliability |
| M1 | execution journal and reconciliation, keyed by ActionId |
| M2 | hub-owned contracts and registry |
| M3 | Operation aggregate and journal-backed live stream |
| M4 / M5 / M6 | investigation / knowledge and experience / Operator |
| M7 | policy in shadow |
| P1 | signed packages |
| H1 | privileged helpers |
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
| **Open research** | R-1 graph library; R-2 nftables helper path; R-3 Docker helper path; R-4 Discord nonce; R-5 prompt sizes; R-6 fsync cost; R-7 container-name escaping |

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
- **Host-local limits.**
  - The `[unattended]` ceiling applies to `policy`-labelled runs.
  - The `[mutation_limits]` rate limiter applies to every mutation. It is journal-backed, so a hub
    restart cannot reset it.
  - A compromised hub can label a run `human` and bypass the ceiling: an accepted v1 limitation.
- **Recovery.** The agent journal (epoch, fsync before reply) produces `NotRun` only for `not_found`
  with a matching epoch. Every crash window that leaves an `accepted` record without a result yields
  `OutcomeUnknown` (04 §15.2).

## What changed in this pass

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

## What is now safe to turn into implementation prompts, and what is not yet safe

**Safe now** (architecture defined, owner decisions complete, no unmerged predecessor):
- F1: unit of work, audit chain, atomic writes, crash harness.
- F3: exact protocol version and tolerant decoding.

**Safe once their predecessors merge** (fully specified, no open decision):
- after F1: F2, F4 (the prompt-cap value stays configurable until R-5);
- after F2 + F3: M1;
- after F3: M2;
- after M1 + M2: M3;
- after M3: M4, M5, M6, M7, U1;
- after M2: P1;
- after U1: U3;
- U4 pages after their backend milestone.

**Not yet safe:**
- **H1:** research R-2 and R-3 and two ADRs needed.
- **M8:** needs H1 for the Docker contract. The `unit.restart` part needs M4 + M7.
- **M9:** needs M8, M5 and H1, plus R-2.
- **U2 graph:** needs the R-1 spike. Only the spike prompt is safe.

**Candid assessment.** The architecture is internally consistent and ready for a final review. No
owner decisions remain open. Remaining risk sits in the research items, especially R-2 and R-3
(privileged helper feasibility across supported platforms), which gate L2 Docker actions and all
containment. Those are the most likely places for the design to change.
