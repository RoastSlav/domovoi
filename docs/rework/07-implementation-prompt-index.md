# 07 — Implementation prompt index and execution guide

Each prompt is one branch and one pull request. The prompt files live in `prompts/NN-branch-name.md`.
They are self-contained: copy one into a local coding model working in this repository.
Shared-contract ownership and the standard blocks are in `prompts/README.md` and `prompts/_blocks.md`.

**Status: written except the gated H1, M8 and M9 prompts.** 49 of the 53 prompts in §2 have a prompt
document; 44, 45, 46 and 47 do not, because their design waits on research evidence or an owner decision
(§6.4). A prompt with a file is launchable only when its **Depends on** prompts are merged and its
**Gate** is met (§1). Readiness per milestone is in `06` (Readiness).

**One active V2 branch.** `overhaul-investigation` is the only long-lived V2 branch, and it holds both
kinds of content: the internal planning documents (`docs/rework/`, `docs/design-pack/` and the prompt
files) and the V2 application implementation with its product documentation. Every prompt branch is cut
from its HEAD and opens its pull request back into it; the coding agent works in the repository checkout
and reads the planning documents and prompt files there. No second branch, worktree, copied
documentation overlay or synchronization step is involved. The workflow is §1.1, the migration rule is
§7, and keeping the internal documents out of `main` is §1.2.

**Implementation state.** Prompts 08–24 and 57 are merged into `overhaul-investigation`; as this is
written `crates/domovoi-store/migrations/` on that branch ends at `0011`. That number moves as prompts
merge, so read the directory instead of this sentence. `main` does not contain the V2 work and is not a
prompt target. `v2-overhaul` is a historical ref only: it is not an active base, not a PR target, and it
is not deleted or rewritten.

## 1. How to use this guide

1. **One prompt at a time.** The owner launches prompts manually: one branch, one pull request, nothing
   else in flight. A prompt is finished when its PR is merged and its post-merge checks have run.
2. **What makes a prompt launchable.** It has a file in `prompts/` (§6); every prompt in its **Depends
   on** column is merged into the implementation base branch; its **Gate** is met. A prompt document
   that exists but whose prerequisite is not in the base branch is not launchable — having the file is
   not enough.
3. **Branch from the base and target the base.** Branch each prompt from the latest
   `overhaul-investigation` HEAD after the preceding prompt has merged, and open its PR against
   `overhaul-investigation`. Never target `main`: it holds no V2 implementation prompt and no planning
   document (§1.2).
4. **After the merge, check on the base.** Run the checks listed in the prompt's completion report on the
   updated `overhaul-investigation`, and update `ARCHITECTURE.md` as that prompt instructs (D-15). Cut
   the next prompt branch from that updated branch.
5. If a prompt reports a blocker or discrepancy, resolve it before launching anything that depends on
   it.

**`∥` means order-flexible, not concurrent.** Where §2's **Parallel with** column or §3 joins prompts
with `∥`, it says only that no dependency requires one of them before the other. It is not permission
or instruction to run two branches, two PRs or two agents at the same time; the owner still launches
one prompt at a time and may pick either order. Three things override that flexibility:

- **Migrations (§7).** A prompt that adds a migration runs only after the previous migration-adding
  prompt has merged, whatever the table says.
- **Shared files.** Prompts that edit the same files keep the order §3 gives them (14 before 15, 38
  before 52), because the later one must be written against the merged earlier one.
- **Dependencies and gates.** A prompt's **Depends on** column and its **Gate** always win over the
  general ordering.

**Model suitability:**
- **S** (small) = mechanical, fully specified.
- **M** (medium) = a few local judgement calls.
- **L** (large) = cross-cutting or security-critical. Use the strongest local model and review
  carefully.

### 1.1 The single-branch workflow

The owner keeps one active V2 branch. `overhaul-investigation` is not a documentation branch and not an
overlay: it is the development branch, and the planning documents live in it alongside the code.

1. `overhaul-investigation` is the base for all V2 work: implementation, migrations, product
   documentation and the internal planning documents.
