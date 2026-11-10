# 27 · Bug: a reused triage report proposes its actions again

## Background (verified on `main`)

- `crates/domovoi-engine/src/triage/run.rs` `outcome()`: on a repeat-cache hit
  (`:86-88`) it returns `TriageOutcome::Cached { report, from }` with an **empty** token
  map. Only the fresh path calls `proposals::create` (`:95`), which stores the
  report's proposals as `Proposed` actions and mints their chat tokens.
- Cards build Approve/Deny buttons from stored actions
  (`crates/domovoi-engine/src/cards.rs`, `action_buttons`), and the web incident page
  shows stored actions, not `report.proposals`.
- So a repeat incident gets the old headline, verdict ("Action needed") and next step,
  with nothing to approve. Meanwhile `GET /api/v1/incidents/{id}` still returns the
  cached `proposals` (`crates/domovoi-hub/src/api/dto.rs:196`), naming skills that have
  no action and were checked against the old skill catalogue.
- Workaround today: "Redo triage" skips the cache.
- `proposals::create(shared, incident, skills, &mut report)`
  (`triage/proposals.rs:143`) already re-validates against the host's **current**
  skills, skips duplicates of waiting actions and notes dropped ones in `details`.

## Steps

1. In `run.rs`, on a cache hit: `let skills = deps.link.skills(incident.host());`, make
   the report mutable, call `proposals::create(shared, incident, &skills, &mut report)`
   and return its tokens with the `Cached` outcome instead of `BTreeMap::new()`.
2. Keep the cache untouched (do not `put` the trimmed report back).
3. No other change: `TriageOutcome::Cached` and the timeline entry stay as they are.

## Tests (`crates/domovoi-engine/tests/services/limits.rs`, next to `repeat_incident_uses_cache_without_model_call`)

- `a_cached_report_stores_its_proposals_as_actions`: a cached report with one valid
  proposal yields one `Proposed` action on the new incident and a chat token in the
  `TriageCompleted` event.
- `a_cached_proposal_for_a_removed_skill_is_dropped`: the skill is gone from the fake
  link; no action is stored and the report's `details` says why.
- `a_cached_report_does_not_duplicate_a_waiting_action`.

## Done when

- A repeat incident shows the same approve buttons a fresh triage would, validated
  against the skills the host has now.
