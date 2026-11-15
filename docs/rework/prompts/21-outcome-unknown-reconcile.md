# 21 - OutcomeUnknown reconciliation

| | |
|---|---|
| Branch | `outcome-unknown-reconcile` |
| Milestone | M1 |
| Scope | `OutcomeUnknown` as a terminal outcome; reconciling dispatched executions against the agent's action journal on authenticated `hello` |
| Depends on (must be merged into `overhaul-investigation` before implementation) | 14, 20 |
| Parallel with | none; 22 depends on this one |
| Produces | `ActionStatus::OutcomeUnknown` and `ActionStatus::NotRun`, the journal epoch recorded at dispatch, the hub-side reconciliation pass, its audit kinds and its tests |
| Gate | none beyond the listed dependencies |
| Suggested model | medium |

Drafting and launch are different. This brief is written now because `03 RW-09` and `04` §15.1–15.2 are its
authoritative contract, and because prompt 20's document freezes the wire it consumes. Implementation starts
only when 14 and 20 have both merged. At the time this was written, 10, 13 and 15 were merged; 14 and 20 had
documents but no merged implementation. Do not weaken the dependency list to match current progress.

The document-drafting dependency is only that prompt 20's document exists: this brief reads its `hello`
extensions, `action_status` types and journal semantics, and does not restate or alter them. The
implementation dependency is 20's merged code, because the reconciliation pass calls the real
`action_status` handler and reads the real `journal_epoch`. If 20's merged code differs from its document in
a way that changes an answer in §5, stop and report before writing the mapping.

## 1. Objective

Make the hub tell the truth about an execution it sent and can no longer see the result of.

Today the hub fails every action left `Dispatched` when it starts, with a fixed reason saying the answer went
to a session that is gone (FACT: `crates/domovoi-engine/src/actions/recover.rs:28-31`, called from
`writer.rs:112`). That converts "may have run" into "failed", and it is wrong in the direction that matters:
somebody then runs the write again. After this prompt the hub keeps the execution `Dispatched` across its own
restart, and asks the agent's journal what became of it when that agent authenticates. The answer becomes one
of four facts, never a guess:

1. it finished, and the recorded result is committed;
2. it is still running in this agent instance, so the hub keeps waiting for it;
3. it never reached the agent, so it is `NotRun`;
4. its outcome cannot be established, so it is `OutcomeUnknown`, terminal, and it must block unattended reuse
   of the same contract on the same target until a person resolves it or a verification proves the state.

This prompt records that requirement and the outcome that satisfies it; it does **not** enforce the blocking
rule, because M1 has no unattended dispatch for it to gate. The `RAN_STATUSES` change below feeds only the
existing hourly host-and-skill loop guard, which is not the same check and must not be described as one. The
enforcement, and its test, belong to prompt 45 with the first policy-authorized unattended dispatch (`§12`,
`22 §2`).

The single rule: uncertainty is never converted into success or failure.

## 2. Scope and non-goals

This prompt owns:

- `domovoi-core`: `JournalEpoch`, `ActionStatus::OutcomeUnknown`, `ActionStatus::NotRun`, the two new
  transitions (including `Approved → NotRun` on expiry), the epoch recorded at dispatch, `RAN_STATUSES`, and
  the `HostReconciler` and `AgentLink::action_status` / `AgentLink::journal_epoch` contracts.
- `domovoi-engine`: the reconciliation pass (`actions/reconcile.rs`), the reconcile write command, the two
  audit kinds, the expiry path that resolves an approved action to `NotRun`, removal of the restart failure,
  and the tests for every crash window in `04` §15.2 plus the expiry case.
- `domovoi-hub`: triggering the pass after an authenticated `hello`, the `WsAgentLink` implementations of the
  two new port methods, `PolicyLink` forwarding, and the DTO's reason text.
- `domovoi-testkit`: scripted journal answers on `FakeAgentLink`, and a recording `HostReconciler`.
- `web/`: the two new status names in the vocabulary and the components that group statuses, so the new
  state is never rendered as "Running".
- `docs/book`: the action status list and the recovery paragraph.

This prompt does **not** own, and must not implement:

| Owner | Excluded work |
|---|---|
| 20 | the agent action journal, its epoch, `action_status`, the `hello` extension fields, the 1.2 message types and golden frames, orphan killing on the agent |
| 22 | plan hash v2, `ExecutionId`, `Authorization`, `PolicyPermit`, approval binding columns, CI authorization checks |
| 23 | the single execution boundary and its arch rules |
| 25 | capability registry, availability model, capability catalogue |
| 26 | the operation aggregate, `executions` table, the `Reconcile` effect and dispatch redesign |
| 46 | containment records, active containment reconciliation and `active_containment_reconciled_after_agent_restart` |
| 58 | removing the legacy action path |

Consequences:

- Do not add a wire message, a protocol field or a protocol version. Everything needed is frozen in 1.2 by
  prompt 20. A change there is a coordinated `1.3` decision, not this prompt.
- Do not add a retry, a re-dispatch or a "force outcome" endpoint. A person resolves uncertainty by reading
  the host and, if the work is still needed, approving a new action through the existing path.
- Do not add a capability, a permit, a policy decision or an availability model. Reconciliation records an
  outcome; it authorises nothing.
- Do not touch `dispatch`'s transition rules, the loop guard, verification or the approval paths, beyond the
  epoch argument that `dispatch` now takes and the expiry outcome in §5.6. **The one expiry change this
  prompt owns** is `Approved → NotRun` when the approval expires before dispatch; the `Proposed → Expired`
  sweep, the incident-gone and incident-resolved rejections, and everything after dispatch stay as they are.
- Row 8 of `04` §15.2 (active containment) is not implemented here. The pass ignores containment because the
  1.2 journal carries no containment record; do not invent one.

## 3. Verified current repository state

Read every file below before editing. Line numbers were verified while writing this; re-read them, and record
any material difference before changing code.

