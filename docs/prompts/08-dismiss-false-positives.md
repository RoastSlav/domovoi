# 08 · Feature: dismiss an incident as a false positive

Needs prompt 07 (ack rules API) first.

## Goal

On an active incident, **Dismiss** closes it now with a reason, marks it `EXPECTED`, and
optionally creates a known-condition rule so the same thing never alerts again (for a
chosen time, or always).

## What is already known (verified)

- The incident state machine is the pure function `apply()` in
  `crates/domovoi-core/src/incident/transition.rs`. Events: `Firing`, `Resolved`, `Ack`,
  `Mute`, `Unmute`, `Tick`. A private `resolve(incident, now, changes)` helper sets the
  status to `Resolved`. Illegal pairs return `TransitionError::Illegal`.
- Human commands go through the single-writer incident manager:
  `Command` enum in `crates/domovoi-engine/src/manager/command.rs`, handle methods in
  `manager/handle.rs` (see `ack()`), decision logic in `manager/decide.rs`.
- **`decide.rs` is 492 lines.** It may grow by at most 6 lines. Put the new logic in a
  new module `crates/domovoi-engine/src/manager/dismiss.rs`, modelled on the free
  functions in `manager/annotate.rs` (which build a `Decision` from `&State`).
- `manager/actor.rs`, `state_for()` matches every command; human commands
  (`Ack | Mute | Unmute | RecordFollowUp`) need nothing extra loaded.
- A resolved incident whose check keeps firing reopens within the reopen window. With a
  matching ack rule it reopens quietly (no alert, no triage) and keeps its verdict.
- The API for incidents is `crates/domovoi-hub/src/api/incidents.rs`; controls are in
  `web/src/features/incidents/IncidentControls.tsx`. Prompt 07 added
  `POST /api/v1/ack-rules` and `ApiDeps.acks`.

## Read these files

- `crates/domovoi-core/src/incident/transition.rs`
- `crates/domovoi-engine/src/manager/{command,handle,annotate,decide,actor}.rs`
- `crates/domovoi-hub/src/api/incidents.rs`, `crates/domovoi-hub/src/api/rules.rs`
- `web/src/features/incidents/IncidentControls.tsx`, `web/src/components/ConfirmDialog.tsx`

## Steps

1. **Core.** Add `IncidentEvent::Dismiss { by: UserId }`. In `apply()`, the pair
   `(Pending | Open | Acked | Muted, Dismiss { .. })` calls `resolve(...)`. Add a unit
   test next to the existing transition tests (or in
   `crates/domovoi-core/tests/incident_transitions.rs`): dismissing an open incident
   resolves it; dismissing a resolved one is illegal.
2. **Engine command.** Add
   `Command::Dismiss { incident: IncidentId, by: UserId, reason: String }` and
   `ManagerHandle::dismiss(incident, by, reason)`. In `actor.rs` add `Command::Dismiss`
   to the arm that loads nothing.
3. **Engine decision** in new `manager/dismiss.rs`:
   `pub(crate) fn dismiss(state: &State, id: IncidentId, by: UserId, reason: &str, policy: &EnginePolicy, now: Timestamp) -> Result<Decision, CommandError>`:
   find `id` in `state.active` (else `UnknownIncident`), `apply(.., IncidentEvent::Dismiss { by }, ..)`
   (map a transition error to `CommandError::NotAllowed`), `set_verdict(Some(Verdict::Expected))`,
   then push `Write::UpdateIncident`, a `Write::Timeline` entry with kind
   `"human.dismissed"` and summary `format!("dismissed by user:{by}: {reason}")`, and the
   event `DomainEvent::new(EventKind::IncidentResolved, &incident, true)` so chat cards
   update. In `decide()` add one arm:
   `Command::Dismiss { incident, by, reason } => return super::dismiss::dismiss(state, *incident, *by, reason, policy, now),`
   and `mod dismiss;` in `manager/mod.rs`.
4. **API.** `POST /api/v1/incidents/{id}/dismiss` with body
   `DismissRequest { reason: String /* 1..=500 chars */, ignore_for_minutes: Option<u32> /* None = do not create a rule; 0 = forever */ }`.
   Call `manager.dismiss`. When `ignore_for_minutes` is set, also store an `AckRule`
   with `host: Some(incident.host())`, `check: incident.check().clone()`,
   `target_pattern` = the exact target (or `None`), no predicates, note = reason,
   `expires_at` = `None` for 0 else now + minutes, and audit it like `rules.rs` does
   (reuse its helper; make it `pub(crate)` if needed). Return 204. Register the route,
   add it to the `writes` array in `crates/domovoi-hub/tests/gates.rs`, regenerate
   OpenAPI.
5. **Web.** Hook `useDismiss(id)` in `web/src/api/hooks/incidents.ts`. In
   `IncidentControls.tsx` add a **Dismiss** button that opens a dialog with: reason
   (required textarea, placeholder "Why is this not a real problem?"), and
   "Don't alert on this again" select: No / 1 day (1440) / 1 week (10080) /
   30 days (43200) / Always (0). Explain under the select: "If the check keeps failing,
   the incident reopens quietly: no alert and no triage."
6. **Web grouping (if prompt 06 is done).** In `IncidentGroups.tsx`, an `open` incident
   whose verdict is `EXPECTED` belongs to "Being handled", not "Needs attention".

## Tests

- Core: the transition test from step 1.
- Engine: in `crates/domovoi-engine/tests/engine/` add a test that dismissing an open
  incident stores status resolved, verdict `EXPECTED`, a `human.dismissed` timeline
  entry, and publishes `IncidentResolved`.
- Hub: an API test that `POST /dismiss` with `ignore_for_minutes: 0` resolves the
  incident and creates one ack rule with `expires_at: null`.
- Web: the dialog sends `{ reason, ignore_for_minutes }`.

## Done when

- Dismiss resolves the incident with verdict Expected and an optional known-condition
  rule; the rule shows on the Known conditions page.
- `decide.rs` is still at or under 500 lines; all tests pass.
