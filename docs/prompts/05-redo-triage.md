# 05 · Feature: redo AI triage on demand (fresh run, no cache)

## Goal

On an incident that already has a report, the user can press **Run triage again**. The
hub runs the model again from scratch, with the skills, notes and evidence it has *now*
(for example a skill that was just added), instead of reusing a cached report.

## What is already known (verified)

- `POST /api/v1/incidents/{id}/triage` exists (`crates/domovoi-hub/src/api/incidents.rs`,
  `retry_triage`) and calls `TriageHandle::triage(&incident)`.
- `crates/domovoi-engine/src/triage/run.rs`, `outcome()` checks the caps, then
  `cached()` (a report for the same fingerprint and evidence from another resolved
  incident), and only then calls the model. A retry can therefore return `Cached`.
- The skills offered to the model are read fresh on every run
  (`deps.link.skills(incident.host())`), so a new skill is used as soon as the agent
  reconnects with it. Notes are searched fresh too.
- `crates/domovoi-engine/src/triage/proposals.rs`, `create()` stores every valid proposal
  as a new `Proposed` action, so a second run can duplicate proposals still waiting.
- The UI button is `web/src/features/incidents/RetryTriage.tsx`; the hook is
  `useRetryTriage` in `web/src/api/hooks/incidents.ts`.

## Read these files

- `crates/domovoi-engine/src/triage/mod.rs`, `run.rs`, `proposals.rs`
- `crates/domovoi-hub/src/api/incidents.rs`
- `web/src/features/incidents/RetryTriage.tsx`, `web/src/api/hooks/incidents.ts`

## Steps

1. `run.rs`: give `triage()` and `outcome()` a `fresh: bool` parameter. When `fresh` is
   true, skip the `cached()` lookup (caps still apply). The queue worker in `queue.rs`
   passes `false`.
2. `mod.rs`: keep `TriageHandle::triage(&self, incident)` (calls with `false`) and add
   ```rust
   /// Triages `incident` again from scratch: no repeat cache, same caps.
   pub async fn retriage(&self, incident: &Incident) -> TriageOutcome
   ```
3. `proposals.rs`, in `create()`: before storing a proposal, skip it when the incident
   already has an action with status `proposed` for the same skill and the same
   arguments. Load those once with
   `deps.actions.list(&ActionFilter { incident: Some(incident.id()), status: Some("proposed".to_owned()), ..ActionFilter::default() }, &Page::first(50))`
   and compare `skill()` and `args()`. A skipped duplicate is not an error and is not
   listed in the details.
4. API (`incidents.rs`): add a query struct
   ```rust
   /// Options for running triage again.
   #[derive(Debug, Clone, Default, Deserialize, IntoParams)]
   #[into_params(parameter_in = Query)]
   pub struct RetryQuery {
       /// Run the model again even when a cached report would fit. Default false.
       pub fresh: Option<bool>,
   }
   ```
   (put it in `api/dto.rs`), add `params(RetryQuery)` to the `utoipa::path` of
   `retry_triage`, take `Query(query): Query<RetryQuery>`, and call `retriage` when
   `query.fresh == Some(true)`, else `triage`. Regenerate OpenAPI (rules, step 9).
5. Web: `useRetryTriage(id)` takes `fresh: boolean` in `mutate` and sends
   `params: { path: { id }, query: { fresh } }`. In `RetryTriage.tsx`:
   - when `triage?.kind` is `report` or `cached`, the button says **Run triage again**
     and calls `mutate(true)`;
   - otherwise it says **Retry triage** and calls `mutate(false)`;
   - under the button add a muted hint: "Runs the model again with the current skills,
     notes and evidence. Counts against the hourly triage limit."

## Tests

- `crates/domovoi-engine/tests/services/limits.rs`: in a copy of
  `repeat_incident_uses_cache_without_model_call`, after the cached run, call
  `w.triage.retriage(&w.incident)` and assert it returns `Report` and the model was
  called again.
- Engine: a test that running triage twice with the same proposal leaves one proposed
  action (use the existing proposal tests in `tests/services/` as a template).
- Web: in `incidents.test.tsx` assert that with a `report` the button reads
  "Run triage again" and the call sends `query: { fresh: true }`.

## Done when

- "Run triage again" always calls the model (unless a cap is reached) and the new report
  replaces the card.
- No duplicate proposed actions appear from re-running.
- OpenAPI files and the snapshot are regenerated; all tests pass.
