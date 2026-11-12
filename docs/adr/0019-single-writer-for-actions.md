# 0019. Action decisions have a single writer

- Status: Accepted
- Date: 2026-11-12

## Context

An action can be decided by more than one party at once: the person clicks Approve or Deny in the web UI, the Discord channel approves the same action with its single-use token, and the expiry sweep closes the approval window of everything still waiting. With the current code these paths interleave: the sweep re-reads the whole action table and writes `expired` into any `proposed` action, so an approval that finished a heartbeat earlier can be overwritten by a sweep that read it before that. The audit log then records both decisions and the store holds whichever was written last. ADR 0011 solved the same shape for incidents with one task owning every transition; ADR 0014 made approvals re-hydrate from the store so both readers see current state, but nothing refuses a stale write.

## Decision

- The action service gets one writer task (`domovoi_engine::actions::writer`), like the incident manager of ADR 0011: approve, deny and the expiry sweep arrive as commands on a bounded `mpsc` channel and are applied one at a time. Commands carry their data; the writer answers each caller over a `oneshot` reply.
- The writer applies decisions through a new store port, `ActionRepo::update_from(action, expected)`: the write carries the status the decision was read from, and the store refuses with `conflict` when the stored status no longer matches. The guard is in the query itself (`json_extract(status, '$.type')` stays unchanged), so it holds for every writer, not only disciplined ones. The store does not choose a winner: whoever wrote first wins, and later writers of the same decision are refused with `this action changed meanwhile; reload`, a 409 on the API.
- Expiry asks the store for exactly what is due: `ActionFilter` gains `expires_before`, and the sweep lists with `status = proposed` and `expires_at` past the deadline instead of reading the whole table and filtering in memory.
- The executor keeps its existing `ActionRepo::update`: those transitions are written only by the executor's run loop and do not race another maintainer. Removing plain `update` from the port waits until the executor's recovery moves under the writer too.

## Consequences

- Two approvals, or an approval and the sweep, settle once; the loser gets a refusal rather than a silent overwrite, and the audit log matches what the store holds.
- The expiry sweep is one store query with a page cap instead of a full read of the action table.
- One task serializes action decisions, as for incidents: ample at homelab volumes, and the store guard makes it safe to fan out again later.
- The refusal must be handled by every maintainer, not just the web UI: the chat gateway maps it to "already decided".
- Gate tests: `approve_and_deny_at_once_leave_exactly_one_outcome`, `expiry_never_overwrites_an_approval`, `expire_due_reads_only_due_proposals` and `update_from_refuses_a_stale_status` pin this; the approval gates of ADR 0007 keep passing unchanged.

## Alternatives considered

- Compare-and-set in the store with no writer task: the store refuses the stale write, but every maintainer (audit entry, telemetry, `action.changed` event) would still announce the decision it lost, and the sweep would still read the whole table each pass. Lost.
- One writer task with no store guard: races settle in one place but an uncoordinated writer can still overwrite a decision; the guard costs nothing and closes that hole. Lost.
- Locking per action: locks across `await` points are the trouble ADR 0011 names. Lost.
- Deciding expiry inside one store transaction: the decision also needs the audit log and the event bus, which the store does not own. Lost.
