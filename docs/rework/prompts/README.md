# Implementation prompts

One file per prompt: `NN-branch-name.md`. `NN` is the stable ID used in `../07-implementation-prompt-index.md`.
Each file is a self-contained, copy-paste-ready brief for one branch and one pull request.

**Every prompt in the index has a file except 44, 45, 46 and 47**, which wait on research evidence or
an owner decision. Prompt 99, the closing cleanup, runs last, after everything else has merged. `../07-implementation-prompt-index.md` is authoritative for prompt status,
dependencies, launch order, gates and the integration branch (its §6); an index row without a file
here is a plan, not a brief, and must not be launched.

- **One at a time.** Prompts are launched manually, one branch and one pull request at a time. `∥` in the
  index means the prompts have no dependency forcing an order — it never means concurrent execution.
- **Migrations are sequential.** A prompt that adds a migration runs only after the previous
  migration-adding prompt has merged, and takes the next free number (index §7).
- **One active V2 branch.** `overhaul-investigation` is the base for all V2 work and holds both the
  internal planning documents and the V2 implementation. Branch from its latest HEAD and open the pull
  request against it; `main` is never a prompt target (index §1.1). The coding agent works in the
  repository checkout and reads `docs/rework/`, `docs/design-pack/` and these prompt files there — no
  second branch, worktree, documentation overlay or sync step is needed.
- **Internal documents stay out of `main`.** `docs/rework/`, `docs/design-pack/` and these prompts are
  never merged into `main`; product documentation under `docs/book/` travels with the feature that
  changes it. Promotion to `main` is a curated branch later, not a merge of the whole V2 branch (index
  §1.2).
- **Authoritative design:** `../03-architecture-decisions.md` (RW-NN), `../04-target-architecture.md`,
  `../04b-attestation-encoding.md`, `../99-open-decisions.md` (D-NN, O-NN, R-NN).
- **Linux research environment:** the owner's Ubuntu WSL2 environment, recorded in
  [`../investigations/H-wsl2-research-environment.md`](../investigations/H-wsl2-research-environment.md).
  Research prompts 53, 54 and 56 may run there for scoped, preliminary evidence; prompt 55 needs no
  environment gate. WSL2 evidence does not accept a research item and does not stand for supported
  conventional Linux hosts.
- **Repository rules:** `AGENTS.md`, `ARCHITECTURE.md`, `TESTING.md`.

The design pack in `docs/design-pack/` is product intent, not an implementation reference.

## Shared contracts and their owning prompts

Exactly one prompt introduces each shared contract. A later prompt consumes it. When a consuming prompt
runs, it must read the owning prompt's merged code and use the actual names. The names listed here are
the planned names.

