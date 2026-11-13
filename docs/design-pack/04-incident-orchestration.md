# 04 — Phase-based incident orchestration

## Principle

The work done about an incident is a durable operational task, not a single LLM prompt. That task is an **Operation**, an aggregate distinct from the incident (owner decision D-02, RW-04): the incident is the detected condition, the operation is the work performed, and an operation may also start from a user request or a schedule with no incident at all. The orchestrator owns lifecycle and state; the reasoner handles interpretation and proposes next steps. Phases are responsibilities and contracts, not a rigid linear workflow. The engine may complete simple phases without an LLM call, and paths may loop or branch.

**How phases are stored.** An operation has a **coarse status** (for example running, waiting, paused, succeeded, failed, escalated, cancelled). Its work is a growing list of **typed, immutable steps** (reasoning turn, evidence request, plan, policy decision, approval, execution, verification, notification, question); a step's progress is recorded as append-only events. A phase in this document is therefore a label over a group of step kinds, not a stored state. The orchestrator decides on typed step outcomes, never on model text. State machines and outcomes are in [`docs/rework/04 §3`](../rework/04-target-architecture.md) and RW-05.

## Lifecycle

```mermaid
flowchart TD
  D[Detect / correlate] --> E[Enrich context]
  E --> A[Assess]
  A -->|need more evidence| E
  A -->|understood| P[Plan]
  P --> R{Route plan}
  R -->|observe / notify| M[Monitor]
  R -->|ask owner| U[Wait for user]
  R -->|manual work| H[Human intervention]
  R -->|approval required| AP[Wait for approval]
  R -->|policy allows| X[Execute]
  R -->|unsafe / unsupported| ES[Escalate]
  U --> E
  AP --> X
  X --> V[Verify / monitor]
  V -->|resolved| RES[Resolve]
  V -->|not resolved or regressed| E
  M -->|new evidence| E
```

## Phase contracts

| Phase | Responsibility | Output / exit condition |
|---|---|---|
| **Detect** | Receive checks, probes, webhooks and integration signals; correlate duplicates and update/create incident. | Durable incident identity, initial signal set and source metadata. |
| **Enrich** | Collect relevant host, service, network and dependency evidence; retrieve knowledge and past experience. | Context bundle with evidence references, timestamps, source, freshness and collection status. |
| **Assess** | Build hypotheses; distinguish established facts, inferences, unknowns and contradictory evidence. | Structured assessment and optional requests for additional evidence. |
| **Plan** | Propose one or more possible next steps, including observation, more investigation, notification, question, manual steps, approval, authorized action, verification or escalation. | Typed plan proposal with rationale, target, expected effect, preconditions and postconditions. |
| **Authorize / route** | Apply deterministic policy to each proposed operation and choose the route. | Allowed, denied, approval required, ask user, manual intervention or escalation. |
| **Execute** | Dispatch only authorized operations through the capability provider. | Execution receipt, bounded output, status, timing and side-effect record. |
| **Verify / monitor** | Check whether the original condition is gone and whether dependent services remain healthy. | Verified, verification failed, inconclusive or regression detected. |
| **Resolve / escalate** | Close the incident only when the condition is demonstrably resolved; otherwise continue or escalate. | Terminal state or a new bounded cycle. |

A phase may return `complete`, `needs_evidence`, `waiting`, `failed`, `not_applicable` or `escalate`. Store these outcomes explicitly on the steps; do not treat missing output as a successful phase. An execution whose result is lost (a dropped link, a restart mid-dispatch) is `OutcomeUnknown`, not `Failed`: it is reconciled against the agent's action journal, and it blocks unattended re-execution of the same capability on the same target until a human or a verification step resolves it (RW-09).

**Outcome vocabulary.** The single definition is in [`docs/rework/04 §3.2`](../rework/04-target-architecture.md); the UI and API use it verbatim.

- **Succeeded** and **Failed:** it ran and reported success, or an attempted operation reported failure. `Failed` is only for attempted-and-failed work.
- **NotRun:** only with positive evidence that it never started, or when its authorization or approval expired before dispatch.
- **Unavailable:** the work could not be attempted (host absent past the wait limit, host `Incompatible`, capability not implemented or disabled, evidence source down).
- **OutcomeUnknown:** it may have run and the result cannot be established.
- **Refused:** rejected before running by validation, policy (`Deny`), the agent (ceiling, `allow_risk`, rate limit, exclusion) or a human.

A step is never shown as `NotRun` or `Failed` when `OutcomeUnknown` is the honest answer.

## Context model

Do not create one universal questionnaire or giant prompt. Build context in layers:

