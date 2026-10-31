# P22 — Hub runtime assembly, dispatcher and schedulers

| | |
| --- | --- |
| Phase | 6, Ship |
| Depends on | P20, P21 |
| Crates | `domovoi-hub` |
| Size | M |
| Invariants touched | INV-11 (delivery retries; alerts survive triage failure) |

## Goal

Assemble everything into one running hub: build adapters from config, start the incident manager, triage, action executor, agent server, web server, gateways and notifiers, deliver notifications through a dispatcher with retries, and run the schedulers (digest, quiet-hours release, reminders, expiry, retention, backups, heartbeat). Shut down cleanly.

## Read first

- `ARCHITECTURE.md` §7.1, R6 (hub is the composition root for hub-side adapters)
- P11–P21 public APIs

## In scope

### `HubRuntime` (`runtime/`)

- `HubRuntime::build(config: HubConfig, deps: HubDeps { clock, process_executor: Option<Arc<dyn ProcessExecutor>> }) -> Result<HubRuntime>` and `run(self, shutdown: CancellationToken) -> Result<()>`.
- Build order: store (open, migrate, backup) → redactor (register every resolved secret) → event bus (`broadcast`, capacity 4096) → incident manager → LLM providers (primary/secondary from config; the CLI kind uses `process_executor`) → triage service → approval service and action executor → agent server and `WsAgentLink` → chat gateways and notifiers → interaction authorizer → web router (API, SPA, webhooks, metrics) → schedulers → dispatcher.
- If the event bus lags (`RecvError::Lagged`), consumers reload state from the store instead of crashing; count it in a metric.

### Dispatcher (`runtime/dispatcher.rs`)

- Subscribes to the bus; for each event calls the P12 policy, then performs the resulting actions against every enabled gateway (`ChatGateway`) and, for digests and held summaries, notifiers.
- Persists message and thread references in `NotifyState` so updates and replies target the right message after restarts.
- Retries each delivery with exponential backoff (1 s, 2 s, 4 s, 8 s, 16 s); after the last failure, records an advisory-level problem (`delivery_failed` count shown in settings) and keeps going. A failed delivery never blocks other incidents.

### Schedulers (`runtime/schedulers.rs`)

All driven by the `Clock` port with the hub's time zone:

| Job | When |
| --- | --- |
| Incident manager tick (TTL, mute expiry) | every 30 s |
| Reminders | every minute, via the P12 policy |
| Quiet-hours release | at the configured end time |
| Digest | first tick after the configured local time, once per day |
| Action approval expiry | every minute |
| Advisory evaluation | every 5 minutes |
| Retention purge | daily at 03:30 local |
| Backup (`VACUUM INTO`) | daily at 04:00 local, keep `[backup] keep` |
| Heartbeat ping | every `[heartbeat] interval` (HTTP GET, 10 s timeout); failures feed the `HeartbeatMissing` advisory |

### Web server

- Bind `[web] bind`; graceful shutdown on cancellation; SSE connections end cleanly.

### Shutdown

- Cancellation stops accepting new work, lets in-flight triage runs finish up to 10 s, flushes the manager queue, closes agent sessions with a `going_away` close code, and closes the store. Total shutdown ≤ 15 s.

## Out of scope

- CLI parsing and single-box mode (P23). Packaging (P24).

## Tests required

With fakes for providers and gateways (`RecordingGateway`, `ScriptedLlm`), a temp store and the fake clock:

1. `end_to_end_signal_to_card_to_triage_update` (webhook in → card posted → triage report → card updated).
2. `delivery_retries_then_records_failure_without_blocking_others`.
3. `message_references_survive_restart` (rebuild runtime on the same store; resolution updates the original message).
4. `digest_sent_once_per_day_at_local_time` (fake clock across two days).
5. `quiet_hours_release_happens_at_end_time`.
6. `heartbeat_failure_raises_advisory_and_success_clears_it` (wiremock).
7. `bus_lag_triggers_reload_not_crash`.
8. `shutdown_completes_within_15_seconds_with_inflight_triage`.

## Architecture gates

- The hub constructs only hub-side adapters; process execution only through the injected `ProcessExecutor` (R3, R6).

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/concepts/how-it-works.md` describes the runtime pipeline with a diagram.

## Deliverables

- `crates/domovoi-hub/src/runtime/*`
- Tests; `docs/book/src/concepts/how-it-works.md`

## PR

Title: `feat(hub): runtime assembly, delivery dispatcher and schedulers`
