# 04 · Fix: triage skipped by the hourly limit never runs again

## Goal

When a triage run is skipped because `[triage] per_hour_runs` is reached, the hub
remembers the incident and triages it automatically as soon as a run slot frees up,
unless the incident resolved or was triaged meanwhile.

## What is already known (verified)

- `crates/domovoi-engine/src/triage/run.rs`, `over_cap()` keeps run start times in
  `Limits.starts` (a `VecDeque<Timestamp>`) and returns
  `"the limit of {n} triage runs per hour is reached"`. `outcome()` turns that into
  `TriageOutcome::Skipped` and nothing ever retries it.
- `crates/domovoi-engine/src/triage/queue.rs`: `serve()` spawns the workers in a
  `JoinSet`; `Queue::push(incident)` queues an incident (replacing a waiting duplicate)
  and returns a dropped one when full, which `skip_dropped()` handles.
- `Shared` (in `triage/mod.rs`) holds `deps`, `settings`, `limits` and `queue`.
- The engine must use the `Clock` port for time: `deps.clock.now()` and
  `deps.clock.sleep_until(t)`. Tests use `FakeClock::advance`.
- Both files are under 300 lines, so there is room.

## Read these files

- `crates/domovoi-engine/src/triage/mod.rs`
- `crates/domovoi-engine/src/triage/run.rs`
- `crates/domovoi-engine/src/triage/queue.rs`
- `crates/domovoi-engine/tests/services/limits.rs` and `tests/services/harness.rs`

## Steps

1. `run.rs`: make the cap explicit.
   ```rust
   /// Why a run may not start.
   enum Cap {
       /// The monthly budget is spent.
       Monthly,
       /// The hourly limit is reached; a slot frees at `retry_at`.
       Hourly { retry_at: Timestamp },
   }
   ```
   `over_cap` returns `Option<Cap>`. For the hourly case, `retry_at` is the oldest entry
   of `limits.starts` plus one hour plus one second.
2. `run.rs`, in `outcome()`: for `Cap::Monthly` keep today's reason text. For
   `Cap::Hourly { retry_at }`, call `shared.defer(incident.id(), retry_at)` (step 3) and
   return `Skipped` with the reason
   `format!("the limit of {} triage runs per hour is reached; it runs again at {} UTC", n, retry_at.strftime("%H:%M"))`.
   Keep the words "per hour" in the text (an existing test matches on them).
3. `mod.rs`: add to `Shared`
   `pub(crate) deferred: std::sync::Mutex<Vec<(Timestamp, IncidentId)>>` and
   `pub(crate) deferred_ready: tokio::sync::Notify`; initialise them in
   `TriageService::spawn`. Add
   `impl Shared { pub(crate) fn defer(&self, id: IncidentId, at: Timestamp) }` which
   replaces an existing entry for the same id (keep the later time) or pushes a new one,
   then calls `self.deferred_ready.notify_one()`. Lock with
   `.lock().unwrap_or_else(std::sync::PoisonError::into_inner)` like `limits()` does.
4. `queue.rs`: add `async fn retry_deferred(shared: Arc<Shared>)` and spawn it into the
   same `JoinSet` in `serve()`. Loop until `shared.deps.shutdown` is cancelled:
   - Read the earliest `at` in `deferred`. If none, `select!` on shutdown and
     `deferred_ready.notified()`. Otherwise `select!` on shutdown,
     `deferred_ready.notified()` and `shared.deps.clock.sleep_until(at)`.
   - Take every entry with `at <= clock.now()` out of the list.
   - For each id: load it with `shared.deps.incidents.get(id)`; skip it when missing,
     `Pending` or `Resolved`. Load `shared.deps.incidents.timeline(id)` and find the last
     entry whose kind starts with `"triage."` and is not `"triage.followup"`; re-queue only
     when that entry is `"triage.skipped"`. Re-queue with `shared.queue.push(incident)`,
     handling a dropped incident with `skip_dropped` as `serve()` does.
5. Add a sentence to `docs/book/src/concepts/incidents.md` (or the triage section of
   `docs/book/src/guide/llm-providers.md`): incidents skipped by the hourly limit are
   triaged automatically once a run slot frees.

## Tests

In `crates/domovoi-engine/tests/services/limits.rs` add
`hourly_cap_skip_runs_again_when_a_slot_frees`:

1. `per_hour_runs: 1`; push two report answers to `w.primary`.
2. `w.run()` → `Report`; `w.run()` → `Skipped` whose reason contains "runs again at".
3. `w.clock.advance(SignedDuration::from_mins(61))`.
4. Loop up to 200 times calling `tokio::task::yield_now().await` until
   `w.primary.requests().len() == 2`; then assert the incident's timeline ends with a
   `triage.report` entry.

Keep `per_hour_cap_and_monthly_budget_skip_without_model_call` passing.

## Done when

- A skipped-by-hourly-cap incident is triaged once the window allows, without a human.
- Resolved incidents and incidents triaged meanwhile are not re-run.
- `cargo test -p domovoi-engine` and `cargo clippy` pass; `run.rs` and `queue.rs` stay
  under 500 lines.

## Do not

- Do not retry monthly-budget skips.
- Do not use `tokio::time::sleep`; use `clock.sleep_until`.