| Area | File | State when this was written |
|---|---|---|
| restart failure | `crates/domovoi-engine/src/actions/recover.rs` | `RESTART_REASON` (`:30`), `scan` (`:37`), `scanned` (`:77`), `approved` (`:126`), `dispatch_history` (`:142`), `fail_abandoned` (`:207`): lists `status = dispatched`, calls `action.complete(Err(RESTART_REASON))` and commits with `ActionGuard::Status("dispatched")`. This is the behaviour this prompt replaces |
| startup order | `crates/domovoi-engine/src/actions/writer.rs` | `run` (`:108`) calls `recover::fail_abandoned` first (`:112`) and then signals `shared.recovered` (`:131`); `complete` (`:360`) maps the agent's answer to `Dispatched → Succeeded/Failed`; `transition` is the one write path for an action |
| recovery watch | `crates/domovoi-engine/src/actions/mod.rs` | `Shared.recovered` (`:143`) is a `watch::Sender<bool>` the executor waits on; `commit_action` (`:155`) is the one commit of action + audit + journal, and takes `extra: Option<(&str, &str)>` for one extra journal field (`:180-182`) |
| executor | `crates/domovoi-engine/src/actions/executor.rs` | `run` (`:248`) waits for the host, dispatches, then calls `deps.link.run_skill` (`:334`) and `ask_complete`; a late reply is not re-requested |
| action statuses | `crates/domovoi-core/src/action/request.rs` | `ActionStatus` (`:15`) has 10 variants and `Dispatched` is a unit variant (`:25`); `NAMES` (`:46`), `name()` (`:62`); `StoredAction` (`:93`) has no epoch |
| transitions | `crates/domovoi-core/src/action/transition.rs` | `ActionChange` (`:11`), `ActionError` (`:20`), `dispatch` (`:104`) takes only the plan hash, `complete` (`:128`) is `Dispatched → Succeeded/Failed`, `verified` (`:142`); every method checks `require` first |
| status storage | `crates/domovoi-store/src/repos/actions.rs` | `status` is bound as JSON (`:123` `json(action.status())`) and read with `from_json::<ActionStatus>` (`:75`); filters and guards use `json_extract(status, '$.type')` (`:186`, `:243`); the column list `action_columns!` (`:22`) has one `status` column |
| schema | `crates/domovoi-store/migrations/0001_init.sql` | `actions` (`:108-127`): `status TEXT NOT NULL` with no CHECK (`:116`), `plan_hash`, `approved_by`/`approved_channel`/`approved_at`; the directory ends at `0010_llm_admission.sql` |
| ran statuses | `crates/domovoi-core/src/ports/repos/actions.rs` | `ActionFilter` (`:15`) has `host` and `status`; `RAN_STATUSES` (`:79`) is `dispatched, succeeded, failed, verified, verify_failed` |
| link port | `crates/domovoi-core/src/ports/agent_link.rs` | `AgentLink` has `run_tool`, `run_skill`, `run_check`, `presence`, `availability` (prompt 14), `skills`; no journal query, no epoch |
| hub session | `crates/domovoi-hub/src/agent_server/session.rs` | `run_session` (`:17`) registers the live session (`:43`) and then serves frames; a reply nobody waits for is logged and dropped (`:139-142`) |
| hub handshake | `crates/domovoi-hub/src/agent_server/handshake.rs` | `authenticate` (`:133`): challenge, signature, exact compare, durable report, `Reject{Incompatible}` (`:207-227`); returns `Accepted { agent, hello, session }` (`:240`) |
| hub dispatch | `crates/domovoi-hub/src/agent_server/registry.rs` | `Registry::dispatch` (`:163`) encodes a hub-initiated request, returns the `RequestId` and a `oneshot`; `deliver` (`:196`) hands a reply to a waiting caller; `session_state` (`:215`) |
| hub link impl | `crates/domovoi-hub/src/agent_server/link.rs` | `request` (`:94`) is the hub → agent request with a `Clock` deadline; `availability` (`:251`) maps `Incompatible` from the stored report |
| hub wiring | `crates/domovoi-hub/src/agent_server/mod.rs` | `AgentServerDeps` (`:57`) holds `clock, hosts, facts, sink, audit, probes, hub_host`; built in `runtime/mod.rs:210` and `admin/mod.rs:129` |
| dto | `crates/domovoi-hub/src/api/dto.rs` | `ActionDto.status` is a plain `String` (`:297`); `reason` is filled only for `Rejected`/`Failed` (`:317-322`) |
| api filter | `crates/domovoi-hub/src/api/actions.rs` | rejects an unknown `status` query value using `ActionStatus::NAMES` (`:57-61`) |
| web vocabulary | `web/src/design/status.ts`, `design/actionVocab.ts` | `ActionStatus` is a hand-written union with 10 members (`status.ts:42-52`); `ACTION_STATUSES` is a table keyed by it (`actionVocab.ts:5`); `design/timeline.ts:105` resolves `action.<status>` audit kinds through that table |
| web status sets | `web/src/features/incidents/ActionSections.tsx:12,31`, `web/src/features/approvals/ActionCard.tsx:25,74`, `web/src/features/approvals/useActionOutcomes.ts:5,20` | hand-written `Set`s and `case`s over the status names; an unknown name falls through to "done" |
| roadmap | `ARCHITECTURE.md:160-172` | the action state diagram ends at `Dispatched → Succeeded/Failed` |

Unavailable today: `OutcomeUnknown`, `NotRun`, `JournalEpoch`, `action_status`, `journal_epoch`,
`HostReconciler`, any epoch on an action, any reconciliation. `03 RW-09` and `04` §15.1–15.2 are the intent.

Discrepancies found while verifying, to record in the pull request:

- `02` §4 lists `crates/domovoi-hub/src/runtime/dispatcher.rs` as key existing code for this prompt. It is
  the event-bus notification consumer (`dispatcher.rs:1`), not an execution dispatch path. It is not touched.
- `02` §4 says a migration is expected and points at `investigations/D-protocol.md:175`, which names a store
  text column. The actual schema needs no column: see §10.
- `04` §15.1 names `[orchestration] agent_wait_limit`. That config key does not exist; `ActionSettings::
  offline_wait` plays that role today (`actions/mod.rs:56`, 30 minutes) and is not changed here.
- `AgentServerDeps` is also built by `admin/mod.rs:129`, which has no action service. The new dependency must
  therefore be optional there; see §7.
- Prompt 14's `startup_recovery_runs_before_catch_up` asserts the ordering this prompt deletes. It must be
  rewritten, not removed: see §12.

What exists, what this prompt adds, and what a later prompt owns:

| Exists in current source | This prompt adds | A later prompt owns |
|---|---|---|
| `Dispatched` as a unit variant, failed on hub restart | the epoch recorded at dispatch, four reconciliation answers, two terminal outcomes | 26: the `executions` table, the `Reconcile` effect, verification of a reconciled success |
| `AgentLink` with dispatch and availability | `journal_epoch`, `action_status` as hub-side port methods | 25: availability derived from the catalogue |
| one writer, one unit of work per action transition | a reconcile write command using the same commit | 26: operation-level reconciliation rules |
| `action_status` frozen on the wire by 20 | the mapping from its answers to hub state | 46: containment reconciliation |

## 4. The protocol 1.2 contract this prompt consumes

Frozen by prompt 20; do not restate it, do not change it, do not add to it.

- `hello.journal_epoch` is the epoch of the agent's journal. It is a `Uuid`, always present on a live 1.2
  session, and it changes only when a local owner runs `journal reset`.
- `ActionStatusQuery { execution_ids: Vec<Uuid>, expected_epoch: Uuid }`, at most 64 ids, answered in request
  order, more than 64 refused with `error{code: "bad_request"}`.
- `ActionStatusAnswers { epoch, answers: Vec<ActionStatusAnswer> }`. `answers[].state` is
  `finished{result}`, `running`, `not_found` or `outcome_unknown{reason}` with
  `reason ∈ {no_start_record, agent_restarted, journal_lost}`.
- `not_found` is positive evidence only with a matching epoch and a journal that has never pruned. The agent
  answers `outcome_unknown{journal_lost}` instead in every other case, including an epoch mismatch, a corrupt
  or fenced journal, and a pruned journal.