2. A prompt implementation branch is created from the latest `overhaul-investigation` HEAD.
3. Only one prompt is launched at a time.
4. Its pull request targets `overhaul-investigation`.
5. After merging, the post-merge checks run on the updated `overhaul-investigation`.
6. The next prompt branch is created from that updated branch.
7. The coding agent works directly in the repository checkout and can read `docs/rework/`,
   `docs/design-pack/` and the prompt files normally.
8. No second branch, worktree, copied documentation overlay or synchronization mechanism is required.
9. `v2-overhaul` is not an active base and not a PR target. It may remain as a historical branch or
   ref; it is never deleted and its history is never rewritten. Older documents that describe prompt 08
   arriving through it are history, not instructions.

`main` is a separate line. It receives neither the internal planning documents nor any implementation
prompt, and it is not a base for V2 prompt branches.

### 1.2 Internal documents stay out of `main`

This is a deliberate repository policy, not an accident of the current branch state. The following must
never be merged into `main`:

- `docs/rework/`;
- `docs/design-pack/`;
- `docs/rework/prompts/`;
- internal investigation and research records that are not intended as product documentation.

Product documentation is different: `docs/book/` is part of normal implementation PRs whenever a feature
changes documented product behaviour, and it reaches `main` with that feature.

**Promotion.** When V2 is ready for `main`, create a curated promotion branch from `main` and transfer
only the intended product implementation, migrations, tests and product documentation. Do not merge the
entire `overhaul-investigation` branch into `main`, and do not merge its planning documents. The
promotion keeps the internal prompts and planning documents as material that was used during
development, without publishing them as part of the product repository history or tree. Promotion is
not part of any prompt in this guide.

## 2. Prompt table

The table is the plan for all 53 prompts. Only the IDs marked in §6 have a file; the others are
not ready to launch. `Depends on` names prompts, not files. `Parallel with` names prompts that have no
dependency forcing an order (§1): it is not permission to run them at the same time.