| Contract | Owner | Planned names (verify in code) |
|---|---|---|
| Unit of work port, transaction, commit errors, journal and effects tables | 08 | `domovoi_core::ports::UnitOfWork`, `Transaction`, `StateWrite`, `JournalDraft`, `AuditDraft`, `EffectDraft`, `CommitError`, `CommitReceipt`; tables `journal`, `effects`, `schema_compat` |
| `Actor` and audit hash chain | 09 | `domovoi_core::Actor`; table `audit_anchor`; audit columns `prev_hash`, `hash` |
| Exact protocol rule, durable incompatibility | 13 | `agents.reported_protocol`, `reported_release`, `last_authenticated_at`, `incompatible_since` |
| Host availability | 14 | `HostAvailability { Known, Unknown, Absent, Incompatible }` |
| Effect lifecycle with lease fencing | 15 | `effects.claim_token`, `claim_generation`, `lease_until` |
| Wire 1.2: complete v1 contract (RW-42) | 20 | `hello` extensions, `agent_status`, `run_capability`, `refused`, `action_status`, `get_catalog`, `transfer_*`, `attest_safety`, `safety_attestation`, `unsupported`, `Open<T>` |
| `ExecutionId`, plan hash v2, `Authorization`, `PolicyPermit`, `KeyFingerprint` | 22 | `domovoi_core::action::{Authorization, PolicyPermit, PlanHash}`, `ExecutionId`, `domovoi_core::host::KeyFingerprint` |
| Capability contracts | 24 | `domovoi_core::capability::{ContractId, EffectClass, CapabilityContract}` |
| Capability registry, availability and target identity | 25 | `domovoi_core::capability::Availability`; ports `domovoi_core::ports::{CapabilityRegistry, TargetResolver}`; `TargetQuery`, `ResolvedTarget`, `CatalogueState`, `canonical_host_identity` |
| Operation aggregate and trace schema | 26 | `Operation`, `OperationStatus`, `Step`, `StepKind`, `StepOutcome`, `StepEdge`, `EdgeKind`; trace JSON golden fixtures |
| Journal-backed SSE | 28 | SSE `id` = journal `seq`; topics; `as_of_seq` |
| Context retrieval port | 31 | `domovoi_core::ports::ContextSource`, `domovoi_core::context::*`, `MetaTool`, `ToolSet` |
| Policy evaluator | 39 | `domovoi_engine::policy::{evaluate, evaluate_shadow, Decision, Rule, simulate, validate_publication}`; `EvaluateRequest`, `Outcome`, `PublicationReport`, `PolicyError` |
| Visibility categories | 37 | `useVisibility(category)`; categories `P0` to `P3` |
| Operation read API, parity suite | 27 | `GET /api/v1/operations`, `/{id}`, `/{id}/trace`, `/{id}/steps/{step}`, step approve/deny routes, `OperationPage`, `OperationDetailDto`, `ExecutionDto`; `tests/services/parity.rs` |
| Journal-backed SSE details | 28 | `GET /api/v1/events`, frames `ready`/`journal`/`reset`, `JournalFeed`, `topics_of`, `snapshot_seq`, `useJournalStream`, `journal_floor` |
| Hub use-case layer | 29 | `crate::services::{Services, Caller, OperationCommands, ...}`; arch rules `MOD-1`…`MOD-4`, `CALLER-1`; control routes `/cancel`, `/pause`, `/resume`, `/steps/{step}/answer` |
| Right slot and approval card | 30 | `web/src/app/rightSlot/` (`RightSlotProvider`, `RightSlotHost`, `RightSlotSurface`, `useRightSlot`), kit `SidePanel`, `ApprovalCard`, smoke mock `npm run smoke` |
| Context registration | 32 | `TriageDeps.extra_context: Vec<Arc<dyn ContextSource>>`, `run_chain` |
| Knowledge store | 34 | `domovoi_core::knowledge::*`, `KnowledgeRead`, `ManagementCoverage`, `KnowledgeSource`, `FactRecordId`, `/api/v1/knowledge/*` (`KnowledgeFactDto`, ...) |
| Experience | 35 | `MetaTool::SearchExperience`, `EffectKind::DraftResolution`, `/api/v1/experience/search`, similar-operation routes |
| Conversations | 36 | `ConversationService`, `AnswerSink`, SSE `live.start`/`live.text`/`live.tool`/`live.end`, `/api/v1/conversations` |
| Policy store | 40 | `StateWrite::Policy`, `PolicyRepo`, `ShadowRecorder`, `/api/v1/policy/*` |
| Packages, agent side | 41 | `domovoi_core::package::*`, `package_contracts()`, `PackageFailureCode`, testkit `PackageFixture` |
| Packages, hub side | 42 | `domovoi_core::install::*`, `PackagePlan`, `/api/v1/packages/*`, `/api/v1/hosts/{name}/packages` |
| Step chat approvals | 59 | `executions.chat_token_hash`, `OperationHandle::authorize(.., chat_token)`, `engine/src/chat_token.rs`, `EventKind::ExecutionChanged` |
| Helper protocol and safety attestation | 47 | helper socket request and response types; `SafetyIdentity`, `ExpectedSafety` (RW-37) |

## Standard blocks every prompt contains

Each prompt copies these sections verbatim, then adds task-specific items.

- "Execution constraints"
- "Repository rules"
- "Completion report"

The text of each block is in `_blocks.md`.