In M1 the execution id is the action id (`RW-36`), so `execution_id = action.id().as_uuid()`. `ActionId`
already has `from_uuid` and `as_uuid` (`crates/domovoi-core/src/id.rs:27,33`).

## 5. Reconciliation: trigger, ordering and cases

### 5.1 Trigger

Reconciliation for a host starts after that host's agent has authenticated, never before. In
`agent_server/session.rs::run_session`, after `inner.registry.register(host, live, remote_ip)` has made the
session addressable, ask the reconciler for that host once. Nothing is read, written or sent for a client that
has not signed a challenge (INV-10, `handshake.rs:47-68`).

The pass is per host and per authenticated `hello`. There is no startup pass over all hosts: a host that has
not reconnected is a host whose answer is unknown, and pretending otherwise is what this prompt removes. The
executor's existing catch-up for `Approved` actions is unchanged.

Do not block the session loop on the pass. Spawn it as a task owned by the server's shutdown token, and let
the normal frame loop run beside it; a slow or dead agent must not stop `ping`/`pong`, signals or results.

### 5.2 The epoch at dispatch

`04` §3.4 requires the dispatch transaction to record `Dispatched{agent_epoch, at}`. In M1 the action row is
the execution record (`RW-36`), so the epoch is recorded on the action: `ActionStatus::Dispatched { agent_epoch:
Option<JournalEpoch> }`.

- The writer reads it from `link.journal_epoch(host)` inside the dispatch decision, so the epoch recorded is
  the one the live session reported, not one cached elsewhere.
- `None` means the hub could not read an epoch at that moment. It is stored as `None`, never guessed.
- `#[serde(default)]` is required on the field, because rows written before this prompt are
  `{"type":"dispatched"}` with no field and must still load.

### 5.3 The pass

```text
reconcile_host(host):
  epoch := link.journal_epoch(host)                 // None: no live session, return
  pending := every action with host and status "dispatched", paged to the end
             (a listing that fails is an error: reconcile nothing this time, log it)
  loop:
    (matching, stale) := partition pending by agent_epoch == Some(epoch)
    for each stale action:
        agent_epoch == None            -> OutcomeUnknown { EpochUnknown }
        agent_epoch != Some(epoch)     -> OutcomeUnknown { EpochChanged }
    running := []
    for each batch of at most 64 matching ids:
        answers := link.action_status(host, batch, epoch, now + reconcile_query_timeout)
        on LinkError: log; drop this batch this round (rows stay Dispatched)
        on answers.epoch != epoch: every id in the batch -> OutcomeUnknown { EpochChanged }
        otherwise: one commit per answer, as in 5.4
        collect the ids answered `running`
    if running is empty: return
    rounds += 1; if rounds > RECONCILE_ROUNDS: log warn; return   // rows stay Dispatched
    if link.journal_epoch(host) != Some(epoch): return            // session replaced
    clock.sleep_until(clock.now() + settings.reconcile_interval)
```

- `RECONCILE_ROUNDS` (240) and `settings.reconcile_interval` (30 s) bound a run that keeps answering
  `running`. Exhausting the bound leaves the rows `Dispatched` and logs it. It never fails them.
- Only ids that answered `running` are queried again. A `finished`, `not_found` or `outcome_unknown` answer is
  terminal and is not re-queried.
- Every answer is looked up by `execution_id`, not by position, even though 1.2 answers in request order. An
  answer for an id that was not asked is logged and ignored.
- A `finished` answer under a matching epoch is authoritative even if a later epoch claims loss. First
  terminal outcome wins; the guarded write in §8 enforces it.

### 5.4 Answers and outcomes

| `action_status` answer | Committed outcome | Reason text | Audit kind |
|---|---|---|---|
| `finished{result}` with a successful outcome | `Succeeded` | - | `action.finished` |
| `finished{result}` with a failed outcome | `Failed { reason }` from the result's code and message | the agent's message | `action.finished` |
| `running` | nothing; the id is re-queried | - | - |
| `not_found` | `NotRun { reason }` | `NOT_RUN_REASON` | `action.not_run` |
| `outcome_unknown{no_start_record}` | `OutcomeUnknown { reason }` | `NO_START_REASON` | `action.outcome_unknown` |
| `outcome_unknown{agent_restarted}` | `OutcomeUnknown { reason }` | `AGENT_RESTARTED_REASON` | `action.outcome_unknown` |
| `outcome_unknown{journal_lost}` | `OutcomeUnknown { reason }` | `JOURNAL_LOST_REASON` | `action.outcome_unknown` |
| an answer echoing another epoch | `OutcomeUnknown { reason }` | `EPOCH_CHANGED_REASON` | `action.outcome_unknown` |
| the action's recorded epoch differs from `hello.journal_epoch` | `OutcomeUnknown { reason }` | `EPOCH_CHANGED_REASON` | `action.outcome_unknown` |
| the action has no recorded epoch | `OutcomeUnknown { reason }` | `EPOCH_UNKNOWN_REASON` | `action.outcome_unknown` |
| no session, timeout, `unsupported`, `error` | nothing | - | - |

Reason constants live beside `RESTART_REASON`'s replacement in one place, are short and plain, and never say
or imply success or failure:

```text
NOT_RUN_REASON         "the agent has no record of this run; nothing was started"
NO_START_REASON        "the agent recorded this run but not its start; it may not have run"
AGENT_RESTARTED_REASON "the agent restarted while this ran; check the host before running it again"
JOURNAL_LOST_REASON    "the agent lost its record of this run; check the host before running it again"
EPOCH_CHANGED_REASON   "the agent's journal was reset since this ran; the result cannot be recovered"
EPOCH_UNKNOWN_REASON   "this ran before the agent kept a journal; the result cannot be recovered"
EXPIRED_REASON         "the approval expired before the action was dispatched"
```

`EXPIRED_REASON` is `NotRun`'s second source (§5.6) and reuses today's sentence, so no rendered text changes.
It is not a journal token: the `outcome_reason` extra carries `expired` for it, and the tokens above for the
journal path.

The journal payload also carries the machine-readable reason token (`no_start_record`, `agent_restarted`,
`journal_lost`, `epoch_changed`, `epoch_unknown`) through `commit_action`'s `extra` field as
`outcome_reason`, so a test and a later UI can distinguish two `OutcomeUnknown` rows without parsing prose.
No column is added for it.

### 5.5 `04` §15.1 and §15.2, row by row