| ID | Branch | Milestone | Scope | Depends on | Parallel with | Produces | Gate | Model |
|---|---|---|---|---|---|---|---|---|
| 08 | `store-unit-of-work` | F1 | UnitOfWork port, SQLite + InMemory impl, contract suite, `journal`/`effects`/`schema_compat`, `version` columns, `synchronous=FULL`, `quick_check` | — | — (already present) | UnitOfWork, tables | — | L |
| 09 | `audit-hash-chain` | F1 | `Actor`, audit anchor + chain, `audit verify`, startup verify | 08 | — (adds a migration) | Actor, audit chain | — | M |
| 10 | `atomic-action-writes` | F1 | action writer via UnitOfWork; audit failure fails transition; token rotation via writer | 09 | 11 | — | — | M |
| 11 | `atomic-incident-writes` | F1 | incident manager via UnitOfWork; timeline into journal | 09 | 10 | — | — | M |
| 12 | `crash-harness` | F1 | CrashingStore, restart helper, crash sweep, N-1 upgrade fixture | 10, 11 | — | testkit harness | — | M |
| 13 | `exact-protocol-rule` | F3 | exact major.minor, check after auth, durable Incompatible, agent idle state, floors removed; no wire change | 08 | 16, 57 | Incompatible state | — | M |
| 14 | `recovery-order` | F2 | HostAvailability, startup order, dispatch freshness checks | 10 | 16 | HostAvailability | — | M |
| 15 | `durable-timers` | F2 | `Wake` effects, verification resume, effect lifecycle with lease fencing, triage reload | 08, 10, 14 | 16 | effect lifecycle | — | L |
| 16 | `delivery-durability` | F2 | spool-before-state, notify-after-delivery, read errors | 08 | 14, 15 | — | — | M |
| 17 | `llm-reliability` | F4 | admission persistence, per-call deadline, prompt cap, attribution with usage source and optional cost, redaction, host-scoped similar (capability negotiation and estimation already integrated) | 08 | 18, 57 | - | - | M |
| 18 | `structured-approvals` | F4 | structured cards (web, Discord); approve binds the **v1** hash | 10 | 17, 19, 57 | — | — | M |
| 19 | `ops-readiness` | F4 | step-up list, `/readyz`, metrics auth, backup verify, restore, pre-migrate pruning | 08 | 18, 57 | — | — | M |
| 57 | `configurable-retention` | F4 | D-21 retention config and pruning for existing data | 08 | 17, 18, 19 | retention config | — | S |
| 20 | `agent-action-journal` | M1 | **protocol 1.1 → 1.2** (complete v1 wire, incl. `hello.skills[].entry_script_digest`), agent journal, `action_status`, mutation limiter | 13, 14, 15, 16, 17, 18, 19 | - (21 depends on it) | wire 1.2, `ScriptDigest` | - | L |
| 21 | `outcome-unknown-reconcile` | M1 | OutcomeUnknown, reconcile on hello | 14, 20 | — | — | — | M |
| 22 | `plan-hash-v2-and-authorization` | M1 | plan hash v2 (manifest hash + the 1.2 entry-script digest, and the canonical target incl. the enrolled key fingerprint), approval binding columns, `Authorization` wrapper, `PolicyPermit` with a restricted `issue`, CI enforcement | 18, 20, 21 | - | ExecutionId, Authorization, KeyFingerprint | - | L |
| 23 | `single-execution-boundary` | M1 | one execute path, one read boundary, arch rules; keeps the v2 approval hash and the v1 `run_skill` local check apart | 22 | - | - | - | M |
| 24 | `capability-contracts` | M2 | contract types, built-ins, skill adapter + floor | 23 | 53–56 | contract types | — | M |
| 25 | `capability-registry` | M2 | availability (per `ExecutionPath`), agent catalogue + `hello.skills` consistency, replace `PolicyLink`, target identity incl. the enrolled key fingerprint, inventory UI | 23, 24 | 39 | registry | — | L |
| 26 | `operation-aggregate` | M3 | operations/steps/edges/executions, writer, decide, effect runner, simulator, trace schema | 23, 25 | 39 | Operation model, trace schema | — | L |
| 27 | `investigate-operation` | M3 | triage as Investigate operation (flag), parity suite, legacy projection, operation read endpoints, step approve/deny on the web | 26, 28 | — | operation read API, parity suite | — | L |
| 28 | `journal-sse` | M3 | SSE ids/replay/reset, topics, `as_of_seq`, `incident_events` copy, journal floor | 26 | — | SSE contract, `snapshot_seq` | — | M |
| 29 | `use-case-layer` | M3 | hub application services (`crate::services`, `Caller`), module-edge rules, operation cancel/pause/resume/answer routes | 26, 27, 28, 59 | — | use-case services, control routes | — | M |
| 58 | `remove-legacy-triage-path` | M3 | remove the triage queue and legacy proposal creation, follow-up answers become plain text, flip the flag default | 27, 28, 59 | 29, 30 | — | parity suite green | M |
| 59 | `chat-step-approvals` | M3 | chat (Discord) approve/deny buttons for Execution steps, per-step chat tokens | 27 | — | step chat tokens | — | M |
| 60 | `remove-legacy-executor` | M3 | one execute path, legacy verification retired, remaining legacy actions settled with an audited reason | 58 | — | — | actions parity checkpoint green (05 §7) | L |
| 30 | `operations-ui-timeline` | U1 | shell right slot, Operations list/detail, step timeline, inspector, the one approval card, banners with controls, browser smoke test | 27, 28, 29 | 31, 34, 40, 41, 58 | right slot, `ApprovalCard`, smoke mock | — | M |
| 31 | `meta-tools-and-validation` | M4 | closed meta-tools, plan validator, target resolution, remove text protocol | 27, 58 | 30, 40, 41 | ContextSource port | — | L |
| 32 | `layered-context` | M4 | layered context aggregator, caps, statuses, one provider loop, `TriageDeps.extra_context` | 31 | 36, 37, 42 | context registration point | — | M |
| 34 | `knowledge-store` | M5 | facts/relations/entities, rename, backfill, management-path facts, knowledge API, knowledge context source | 11, 27, 29, 31, 32 | 36, 37, 42 | knowledge API | — | M |
| 35 | `experience-search` | M5 | resolution summary fields, FTS, `search_experience`, experience context source, similar operations | 27, 31, 32, 34 | 36, 37, 42 | experience API | — | M |
| 36 | `operator-conversations` | M6 | conversation aggregate, streaming (`live.*`), Operator panel as a right-slot surface, workspace | 28, 29, 30 | 32, 34, 35, 37 | conversations, `live.*` | — | M |
| 37 | `visibility-profiles` | U3 | visibility categories, profiles, preferences endpoint | 29, 30 | 32–36 | visibility categories | — | M |
| 39 | `policy-evaluator` | M7 | pure evaluator incl. locked Deny, hard limits, publication helpers | 24 | 25, 26 | evaluator API | — (O-31 settled: UTC, D-30) | L |
| 40 | `policy-shadow` | M7 | policy versions, shadow decisions, simulator, publication report, step-up | 27, 29, 39 | 30, 31, 41 | policy API | — | M |
| 41 | `packages-agent` | P1 | agent package verification/install/uninstall over `transfer_*` | 20, 25 | 30, 31, 40 | `package_contracts()`, agent receiver | — | L |
| 42 | `packages-hub` | P1 | owner import, artifact store, relay, install jobs, audit | 29, 41 | 32–37 | packages API | — | M |
| 33 | `graph-spike` | U2 | R-1 spike: library choice, evidence, DESIGN.md amendment proposal | 30 | 32–37, 42 | R-1 evidence | — | M |
| 38 | `graph-canvas` | U2 | interactive graph, keyboard model, motion | 33, 37 | 43, 48–51 | — | R-1 accepted | M |
| 52 | `graph-playback` | U2 | historical playback (last in U2) | 38 | 43, 48–51 | — | — | M |
| 43 | `policy-pages` | U4 | policy editor, explanation, simulator UI, publication report | 37, 40 | 48–51 | — | — | M |
| 48 | `knowledge-pages` | U4 | knowledge browser and detail, proposals, management paths view | 37, 34, 35 | 43, 49–51 | — | — | M |
| 49 | `capability-center-and-install-wizard` | U4 | inventory, owner import, install wizard, read-only ceiling view; helper and key-enrollment screens as labelled placeholders until 47 | 37, 25, 42 | 43, 48, 50, 51 | ceiling read endpoint | — | M |
| 50 | `audit-and-retention-pages` | U4 | audit page (legacy and chained), retention settings | 37, 09, 57 | 43, 48, 49, 51 | — | — | S |
| 51 | `control-room-attention-and-search` | U4 | Control Room, attention centre, global search | 37, 28, 30 | 43, 48–50 | — | — | M |
| 53 | `spike-nft-containment` | R-2 | evidence on supported Linux platforms | — | — | R-2 evidence | WSL2 environment available (§6.3) | M |
| 54 | `spike-helper-and-skill-runner-platforms` | R-3, R-9 | evidence on socket activation, sandbox and filesystem view | — | — | R-3, R-9 evidence | WSL2 environment available (§6.3) | M |
| 55 | `spike-safety-attestation` | R-10 | scratch implementation reproducing `04b` vectors, parser, route state, locking, adversarial tests | — | — | R-10 evidence | — (no environment gate) | L |
| 56 | `spike-systemd-unit-identity` | R-8 | evidence on effective unit identity | — | — | R-8 evidence | WSL2 environment available (§6.3) | S |
| 44 | `helper-adrs` | H1 | ADRs for the three identities and for helper attestation | — | — | ADRs | 54 and 55 evidence (or unproven items recorded) | M |
| 47 | `three-identities-helper-and-skill-runner` (may be split 47a/b/c) | H1 | helper, attestation, skill runner, installer migration, hub enrollment | 20, 22, 44 | — | helper protocol, SafetyIdentity | **R-3, R-9, R-10 accepted** (O-30 settled as D-31) | L |
| 45 | `policy-permit-autonomy` | M8 | PermitIssuer, ceiling, guardrails, `unit.restart`, `container.restart`; the unresolved-`OutcomeUnknown` unattended-reuse guard | 32, 40, 47 | — | — | **R-8 accepted; H1 complete** | L |
| 46 | `containment` | M9 | `firewall.block_source` with kernel timeout, verified and unverified paths, drills | 45, 34, 47 | — | — | **R-2, R-10 accepted** | L |