1. **Common incident context:** incident/host identity, triggering signal, severity, time, recent changes and current work state.
2. **Domain context:** relevant Docker, storage, network, security, database or application details.
3. **Knowledge context:** applicable facts, constraints, dependencies, preferences and recovery paths.
4. **Experience context:** similar past operations (found through their resolution summaries and the host-scoped `search_experience` read capability), prior plans, failures and successful verification evidence. Each item supplied to the reasoner carries its operation id, date, verified flag and source, inside a block labelled historical evidence, not authorization (owner decision D-02, RW-25).
5. **Action-specific context:** only facts needed to evaluate a specific operation (for example, trusted management addresses before firewall changes).

Every requested field/evidence item should have a state: `available`, `unknown`, `unavailable`, `failed`, `stale` or `not_applicable`. Missing data is not equivalent to `false`. Include source and observed time. Distinguish user-confirmed, automatically observed and inferred information.

## Dynamic evidence requests

The reasoner may request further evidence at any assessment stage. The engine validates that the requested capability is read-only/appropriate, in scope and within collection/time/token budgets. Deduplicate repeated requests and retain references so the same expensive query is not run without reason.

A request should identify what is needed and why, not dictate an arbitrary shell command. The engine maps it to an allowed capability and supplies a concise result or a structured failure.

## Planning and routing

A plan may contain several independent steps and multiple routes. For example: notify the owner immediately, collect more logs, then request approval for a service restart; or execute an owner-authorized temporary IP block and still ask the owner to review the broader exposure.

The plan is a proposal, not authorization. The policy engine evaluates each consequential operation separately. One denied step must not silently authorize another. Plans should state dependencies and ordering explicitly.

## Repeated cycles and no blind retries

- Verification failure returns to assessment with the failed operation and its output as evidence.
- A failed or uncertain operation is recorded and not blindly repeated.
- A revised plan must explain what changed and why the next attempt is different.
- Re-evaluate policy and preconditions before every new operation.
- Apply per-incident/per-capability cycle and rate limits.
- Support delayed verification and observation windows for transient conditions.
- If evidence, budget or confidence is insufficient, pause, ask the owner or escalate.

## Durable experience timeline

Record phase transitions and meaningful events with timestamp, actor/component, incident, input/output reference, evidence references, decision/rationale, policy result, operation identity and outcome. Preserve enough to reconstruct the lifecycle without persisting secrets or unlimited raw logs.

The experience record **is the journal**: one append-only table of typed events that also feeds live UI replay, not a separate experience store (RW-15). Security-relevant facts (who authorized what, policy and settings changes, refusals) go to the separate hash-chained audit record, written in the same transaction. When an operation ends it gets a **resolution summary**: deterministic fields filled by code (subjects, capabilities executed, verification results, outcome) and narrative fields (problem, cause, fix) drafted by the model, marked as model-sourced and editable or confirmable by the owner. Summaries belong to the Operation aggregate and the operation writer is their only writer; the Knowledge UI sends edit commands to it (RW-41). The UI shows similar past operations on incident and operation pages. A past approval or success never satisfies policy.

## Delivery semantics

Notification delivery is **at-least-once** (RW-35). If the platform accepts a message and the hub crashes before recording it, the message is retried and a duplicate is possible; each message carries a short stable id so duplicates are recognisable, and remote idempotency is used where the platform supports it. The product never claims exactly-once delivery. Notification state is written only after a confirmed success, and a failure to read it fails the delivery rather than becoming default state. Side effects are claimed under a lease with a claim token: a worker whose lease expired and was reclaimed can no longer commit any local state, so only remote delivery can repeat, which is why it stays at-least-once (RW-35).

## Example: a fix reveals a second problem

A Plex container is unhealthy. Domovoi inspects logs and dependencies, then proposes or performs an owner-authorized Plex restart. The restart command succeeds, but the application health check still fails. Domovoi records that failure as new evidence, returns to assessment, notices database-connection errors, checks the database service and its recent history, then proposes a separate database restart. If that operation is not covered by autonomous policy, it asks for approval. After the database restart it verifies both the database and Plex, watches for regression, and resolves only when the original Plex condition is healthy.

The important behavior is not the Plex-specific sequence: it is **failed postcondition → new evidence → revised diagnosis → new authorization decision → verification**. No blind retry and no assumption that a successful process exit means the incident is fixed. A retry is always a new execution step with its own authorization; reconciling an unknown outcome is not a retry.

## LLM efficiency

An LLM conversation may span multiple phase transitions when useful; phase boundaries do not require one model call each. Use concise summaries and on-demand detail. Enforce incident-wide limits for turns, tokens, tool calls, time, cost and execution cycles.