| Situation | Hub behaviour after this prompt |
|---|---|
| `Dispatched`, agent epoch matches | query `action_status`, per §5.4 |
| `Dispatched`, agent epoch differs | `OutcomeUnknown { EpochChanged }`, no query |
| `Dispatched`, host `Incompatible` | nothing: there is no authenticated 1.2 session, so no pass runs. The row stays `Dispatched` ("awaiting compatible agent"). If that agent later connects with a different epoch, it becomes `OutcomeUnknown` then |
| `Dispatched`, agent never reconnects | stays `Dispatched` forever. `agent_wait_limit` / `offline_wait` applies to `Approved` work that was never dispatched, not to this |
| `Approved`, not dispatched | prompt 14's freshness check and host wait, then dispatch; **expired → `NotRun`** (§5.6); plan or target changed, incident gone or resolved → `Rejected` as today |
| `Proposed` | unchanged: the expiry sweep |
| early `hello` (agent connects before the action service is ready) | the pass finds nothing, or queues behind the writer; both are safe |
| duplicate `hello` (reconnect, or a second session replacing the first) | a fresh idempotent pass; already-terminal rows are no longer `Dispatched` and are skipped |
| late `hello` | the same pass; nothing has failed the row in the meantime |

Crash windows (`04` §15.2), as the hub sees them:

| # | Scenario | Agent answer | Hub outcome |
|---|---|---|---|
| 1 | `accepted` durable, crash before launch | `outcome_unknown{no_start_record}` | `OutcomeUnknown` (indistinguishable from #2 by design) |
| 2 | launched, crash before `started` durable | `outcome_unknown{no_start_record}` | `OutcomeUnknown` |
| 3 | running, agent crashes | `outcome_unknown{agent_restarted}` (the agent kills a matching orphan first) | `OutcomeUnknown` |
| 4 | finished, crash before `finished` durable | `outcome_unknown{agent_restarted}` | `OutcomeUnknown` |
| 5 | `finished` durable, hub lost the reply | `finished(result)` | result committed |
| 6 | hub received the result, crashed before commit | `finished(result)` | result committed |
| 7 | journal corrupt, replaced, or id outside retention | `outcome_unknown{journal_lost}` / epoch mismatch | `OutcomeUnknown` |
| 8 | agent restarts with active containment | containment records | **not this prompt**: prompt 46 updates containment state |
| - | request never reached the agent | `not_found` with a matching epoch | `NotRun` |

### 5.6 Expiry before dispatch: `Approved → NotRun`

`04 §3.2` places "its authorization or approval expired before dispatch" inside `NotRun`; `04 §3.3` and
`04 §15.1` say the same for an authorized execution that never dispatched. Today that case becomes
`Rejected` with "the approval expired before the action was dispatched" (`freshness.rs:31`, reached from
`writer::dispatch` at `:340-346` and from the executor's pre-dispatch check). That is the wrong vocabulary:
nothing was attempted. This prompt changes that one outcome.

Two trigger points, one outcome:

1. **At dispatch.** `freshness::check` still returns `Stale::Expired`. `writer::dispatch` sends that one
   reason through `never_ran` (7.3) instead of `reject::stop`. `IncidentGone` and `IncidentResolved` keep the
   `Rejected` path unchanged.
2. **Without a dispatch attempt.** `expire_due` (`writer.rs:408-453`) sweeps `status = "proposed"` today.
   Add a second pass over `status = "approved"` with the same `expires_before` filter: each row takes
   `Approved → NotRun`, the audit entry below, and the guarded write on `"approved"`. Without it, an approval
   whose host never returns sits `Approved` for ever, because nothing else looks at the row.

Rules:

- The outcome is `NotRun { reason }` with `EXPIRED_REASON`, reusing today's sentence so no user-visible text
  changes: "the approval expired before the action was dispatched".
- The audit kind is `action.not_run` — the kind this prompt adds — **not** `action.expired`.
  `action.expired` stays the `Proposed` sweep's kind and `action.rejected` stays the incident-gone path's.
- `NotRun` is terminal: the row is never dispatched, retried or re-approved afterwards. A person who still
  wants the work approves a new action.
- Nothing is sent to the agent, and the row never enters `RAN_STATUSES`.
- The DTO already carries the reason (7.6) and the web status set already treats `not_run` as terminal, so no
  new API or UI shape appears for this path.
- Keep the sweep idempotent and race-safe as it is: the filter picks the rows, the transition checks the
  status, the guarded write refuses a row decided meanwhile, and a `Conflict` is logged and skipped.
- The host-availability rows of `04 §15.1` are **out of scope here**: do not change the offline-timeout
  outcome, and do not call it `Unavailable` until the prompt that owns `Unavailable` exists. Only the expiry
  outcome moves.

## 6. Behavioural invariants

1. **Uncertainty is terminal and never resolved by guessing.** `OutcomeUnknown` is not retried, not
   re-dispatched, not converted to `Failed`, and never to `Succeeded`. Only a person or a later verification
   resolves it.
2. **Only positive evidence produces `NotRun`.** Its sources are exactly two: `not_found` from a matching,
   unpruned journal, and an approval that expired before dispatch. A missing answer, a timeout, an error
   frame, an `unsupported` reply or a missing session is none.
3. **A dispatch is never failed because the hub restarted.** The restart path writes nothing about
   `Dispatched` rows.
4. **A dispatch is never failed because the agent restarted.** That is `OutcomeUnknown`.
5. **`running` never becomes an outcome and never causes a second dispatch.** It only keeps the row
   `Dispatched` and schedules another query.
6. **Every committed outcome is derived from one answer under one epoch.** No outcome mixes answers, and an
   epoch mismatch inside a batch invalidates the whole batch.
7. **Nothing is persisted before authentication**, and nothing is persisted for a host with no session.
8. **The pass is idempotent and re-runnable.** Running it twice, or crashing halfway, leaves the store either
   at `Dispatched` or at one terminal outcome per action, never between.
9. **A terminal outcome is never overwritten.** The guarded write is on the status the pass read.
10. **Reconciliation authorises nothing.** It cannot approve, deny, dispatch, retry or delete. It calls no
    constructor reserved for a human.
11. **The agent's journal is evidence; the hub's action row is the record.** The hub never treats the journal
    as its own state, never writes to it, and never reports a journal fact the answer did not contain.
12. **Expiry before dispatch is `NotRun`, never `Rejected`, `Failed` or `Succeeded`.** It is positive evidence
    the run never started, so it must not be grouped with an attempted-and-failed run or with a refusal.
13. **`NotRun` is terminal on both paths.** Neither a matching-epoch `not_found` nor an expired approval is
    later dispatched, retried or overwritten.

## 7. Data model, interfaces and state transitions

### 7.1 `domovoi-core`: `JournalEpoch`

A new type in `crates/domovoi-core/src/action/` (its own small module, re-exported from `action/mod.rs` and
from the crate root beside the other action types):

```rust
/// The identity of an agent's action journal, as the agent reported it on `hello`.
///
/// Opaque. The hub compares epochs and never creates one; only the agent's local `journal
/// init` and `journal reset` produce a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JournalEpoch(Uuid);
```

- `from_uuid` and `as_uuid`, `Display` as the hyphenated lowercase form, `serde` as that string.
- No `new()`, no `Default`, no clock, no I/O: `domovoi-core` stays pure.

### 7.2 `domovoi-core`: the two statuses

In `crates/domovoi-core/src/action/request.rs`:

```rust
Dispatched {
    /// The agent journal epoch the hub saw when it sent this run; absent for a run
    /// sent before the epoch was recorded, or when it could not be read.
    #[serde(default)]
    agent_epoch: Option<JournalEpoch>,
},
...
/// The run may have happened and its result cannot be established.
OutcomeUnknown {
    /// Short, plain, already user-facing; see the reason constants.
    reason: String,
},
/// Positive evidence that the run never reached the agent.
NotRun {
    /// Short, plain, already user-facing.
    reason: String,
},
```

- `NAMES` gains `"outcome_unknown"` and `"not_run"` (the array's length becomes 12), and `name()` returns
  them; insert both after `"failed"` in both places. The array order carries no meaning, only readability.
- `RAN_STATUSES` (`ports/repos/actions.rs:79`, `[&str; 5]`) gains `"outcome_unknown"` and **not**
  `"not_run"` (its length becomes 6), with the doc comment updated: `OutcomeUnknown` may have run, `NotRun`
  provably did not. If a consumer of `last_run_by_skill` needs the opposite, stop and report.
- The doc comment on `ActionStatus` lists both states as terminal.

### 7.3 `domovoi-core`: transitions

In `crates/domovoi-core/src/action/transition.rs`, two methods with the same shape as the existing ones
(check first, then `move_to`):

```rust
/// `Dispatched → OutcomeUnknown`, when the agent cannot establish what became of the run.
pub fn outcome_unknown(&mut self, reason: String, now: Timestamp) -> Result<ActionChange, ActionError>;

/// `Dispatched | Approved → NotRun`: positive evidence the run never started, either because the
/// agent's journal has no record of it or because its approval expired before it was dispatched.
pub fn never_ran(&mut self, reason: String, now: Timestamp) -> Result<ActionChange, ActionError>;
```

`never_ran` accepts two source states because both are positive evidence of never starting, and the outcome
is the same one `04 §3.2` defines. It refuses any other source with `NotAllowed`, including `Proposed` —
expiry of a proposal stays `Expired` (§5.6).

`dispatch` gains the epoch:

```rust
pub fn dispatch(
    &mut self,
    current_plan_hash: PlanHash,
    agent_epoch: Option<JournalEpoch>,
    now: Timestamp,
) -> Result<ActionChange, ActionError>;
```

`complete` and `verified` are unchanged. No new `ActionError` variant is needed: a wrong source status is
`NotAllowed`.

### 7.4 `domovoi-core`: the link and the reconciler

In `crates/domovoi-core/src/ports/agent_link.rs`, re-exported from `ports/mod.rs`:

```rust
/// What an agent's journal says about a run id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionAnswer {
    /// The run finished; `detail` is the agent's failure message when it failed.
    Finished { succeeded: bool, detail: Option<String> },
    /// Still running in the agent instance that started it.
    Running,
    /// Positive evidence the request never reached the journal.
    NotFound,
    /// The journal cannot establish what happened.
    Unknown { reason: UnknownReason },
}

/// Why an outcome is unknown. The first three come from the agent's answer; the hub
/// decides the last two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    NoStartRecord,
    AgentRestarted,
    JournalLost,
    EpochChanged,
    EpochUnknown,
}