## 3. Recommended launch order

Waves run top to bottom, **one prompt at a time**. Within a wave, items joined with `∥` have no
dependency between them, so the owner may take them in either order; `∥` is not concurrent execution and
does not suspend the migration rule (§7) or the shared-file order (§1).

1. **Wave A, foundation:** 08 (already present in the implementation line), then 13. Both add
   migrations, so they run one after the other (§7); 13 takes the next free number after 08's. The
   research prompts 53, 54, 55 and 56 have no dependencies: each can be written at any point and
   launched whenever its gate in §2 is met, including during wave A; 53, 54 and 56 run on the owner's
   Ubuntu WSL2 environment (§6.3), and 55 needs no additional environment. They change no product code
   and add no migration. All four have a file (§6.3).
2. **Wave B:** 09, after 13 has merged, so its audit migration takes the next number.
3. **Wave C:** 10 ∥ 11.
4. **Wave D:** 12 ∥ 14 ∥ 16 ∥ 18 ∥ 57, then 17, then 19: 17 and 19 each add a migration and run one at
   a time (§7). Then 15, after 14: both edit `engine/src/actions/{executor,writer,mod}.rs`; 15 adds no
   migration.
5. **Wave E, protocol bump:** 20. This is a coordinated hub and agent upgrade.
6. **Wave F:** 21, then 22, then 23.
7. **Wave G:** 24, then 25 ∥ 39.
8. **Wave H, operations core:** 26, then 28, then 27, then 59, then 29. Each edits hub wiring the next
   one builds on, so this order is fixed.
