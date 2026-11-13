# Implementation prompts

One file per prompt: `NN-branch-name.md`. `NN` is the stable ID used in `../07-implementation-prompt-index.md`.
Each file is a self-contained, copy-paste-ready brief for one branch and one pull request.

**Not every prompt in the index has been written.** `../07-implementation-prompt-index.md` §6 is the
authoritative list of the files that exist; an index row without a file here is a plan, not a brief, and
must not be launched. Prompts are launched one at a time, and prompts that add a migration are launched
sequentially so their numbers cannot collide (index §7).

- **Execution order and dependencies:** `../07-implementation-prompt-index.md`.
- **Authoritative design:** `../03-architecture-decisions.md` (RW-NN), `../04-target-architecture.md`,
  `../04b-attestation-encoding.md`, `../99-open-decisions.md` (D-NN, O-NN, R-NN).
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
| `ExecutionId`, plan hash v2, `Authorization`, `PolicyPermit` | 22 | `domovoi_core::action::{Authorization, PolicyPermit, PlanHash}`, `ExecutionId` |
| Capability contracts | 24 | `domovoi_core::capability::{ContractId, EffectClass, CapabilityContract}` |
| Capability registry and availability | 25 | `Availability` |
| Operation aggregate and trace schema | 26 | `Operation`, `OperationStatus`, `Step`, `StepKind`, `StepOutcome`, `StepEdge`, `EdgeKind`; trace JSON golden fixtures |
| Journal-backed SSE | 28 | SSE `id` = journal `seq`; topics; `as_of_seq` |
| Context retrieval port | 31 | `ContextSource` (planned) |
| Policy evaluator | 39 | `domovoi_engine::policy::{evaluate, Decision, Rule}` |
| Visibility categories | 37 | `useVisibility(category)`; categories `P0` to `P3` |
| Helper protocol and safety attestation | 47 | helper socket request and response types; `SafetyIdentity`, `ExpectedSafety` (RW-37) |

## Standard blocks every prompt contains

Each prompt copies these sections verbatim, then adds task-specific items.

- "Execution constraints"
- "Repository rules"
- "Completion report"

The text of each block is in `_blocks.md`.