/// One answer, keyed by the id that was asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAnswerEntry {
    pub execution: ActionId,
    pub answer: ActionAnswer,
}

/// The agent's answers and the epoch it answered under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAnswers {
    pub epoch: JournalEpoch,
    pub answers: Vec<ActionAnswerEntry>,
}
```

Two methods on `trait AgentLink`, **required, not defaulted**: a silently missing implementation would leave
dispatched work unreconciled forever, and all three implementors are in-tree.

```rust
/// The journal epoch the host's live session reported, or `None` with no live session.
fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch>;

/// What the host's agent knows about `ids` under `expected_epoch`. At most 64 ids.
async fn action_status(
    &self,
    host: HostId,
    ids: Vec<ActionId>,
    expected_epoch: JournalEpoch,
    deadline: Timestamp,
) -> Result<ActionAnswers, LinkError>;
```

A new port in `crates/domovoi-core/src/ports/` for the trigger:

```rust
/// Told when a host's agent authenticated, so the host's dispatched work can be reconciled
/// against that agent's journal. Implemented by the action service.
#[async_trait]
pub trait HostReconciler: Send + Sync {
    /// Reconciles the host's dispatched executions. Idempotent; safe to call on every
    /// authenticated `hello`.
    async fn reconcile_host(&self, host: HostId);
}
```

`ActionAnswers` is the hub's own shape, not the wire shape: it must not carry `domovoi-proto` types into
`domovoi-core`, which has no such dependency. The mapping from `ActionStatusAnswers` to `ActionAnswers` lives
in `domovoi-hub`.

### 7.5 `domovoi-engine`: the pass and its write

- New `crates/domovoi-engine/src/actions/reconcile.rs` holding the pass, the answer → outcome mapping as a
  pure function, and the reason constants. Keep it under the 500-line limit; if the mapping grows, split the
  mapping into `reconcile/answer.rs`.
- `ActionSettings` gains `reconcile_interval: SignedDuration` (default 30 s) and `reconcile_query_timeout:
  SignedDuration` (default 10 s), both used through the `Clock` port. Tests set them and advance `FakeClock`.
- `WriteCommand::Reconcile { id, outcome: ReconcileOutcome, wire_reason: &'static str, reply }` with
  `ReconcileOutcome ∈ { Finished { succeeded, detail }, Unknown(UnknownReason), NeverRan }`, handled by a new
  `fn reconcile` in `writer.rs` beside `complete`. It loads the action, calls the matching transition,
  builds the audit entry, and commits with `ActionGuard::Status("dispatched".to_owned())` and
  `extra = Some(("outcome_reason", wire_reason))`. The pass awaits the reply, logs a refused commit at debug
  and never retries it.
- The reason constants, the answer → outcome mapping, and `never_ran`'s expiry use live in `reconcile.rs`
  beside the journal reasons, so there is one place that says why something did not run.
- `writer::dispatch` maps `Stale::Expired` to `not_run::expire(...)` (`never_ran` + `action.not_run` +
  `EXPIRED_REASON`, guarded on `"approved"`) instead of `reject::stop`; `IncidentGone` and `IncidentResolved`
  keep `reject::stop` unchanged (§5.6).
- `writer::expire_due` gains the second pass over `status = "approved"` described in §5.6: same
  `expires_before` deadline, `never_ran` with `EXPIRED_REASON`, `audit::not_run`, guard `"approved"`. The
  `proposed` pass keeps `action.expired` and `ExpireDue`'s existing reply count; return the total so the
  caller's count still means "rows resolved".
- `impl HostReconciler for ActionHandle`, delegating to `reconcile::host(shared, host)`.
- `writer::run` no longer calls `fail_abandoned`; `recover::fail_abandoned` and `RESTART_REASON` are deleted.
  `recover::scan`, `scanned`, `approved` and `dispatch_history` stay, because the catch-up and the loop guard
  still use them.
- The `recovered` watch (`actions/mod.rs:143`, `writer.rs:131`, `executor.rs:57-60`) existed only to hold the
  executor's catch-up until `fail_abandoned` had run. With nothing before `send_replace` left, delete the
  field, the signal and the executor's wait, update the startup-order comment at `mod.rs:289-293`, and keep
  the loop-guard reload (`mod.rs:273`) ahead of both halves exactly as it is. If you find any other startup
  work that must precede catch-up, keep the watch and say why in the report instead of deleting it.

### 7.6 `domovoi-hub`

- `agent_server/reconcile` trigger: in `session.rs::run_session`, after registration and before the frame
  loop, spawn the pass through the new optional dep.
- `AgentServerDeps` gains `reconciler: Option<Arc<dyn HostReconciler>>`. `runtime/mod.rs` passes
  `Some(action_handle.clone())`; `admin/mod.rs` passes `None` and says why in a comment: that harness holds
  no action state. `None` reconciles nothing and logs at debug once per session.
- `WsAgentLink::journal_epoch` reads the live session's `hello.journal_epoch` from the registry; therefore
  `Live` keeps the epoch from `Accepted.hello` (it already keeps `version`, `facts`, `skills`, `checks`,
  `docker`).
- `Registry` gains `epoch(host) -> Option<JournalEpoch>`, beside `facts`, `checks` and `docker`.
- `WsAgentLink::action_status` builds `ActionStatusQuery` from `ids` via `ActionId::as_uuid()`, goes through
  `WsAgentLink::request` (`link.rs:94`) for the deadline and reply handling, and maps the reply.
  `Message::Error`/`Message::Unsupported` become `LinkError::Rejected`. An id in the reply that was not asked
  about is dropped with a warning.
- `PolicyLink` forwards both methods unchanged; it wraps the link and must not filter by policy, because
  reading an outcome is not a capability.
- `api/dto.rs`: include `OutcomeUnknown { reason }` and `NotRun { reason }` in the reason match, so the
  person sees why. `ActionDto` shape does not change.

### 7.7 `domovoi-testkit`

`FakeAgentLink` gains `journal_epoch` and `action_status` with scripted answers per host:
`set_journal_epoch(host, Option<JournalEpoch>)`, `answer_action(host, ActionId, ActionAnswer)`,
`fail_action_status(host, LinkError)`, `action_status_queries() -> Vec<(HostId, Vec<ActionId>, JournalEpoch)>`
so a test can assert batching and the expected epoch. Answers are keyed per id and survive repeated queries,
which is what makes an idempotency test possible.

## 8. Transaction rules, durability and interruption

- One answer, one `UnitOfWork` commit, by the action writer, containing exactly: the guarded action write
  (`expected = "dispatched"`), the audit entry, and the journal entry. This is `commit_action`'s existing
  shape (`actions/mod.rs:155-213`); do not add a second write path and do not commit from the pass directly.
- The guard is the whole concurrency story. Two passes, a pass racing a hand decision, or a pass racing
  another epoch all resolve the same way: the second commit finds the status is no longer `dispatched`, the
  store refuses it, and the pass drops that result. Never retry a refused commit and never reload-and-force.
- The pass keeps no durable "in progress" marker and no in-memory idempotency key. Its inputs are the store's
  `Dispatched` rows and the agent's answers, both re-read on every pass, so repeating it is always safe.
- Interruption: if the hub dies after sending `action_status` and before committing, the rows stay
  `Dispatched` and the next authenticated `hello` asks again. If it dies after the commit, the row is
  terminal and later passes skip it. There is no third state.
- If the store refuses the listing at the start of a pass, reconcile nothing and log it. A short list is not
  a complete list.
- Time comes from `Clock` only. Tests never sleep; they advance the fake clock.

## 9. Files and components expected to change

New:

- `crates/domovoi-core/src/action/epoch.rs` — `JournalEpoch`.
- `crates/domovoi-engine/src/actions/reconcile.rs` — the pass, the mapping, the reason constants.
- `crates/domovoi-core/src/ports/reconcile.rs` — `HostReconciler`, re-exported from `ports/mod.rs`.
- `crates/domovoi-engine/tests/services/reconciliation.rs` — the hub-side crash-window suite, registered in
  `tests/services/main.rs`.
- `crates/domovoi-engine/tests/services/expiry.rs` — the §5.6 suite, registered in `tests/services/main.rs`.
- `crates/domovoi-hub/tests/hub/reconciliation.rs` — the trigger, authentication and no-reconnect cases,
  registered in `tests/hub/main.rs`.

Changed:

- `crates/domovoi-core/src/action/{mod,request,transition}.rs`, `src/lib.rs`, `src/ports/agent_link.rs`,
  `src/ports/repos/actions.rs`.
- `crates/domovoi-engine/src/actions/{mod,writer,recover}.rs` (settings, command, transition call, expiry
  mapping and sweep, delete the restart failure and the `recovered` watch), `src/actions/executor.rs` (drop
  the `recovered` wait, send an expired action through the `NotRun` path), `src/actions/freshness.rs` (no
  signature change; its `Expired` reason now selects `NotRun` at both call sites).
- `crates/domovoi-hub/src/agent_server/{mod,session,link,registry,skill_policy}.rs`,
  `src/runtime/mod.rs`, `src/admin/mod.rs`, `src/api/dto.rs`.
- `crates/domovoi-testkit/src/link.rs`.
- `crates/domovoi-store/tests/sqlite/actions.rs` (old-row compatibility; no SQL change).
- `web/src/design/status.ts`, `web/src/design/actionVocab.ts`,
  `web/src/features/incidents/ActionSections.tsx`, `web/src/features/approvals/ActionCard.tsx`,
  `web/src/features/approvals/useActionOutcomes.ts`, plus their tests.
- `docs/book/src/concepts/actions.md`, `ARCHITECTURE.md`.
- `crates/domovoi-engine/tests/services/recovery.rs` (`startup_recovery_runs_before_catch_up`), and any test
  that asserted `Dispatched → Failed(RESTART_REASON)`.

Not touched: `domovoi-proto`, `domovoi-agent`, `domovoi-config`, `domovoi-runner`, `migrations/`, any
`Cargo.toml`.

## 10. Migration and protocol treatment

**No SQL migration.** This is a verified decision, not an omission, and it contradicts the expectation in
`02` §4 and the hint in `investigations/D-protocol.md:175`:

- `actions.status` is `TEXT NOT NULL` with **no** CHECK constraint (`0001_init.sql:116`).
- The value stored is the serialised status as JSON, not its name (`repos/actions.rs:123`), and it is read
  back with `from_json::<ActionStatus>` (`:75`). Guarded updates and listings compare
  `json_extract(status, '$.type')` (`:186`, `:243`), so a new variant and a new field inside `Dispatched`
  need no column, no index and no constraint change.
- The epoch therefore lives inside the status JSON. A separate epoch column, or a separate
  `action_reconciliation` table, would duplicate state the status already owns and would need its own guard.
  Reject both; if a reviewer asks for one, say why.
- `domovoi-core`/`05` §2's "new enums validated in Rust, not SQL CHECK" is already satisfied: there is no
  CHECK to widen.

The compatibility work that replaces a migration:

- `#[serde(default)]` on `agent_epoch`, so a pre-upgrade `{"type":"dispatched"}` row loads as
  `Dispatched { agent_epoch: None }` and reconciles on the next `hello` to `OutcomeUnknown { EpochUnknown }`.
  That is the `05` §4 rule: executions dispatched before the upgrade become `OutcomeUnknown` when the
  upgraded agent connects with a new journal epoch.
- One N-1 test from stored JSON, in `crates/domovoi-store/tests/sqlite/actions.rs`: an action row whose
  `status` is the old literal `{"type":"dispatched"}` loads, keeps its other fields, and matches the
  `dispatched` filter.
- Run `cargo xtask openapi` and confirm it produces no diff: `ActionDto` keeps `status: String` and
  `reason: Option<String>`. If it does produce a diff, explain it in the completion report before committing
  the regenerated files.

Stop and report if, on the launch branch: `actions.status` now has a CHECK constraint; an `executions` table
or an execution-epoch column already exists; or `StoredAction` has gained a dispatch field that duplicates
`agent_epoch`.

Protocol: no change. 21 consumes 1.2 exactly as 20 froze it, adds no message, no field and no version. Golden
frames are not regenerated. `cargo nextest run -p domovoi-proto` is run only because a public type in another
crate changed; `domovoi-proto` itself is not edited.

## 11. Security and failure handling

- **Nothing before authentication.** The pass starts only from `Accepted`, so an unauthenticated client can
  neither learn nor change any action state.
- **The pass cannot authorise.** It never constructs `HumanApproval`, never calls `dispatch`, never writes
  `Approved`, and never issues a permit. Reconciliation records what an authenticated agent said about work
  already authorised.
- **No new surface.** No endpoint, chat command, LLM tool or message may set an outcome or force a
  reconciliation. The closed LLM catalogue and INV-1 are untouched.
- **A hostile or buggy agent can only report its own runs.** Every id in the query is one the hub dispatched
  to that host and still holds as `Dispatched`; an answer for anything else is dropped. A forged `finished`
  can only claim an outcome for work the hub already sent to that agent.
- **Uncertainty is user-visible, not silent.** `OutcomeUnknown` and `NotRun` must reach the incident page and
  the approvals card, so a person can see that a write may have happened. Never render them as "Running",
  "Succeeded" or "Failed".
- **Failure handling:**

| Fault | Behaviour |
|---|---|
| no live session when the pass asks for the epoch | nothing happens; the rows stay `Dispatched` |
| listing the store fails | nothing is reconciled; the error is logged |
| `action_status` times out or the link drops | that batch is skipped this round; rows stay `Dispatched`; the next `hello` asks again |
| the agent replies `unsupported` or `error` | same as a timeout: no outcome is invented |
| the answer's epoch differs from the query's | the whole batch is `OutcomeUnknown { EpochChanged }` |
| two passes for one host at once | every commit is guarded; the loser's commits are refused and dropped |
| the epoch changes during a pass | the pass stops before the next round |
| the writer queue is closed (shutdown) | the outcome is not committed; the row stays `Dispatched`, which is correct |
| an approved action expires while the hub is down | the next `expire_due` resolves it to `NotRun` (§5.6); no agent is contacted |
| an approved action expires while its run waits for the host | the pre-dispatch freshness check resolves it to `NotRun`, not `Rejected` |

## 12. Named tests and acceptance criteria

Name tests after the behaviour. Keep the six names from `02` §4 exactly; they assert the **hub** outcome for
the crash window whose **agent** side prompt 20 tests under the same names. Say so in each test's doc
comment.

`crates/domovoi-engine/tests/services/reconciliation.rs` (new):

- `crash_after_accepted_is_outcome_unknown` — `outcome_unknown{no_start_record}` → `OutcomeUnknown`, reason
  token `no_start_record`, one audit row, no success text anywhere.
- `crash_after_started_is_outcome_unknown` — `outcome_unknown{agent_restarted}` after a recorded start →
  `OutcomeUnknown`.
- `finished_durable_reply_lost_reconciles` — `finished{result}` for a row the executor gave up on →
  `Succeeded`/`Failed` with the agent's message.
- `hub_crash_before_commit_reconciles_from_journal` — the pass gets `finished`, the commit fails, a second
  pass asks again and commits exactly once.
- `epoch_change_is_outcome_unknown` — recorded epoch differs from `hello.journal_epoch` → `OutcomeUnknown`
  with `epoch_changed`, and no `action_status` query is sent.
- `not_found_with_matching_epoch_is_not_run` — `not_found` under a matching epoch → `NotRun`, not
  `OutcomeUnknown` and not `Failed`.

`04` §15.2 coverage beyond the six:

- `running_execution_stays_dispatched` — `running` writes nothing, the row stays `Dispatched`, and the pass
  asks again after `reconcile_interval` on the fake clock.
- `running_is_never_redispatched` — `runs()` on the fake link stays at one dispatch across repeating passes.
- `journal_lost_is_outcome_unknown` — `outcome_unknown{journal_lost}` → `OutcomeUnknown`, token
  `journal_lost`.
- `pre_upgrade_dispatched_row_reconciles_as_outcome_unknown` — a row with no recorded epoch →
  `OutcomeUnknown` with `epoch_unknown`, no query.
- `reconciliation_is_idempotent` — two passes over one finished row produce one commit and one audit row.
- `interrupted_reconciliation_is_repeated_on_the_next_hello` — the pass is dropped mid-round; the row is
  still `Dispatched`; the next hello reconciles it.
- `answer_for_an_unasked_id_is_ignored` — an extra answer changes nothing.
- `stale_answers_from_another_epoch_are_outcome_unknown` — the answer echoes a different epoch.
- `reconcile_query_batches_at_sixty_four_ids` — 65 matching ids produce two queries of 64 and 1, each with
  the recorded epoch.
- `outcome_unknown_is_not_the_loop_guard` — the loop guard (`dispatch_history`, rolling hour, host + skill)
  and `RAN_STATUSES` are unchanged in what they enforce; a test asserts that adding `outcome_unknown` to
  `RAN_STATUSES` does not by itself block a proposal, so nobody mistakes the two rules for each other. The
  full unattended-reuse block is prompt 45's and is not asserted here.
- `startup_no_longer_fails_dispatched_actions` — replaces the assertion in `startup_recovery_runs_before_catch_up`
  (`tests/services/recovery.rs`): after a restart the row is still `Dispatched`, with no audit row and no
  `Failed`. Keep the rest of that test's coverage.
- `orphan_process_killed_on_agent_restart` — prompt 20 owns it; the hub-side consequence is
  `crash_after_started_is_outcome_unknown`. Do not duplicate the agent test.
- `active_containment_reconciled_after_agent_restart` — prompt 46. Not implemented, not tested, not claimed
  here.

`crates/domovoi-engine/tests/services/expiry.rs` (new), for §5.6:

- **`approved_expiry_before_dispatch_is_not_run`** — approve an action, advance the fake clock past
  `expires_at`, run the executor: the row is `NotRun` with `EXPIRED_REASON`, exactly one `action.not_run`
  entry, no `action.rejected` and no `action.failed`, and the recording link saw no call.
- `expiry_sweep_resolves_an_approved_action_without_a_host` — an approved action whose host never appears is
  swept to `NotRun` by `expire_due` when its window closes, with no agent call and the same audit kind.
- `proposed_expiry_stays_expired` — a `Proposed` action still becomes `Expired` with `action.expired`; the
  sweep's first pass and its count are unchanged.
- `incident_resolved_before_dispatch_stays_rejected` — the incident-gone and incident-resolved paths still
  yield `Rejected`, proving the change is limited to expiry.

`crates/domovoi-hub/tests/hub/reconciliation.rs` (new):

- `authenticated_hello_starts_reconciliation` — one recording `HostReconciler` call per accepted session.
- `unauthenticated_hello_reconciles_nothing` — a bad signature or a protocol mismatch triggers no call and
  writes nothing.
- `incompatible_agent_keeps_dispatched_action` — a 1.1 agent is rejected and the row stays `Dispatched`.
- `duplicate_hello_reconciles_each_time` — two sessions in a row each trigger a pass.
- `agent_that_never_reconnects_keeps_dispatched_action` — advancing past `offline_wait` changes nothing.

`crates/domovoi-store/tests/sqlite/actions.rs`:

- `pre_upgrade_dispatched_status_loads_without_an_epoch`.

`web/`:

- `outcome_unknown_is_not_in_flight` — the incident and approvals views do not group it with `approved` or
  `dispatched`.
- `outcome_unknown_and_not_run_have_vocabulary` — both statuses resolve through `ACTION_STATUSES` with a
  label, tone, hint and icon, and the axe run over each page test still passes.

Acceptance criteria:

1. A `Dispatched` action survives a hub restart untouched, and adding it to `Fail`-on-restart is impossible
   because that code is gone.
2. Each of the four answer kinds maps to exactly one outcome from §5.4, with the reason token recorded.
3. No code path produces `Succeeded` from anything but `finished{result}` with a successful outcome, or
   `Failed` from anything but `finished{result}` with a failed outcome.
4. `not_found` produces `NotRun` only with a matching epoch; every other absence produces `OutcomeUnknown`.
5. An epoch change produces `OutcomeUnknown` without a query.
6. Interrupting the pass, repeating it, or racing two passes never yields two terminal outcomes or a lost
   result.
7. Nothing is reconciled for an unauthenticated client, and nothing is persisted for a host with no session.
8. No new wire type, no protocol version change, no migration file, no new crate, no new dependency.
9. The old stored status shape loads and reconciles as `OutcomeUnknown`, proven by a test.
10. An approved action whose window closes before dispatch is `NotRun` on both trigger points, with one
    `action.not_run` entry, no agent call, and no `action.rejected` or `action.failed`; `Proposed` expiry and
    the incident-gone rejection are unchanged.
11. The unattended-reuse rule is recorded as prompt 45's, and this prompt does not claim the loop guard
    enforces it.

## 13. Documentation and architecture updates

- `docs/book/src/concepts/actions.md`: add `OutcomeUnknown` and `NotRun` to the status list, with one plain
  sentence each, and replace the "a restart fails what was running" paragraph with what now happens:
  the hub asks the agent's journal, and uncertainty is shown as uncertainty. User-facing text is short and
  specific: "Result unknown — it was sent to the host and may have run", "Never ran — the agent has no
  record of it".
- `ARCHITECTURE.md:160-172`: extend the action state diagram with `Dispatched --> OutcomeUnknown: journal
  lost / epoch changed / agent restarted mid-run`, `Dispatched --> NotRun: not_found with matching epoch` and
  `Approved --> NotRun: approval expired before dispatch`, and keep `Dispatched --> Failed: the agent
  reported a failure`. Update the status list if the file enumerates it. Do not restate `04` §15.2 there.
- `web/DESIGN.md` needs no change: the two statuses use existing tones and the existing `actionVocab` table.
- No `docs/rework/` document is edited by the implementation. If this brief is wrong, the completion report
  says so.
- `cargo xtask docs-links` if a book page or an anchor changes.

## 14. Implementation order and completion requirements

1. `domovoi-core`: `JournalEpoch`, the two statuses with `serde(default)` on the epoch, the two transitions
   (with `never_ran` accepting `Approved`), the `dispatch` signature, `RAN_STATUSES`, the new port types.
   Compile the workspace and fix call sites (`writer::dispatch` passes `None` for now).
2. `domovoi-store`: the old-row test. Nothing else; no migration.
3. `domovoi-testkit`: scripted epochs and answers, the recording reconciler.
4. `domovoi-engine`: the expiry path first — `never_ran` in `writer::dispatch`, the `approved` pass in
   `expire_due`, `EXPIRED_REASON` and `audit::not_run` — then the pass, the mapping, the write command and the
   remaining audit kind; then delete `fail_abandoned`, `RESTART_REASON` and the `recovered` watch, and rewrite
   the recovery test. Do the expiry path first because it is the smallest change that touches the same
   transition and audit code the reconciliation mapping also uses.
5. `domovoi-engine` tests: the §5.6 expiry suite, then the crash-window suite.
6. `domovoi-hub`: `WsAgentLink`, `PolicyLink`, the trigger in `run_session`, `AgentServerDeps`, the DTO
   reason, the hub tests.
7. `web/`: the two statuses and the status sets, with their tests.
8. Docs, `ARCHITECTURE.md`, and the scoped checks.

Do not reorder the core types after the engine: the mapping cannot be written before the statuses exist.

Run, and put the real output in the completion report:

- `cargo fmt --all`
- `cargo clippy -p domovoi-core --all-targets -- -D warnings`
- `cargo clippy -p domovoi-engine --all-targets -- -D warnings`
- `cargo clippy -p domovoi-hub --all-targets -- -D warnings`
- `cargo clippy -p domovoi-store --all-targets -- -D warnings`
- `cargo clippy -p domovoi-testkit --all-targets -- -D warnings`
- `cargo nextest run -p domovoi-core`, `-p domovoi-engine`, `-p domovoi-hub`, `-p domovoi-store`
- `cargo nextest run -E 'rdeps(domovoi-core)'` (public items changed)
- `cargo xtask arch-check` (a new module and a new port)
- `cd web && npm run lint && npm run typecheck && npm test`
- `cargo xtask openapi`, confirming no diff

Completion requirements:

- Every invariant in §6 holds in code, not only in a comment, and each has a test or an explicit "not
  applicable" line in the report.
- `fail_abandoned`, `RESTART_REASON` and the `recovered` watch are gone, with no replacement that writes to a
  `Dispatched` row on restart.
- No path sets `Succeeded`, `Failed`, `NotRun` or `OutcomeUnknown` from anything but the answer table in
  §5.4.
- No migration file, no protocol change, no new dependency, no new crate, no `#[allow(...)]`, no line over
  the size limits.
- Anything the brief left open, or that the merged code contradicted, is listed in the completion report as
  a deviation or as unfinished — never smoothed over.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
