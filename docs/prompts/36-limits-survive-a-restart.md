# 36 · Bug: the loop guard, the hourly triage count and triage retries survive a restart

A settings save restarts the hub runtime in-process (ARCHITECTURE.md §14), so today
every save resets these. Do it after 25 and 26. No new table: everything is rebuilt at
start from what is already stored.

## Background (verified on `main`)

- **Loop guard**: `Shared.dispatches` (`crates/domovoi-engine/src/actions/mod.rs:99-104`),
  checked in `executor.rs` `over_loop_guard` (`:206`), filled by `remember_dispatch`
  (`:230`). In memory only. Every dispatch is also audited as `action.dispatched`
  with `host` and `skill` in the detail (`actions/audit.rs:65-79,149`).
- **Hourly triage count**: `Limits.starts` (`triage/run.rs:24`, moved to
  `triage/admission.rs` by prompt 26). In memory only.
- **Deferred retries**: `Shared.deferred` (`triage/mod.rs:168`), filled by `defer()` when
  the hourly cap skips a run or a run fails, served by `queue.rs` `retry_deferred`
  (`:105`). In memory only, so a restart drops every pending retry and the incident
  stays "triage skipped" until someone presses Redo.
- Each triage outcome is written to the incident timeline as `triage.report`,
  `triage.cached`, `triage.skipped` or `triage.failed`, with the outcome as JSON detail
  (`crates/domovoi-engine/src/manager/annotate.rs:52-62`).
- `AuditLog::list` takes an `AuditFilter { kind, since, .. }`
  (`crates/domovoi-core/src/ports/repos/audit.rs`).

## Steps

1. **Loop guard.** `ActionService::spawn` (or the writer start from 25b) lists audit
   events with `kind = "action.dispatched"`, `since = now - 1h` (page through) and seeds
   `dispatches` from each detail's `host` and `skill`. Malformed details are skipped.
2. **Hourly count.** Add `IncidentRepo::timeline_since(kinds: &[&str], since)` returning
   `(incident, at, kind)` (store: one indexed query on `incident_events`; add an index on
   `(kind, at)` in a new migration if none fits). At triage start, seed `Limits.starts`
   with the `at` of every `triage.report` and `triage.failed` entry in the last hour
   (cached and skipped runs did not call a model). Questions from prompt 26 are not on
   the timeline; follow-up entries (`triage.followup`) count too, general chat does not
   (accepted gap; say so in the doc comment).
3. **Retries.** At triage start, for every **active** incident whose newest triage
   entry is `triage.skipped` with the hourly-cap reason or `triage.failed`, call
   `defer()` with `RetryReason::Cap` (due when the next slot frees) or
   `RetryReason::Failed` (due `FAILED_RETRY` after that entry), respecting
   `FAILED_RETRY_LIMIT` by counting that incident's consecutive failed entries. Skips for
   the monthly budget are not retried.
4. Put the start-up seeding in a new `triage/restore.rs` and `actions/restore.rs`, each
   with a `//!` line, so `run.rs`, `queue.rs` and `executor.rs` do not grow.

## Tests

- `the_loop_guard_remembers_dispatches_from_before_a_restart`: audit three dispatches of
  one skill in the last hour, start the service, approve a fourth: it is blocked.
- `the_hourly_triage_count_survives_a_restart`.
- `a_triage_skipped_by_the_cap_is_retried_after_a_restart`.
- `a_triage_that_failed_three_times_is_not_retried_after_a_restart`.

## Done when

- Saving settings does not let a skill run a fourth time in an hour, does not reset the
  triage cap, and does not drop pending triage retries.