9. **Wave I:** 58 (once 27's parity suite is green) ∥ 30 ∥ 40 ∥ 41; then 31 (after 58). 60 once its
   gate is green.
10. **Wave J:** 32, then 34, then 35 (each consumes the previous one's context registration or API);
    36 ∥ 37 ∥ 42 ∥ 33 anywhere in this wave (36 and 37 need 30; 42 needs 41).
11. **Wave K, UI pages:** 43 ∥ 48 ∥ 49 ∥ 50 ∥ 51. They share shell files (`nav.ts`, `routes.tsx`,
    `pages.ts`, `tone.ts`, the smoke mock), so each is written against the previous one merged. Then 38
    (gate R-1), then 52.
12. **Wave L, privilege boundary:** 44, once the 54 and 55 evidence exists; then 47 (or 47a, 47b,
    47c in order), once R-3, R-9 and R-10 are accepted.
13. **Wave M, autonomy:** 45, once R-8 is accepted; then 46, once R-2 and R-10 are accepted.

## 4. Integration contracts that must exist before dependants start

| Contract | Owner | First consumers |
|---|---|---|
| `Transaction`, `CommitError`, `CommitReceipt`; `advisories.kind` CHECK removed (new advisory kinds are Rust-only) | 08 (present in this branch line) | 09, 10, 11, 15, 16, 19, 26 |
| effect lifecycle and lease fencing | 15 | 16, 26 |
| wire 1.2 (complete RW-42 list, frozen golden frames, `hello.skills[].entry_script_digest`) | 20 | 21, 22, 25, 41, 47 |
| `ScriptDigest`, `SkillSummary::entry_script_digest` | 20 | 22, 25 |
| `ExecutionId`, `Authorization`, plan hash v2 (manifest hash + entry digest) | 22 | 23, 26, 45 |
| capability contract types | 24 | 25, 31, 39 |
| capability registry, `Availability`, target identity | 25 | 26, 41, 49 |
| trace schema (Rust and web fixtures) | 26 | 28, 30, 38, 52 |
| operation read API, parity suite | 27 | 29, 30, 58 |
| step chat tokens, `OperationHandle::authorize(.., chat_token)` | 59 | 29, 58, 60 |
| hub use-case layer (`crate::services`, `Caller`, `MOD-*` rules) | 29 | 30, 34, 36, 37, 40, 42 |
| right slot, `ApprovalCard`, smoke mock | 30 | 36, 43, 48–51 |
| context registration (`TriageDeps.extra_context`) | 32 | 34, 35 |
| knowledge API, `KnowledgeRead`, `ManagementCoverage` hook | 34 | 35, 47, 48 |
| experience API, `SearchExperience` | 35 | 48, 51 |
| conversations, `live.*` SSE events | 36 | — |
| policy store and API | 40 | 43, 45 |
| package contracts, agent receiver | 41 | 42, 47 |
| packages API | 42 | 49 |
| SSE contract | 28 | 30, 36, 51 |
| ContextSource port | 31 | 32, 34, 35 |
| visibility categories | 37 | 38, 43, 48–51 |
| evaluator API | 39 | 40, 43, 45 |
| helper protocol and SafetyIdentity | 47 | 45, 46, 48, 49 |

A wire change after prompt 20 has merged is a coordinated protocol decision (RW-42). It is never a
silent edit by a later prompt.

## 5. Milestone coverage

Every milestone has a prompt, and the research gates have spikes. All are written except 44–47 (§6.4).

| Milestone | Prompts |
|---|---|
| F1 | 08–12 |
| F2 | 14–16 |
| F3 | 13 |
| F4 | 17–19, 57 |
| M1 | 20–23 |
| M2 | 24–25 |
| M3 | 26–29, 58–60 |
| M4 | 31–32 |
| M5 | 34–35 |
| M6 | 36 |
| M7 | 39–40 |
| P1 | 41–42 |
| H1 | 44, 47 |
| M8 | 45 |
| M9 | 46 |
| U1 | 30 |
| U2 | 33, 38, 52 |
| U3 | 37 |
| U4 | 43, 48–51 |
| Research | 53 (R-2), 54 (R-3, R-9), 55 (R-10), 56 (R-8) |

The remaining research items need no prompt of their own:
- R-1 is prompt 33.
- R-4 is a step in 16.
- R-5 is a step in 17.
- R-6 is a measurement in 08 and 20.
- R-7 became a test in 25.

Deferred items (`06`, "Deferred and after-v1") get no prompts.

## 6. Prompt file status

Every one of the 53 prompt IDs in §2 appears in exactly one group below. An ID without a file in
`prompts/` cannot be launched, whatever its dependencies say. The launch state is a snapshot: check
`git log --oneline overhaul-investigation` before treating a row as pending.

### 6.1 Written prompts (49)

- **Merged:** 08–24, 57.
- **Research, launchable at their gate (§6.3):** 53, 54, 55, 56.
- **Written, waiting for their dependencies:** 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37,
  38, 39, 40, 41, 42, 43, 48, 49, 50, 51, 52, 58, 59, 60. 25 and 39 are next (each needs only 24). 38 is also gated
  on R-1 accepted, 58 on 27's parity suite and 60 on the actions parity
  checkpoint.

Prompts that consume a contract from an unmerged prompt say so in their **Depends on** row and mark each
borrowed name "introduced by prompt NN — verify names in code before use".

### 6.2 Prompts added during authoring (2)

Writing the M3 prompts showed two pieces of work no row owned. They are in §2 as **59**
(`chat-step-approvals`: without it, making investigations the default in 58 would remove one-tap chat
approval) and **60** (`remove-legacy-executor`: the legacy executor removal and the audited expiry of the
remaining legacy actions that 05 §2 requires). Both are written and counted in 6.1.

### 6.3 Research prompts (4, written)

| ID | Research | Produces | Launch gate | WSL2 assessment |
|---|---|---|---|---|
| 53 | R-2 nftables containment | R-2 evidence on supported Linux platforms | Ubuntu WSL2 available | available for preliminary experiments; full packet-path and coexistence validation remains open |
| 54 | R-3 / R-9 helper and skill runner | R-3, R-9 evidence on socket activation, sandbox and filesystem view | Ubuntu WSL2 available | available for initial systemd, socket and sandbox research |
| 55 | R-10 safety attestation | R-10 evidence: `04b` vectors, parser, route state, locking | **no additional environment gate** | available for cryptographic, parser, key-lifecycle and state tests |
| 56 | R-8 systemd unit identity | R-8 evidence on effective unit identity | Ubuntu WSL2 available | available for initial systemd unit identity tests |

**Research environment.** Ubuntu WSL2 on the owner's workstation, recorded with its details, the
owner-executed tests and the untested areas in
[`investigations/H-wsl2-research-environment.md`](investigations/H-wsl2-research-environment.md). WSL2 is a
virtualised, NAT-networking environment with a Microsoft kernel: it makes scoped experiments possible and
is not evidence for supported conventional Linux hosts. Every research report names its environment and
labels each finding **demonstrated on WSL2**, **not demonstrated**, or **requiring conventional Linux
validation**.

**Nothing is accepted by availability.** R-2, R-3, R-8, R-9 and R-10 stay open until the evidence each
requires exists; acceptance is recorded in the ADRs and the decision register, never by a research prompt.
R-10 in particular stays an implementation validation gate: the attestation design is not validated
until its implementation and tests demonstrate it.

### 6.4 Unwritten downstream prompts waiting on evidence or an owner decision (4)

No file yet. Their design depends on evidence or an owner decision, so they are neither written nor
launchable until the gate below is satisfied.

- **44** (`helper-adrs`): needs 54 and 55 evidence, or the unproven items recorded.
- **45** (`policy-permit-autonomy`): needs R-8 accepted and H1 complete.
- **46** (`containment`): needs R-2 and R-10 accepted.
- **47** (`three-identities-helper-and-skill-runner`): needs R-3, R-9 and R-10 accepted
  (its `run_as` rule is settled: D-31). When written it must also replace 49's helper and key-enrollment placeholders and implement
  34's `ManagementCoverage` hook (both prompts list what it must add).

Counts: 49 written + 4 unwritten = 53.

## 7. Migration numbering

Migrations live in `crates/domovoi-store/migrations/`. Prompt 08 added **two** files,
`0006_unit_of_work.sql` and `0007_advisories_kind_unchecked.sql`, so any plan that assumed `0006` for
prompt 13 and `0007` for prompt 09 was wrong. The same mistake is easy to repeat, because the starting
number is not the same everywhere:

- the only number that counts is the next free one in `crates/domovoi-store/migrations/` on the actual
  `overhaul-investigation` implementation branch, read when the prompt branch is cut. As this is written
  that directory ends at `0009`, and the number rises each time a migration-adding prompt merges, so
  treat it as an observation and re-read the directory;
- `main` still ends at `0005_ack_rule_creator.sql`, because prompt 08 has not reached it. Its directory
  is never the source of a migration number;
- neither is a historical `v2-overhaul` checkout, nor an example number in a planning document or in a
  prompt's own text.

So `0010` on the implementation branch and `0006` read from `main` would both look like "the next free
number" — which is exactly why the number is read from the directory on the implementation branch and
never assumed.

Rules for every prompt that adds a migration:

1. **Inspect, then number.** List `crates/domovoi-store/migrations/` on the branch the prompt will land
   on — its PR target, `overhaul-investigation` (§1 step 3) — and take the next unused number. Never take
   a number from a prompt, from this guide, from `main`, or from an earlier V2 checkout.
2. **Never edit or renumber a merged migration.** Add a new one.
3. **Two unmerged branches, one number:** the branch that merges later renumbers its own file before
   integration. Nothing else is renumbered.
4. **Contents, not numbers.** A prompt specifies its migration's contents and its name suffix
   (`_agent_protocol_report`, `_audit_chain`, ...). Its number is not part of the contract, and tests are
   named after the behaviour, not after a file number.
5. **One at a time.** Migration-adding prompts run sequentially, never in parallel with each other. In
   the recommended order of §3 they are 13, then 09, then 17 and 19 in wave D; later 22, 26, 28, 59, 40, 34, 35, 36 and 42 each add
   one (check each prompt's section 8). On a branch
   that contains prompt 08 that sequence started at `0008`; the numbers it uses next depend on what has
   already merged. That sequence is an illustration of the rule, not a promise: verify the directory
   before every one of them.
