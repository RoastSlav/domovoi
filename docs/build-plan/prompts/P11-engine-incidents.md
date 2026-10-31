# P11 — Engine I: incident manager and correlation

| | |
| --- | --- |
| Phase | 3, Hub core |
| Depends on | P03, P10 |
| Crates | `domovoi-engine` |
| Size | L |
| Invariants touched | single writer (ADR 0011); fingerprint rule; INV-11 groundwork |

## Goal

Build the single-writer incident manager: it ingests signals, correlates them into incidents (fingerprints, recurrence folding, cross-source grouping), applies damping, TTL, maintenance windows and ack rules, records entity history, persists, and publishes domain events. All decisions are pure functions tested with the fake clock.

## Read first

- `ARCHITECTURE.md` §5.2–§5.4, §7.1
- ADR 0011
- P01 incident state machine (`apply`), P03 ports

## In scope

Add `tokio` (`sync`, `time`, `rt`, `macros`), `tokio-util` (`CancellationToken`), `async-trait`, `tracing`, `serde`, `thiserror`, `jiff`, `uuid`. Dev: `domovoi-testkit`, `proptest`, `insta`.

### Events (`events.rs`)

`DomainEvent` (`Clone`, broadcast to many consumers): `IncidentOpened { incident }`, `IncidentFolded { incident, signal, occurrences }`, `IncidentEscalated { incident, from, to }`, `IncidentReopened { incident }`, `IncidentResolved { incident }`, `IncidentAcked { incident, by }`, `IncidentMuted { incident, until }`, `IncidentUnmuted`, `TriageRequested { incident }` (emitted on open, reopen and escalation), plus action and triage variants that later prompts add. Each carries the incident snapshot after the change and a `notify: bool` flag (false when suppressed by maintenance or an ack rule).

### Correlation (`correlate.rs`, pure)

`fn correlate(signal: &Signal, candidates: &Candidates, policy: &CorrelationPolicy, now) -> Target` where `Target` is one of `Existing(IncidentId)`, `Reopen(IncidentId)`, `New`, `Ignore` (resolved signal with nothing to resolve). Rules in order:

1. Active incident with the same fingerprint → `Existing`.
2. Resolved incident with the same fingerprint within `reopen_window` → `Reopen`.
3. Cross-source grouping: an active incident on the same host whose **normalised target** equals this signal's normalised target, opened within `grouping_window` (5 min), and from a different source → `Existing` (record the extra fingerprint on the incident's related list). Normalisation: lowercase, strip common container prefixes/suffixes (`-1`, `_1`, compose project prefix), strip URL scheme and port. Write it as a small pure function with a table test.
4. Resolved signals with no active match → `Ignore`.
5. Otherwise `New`.

### Suppression (`suppress.rs`, pure)

`fn suppression(signal, incident, acks: &[AckRule], windows: &[MaintenanceWindow], now) -> Option<SuppressReason>`. Suppressed incidents still change state and persist; their events carry `notify: false`.

### Incident manager (`manager/`)

- `IncidentManager::spawn(deps: ManagerDeps { repos, clock, policy, bus: broadcast::Sender<DomainEvent> }) -> (ManagerHandle, JoinHandle)`.
- `ManagerHandle` sends `Command`s over a bounded `mpsc` (capacity 1024) with a `oneshot` reply where useful: `Ingest(Vec<Signal>)`, `Ack { incident, by }`, `Mute { incident, until, by }`, `Unmute { incident }`, `Tick`. Later prompts add `AttachTriage` and action commands. `ManagerHandle` also implements the `SignalSink` port.
- Loop: receive command → load needed state (cache of active incidents loaded at start, refreshed on miss) → call the pure decision function `decide(&State, &Command, &Policy, now) -> Decision { writes, events }` → apply writes through repos (signal insert, incident insert/update, timeline append) → publish events **after** successful writes. A store error is logged, the command fails, no event is published.
- `Tick` every 30 s from a small timer task using the `Clock`: drives mute expiry and TTL resolution through `apply(Tick)`.
- Pending incidents that are dropped (resolved before confirmation) are deleted and counted in a `flaps` metric.
- History: when a firing signal carries evidence `value`, it is recorded via `apply` (P01) into `EntityHistory`.

## Out of scope

- Notifications, cards, quiet hours, digest (P12). Triage (P16). Actions (P17).

## Tests required

1. `correlate` table tests covering each rule and the precedence between them.
2. `normalised_target_table` (`plex`, `plex-1`, `media_plex_1`, `https://plex.example:32400` → `plex`).
3. `decide` unit tests with the fake clock:
   - `two_firings_confirm_and_open_once` (one `IncidentOpened`, one `TriageRequested`).
   - `immediate_signal_opens_on_first_firing`.
   - `repeat_firing_folds_and_does_not_open_again`.
   - `resolved_then_firing_within_window_reopens_same_incident`.
   - `uptime_kuma_down_and_container_unhealthy_group_into_one_incident`.
   - `maintenance_window_suppresses_notify_but_keeps_state`.
   - `ack_rule_with_evidence_predicate_suppresses_matching_signals_only`.
   - `ttl_tick_resolves_quiet_incident`.
   - `pending_resolved_before_confirmation_is_deleted_and_counted`.
   - `value_evidence_is_recorded_in_history`.
4. Actor tests with in-memory repos: `events_are_published_only_after_writes`, `store_error_publishes_nothing`, `commands_are_processed_in_order`.
5. `at_most_one_active_incident_per_fingerprint`: proptest over random signal streams for a few fingerprints.
6. `signal_sink_ingest_round_trip`.

## Architecture gates

- Engine imports only `domovoi-core` internally and no I/O modules (R2).
- All state changes to incidents go through `IncidentManager` (no repo `update` calls for incidents elsewhere in engine; reviewers check).

## Acceptance criteria

- `cargo xtask ci` passes.
- `decide` and `correlate` have no `async` and no dependencies on ports.

## Deliverables

- `crates/domovoi-engine/src/{events,correlate,suppress,manager/*,policy}.rs`
- Tests under `crates/domovoi-engine/tests/` and unit tests

## PR

Title: `feat(engine): single-writer incident manager with correlation and suppression`
