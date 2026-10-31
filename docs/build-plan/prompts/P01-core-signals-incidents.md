# P01 — Core model I: identifiers, hosts, signals, fingerprints, incidents

| | |
| --- | --- |
| Phase | 1, Core contracts |
| Depends on | nothing (scaffold only) |
| Crates | `domovoi-core` only |
| Size | M |
| Invariants touched | none directly; establishes the fingerprint rule ("no volatile values") |

## Goal

Create the foundational domain types every other crate builds on: typed identifiers, hosts, severities, signals with their keys and evidence, fingerprints, verdicts, entity history, and the incident state machine as a pure function. Nothing here does I/O.

## Read first

- `ARCHITECTURE.md` §4 (rules R1, R2, R7–R10), §5.1–§5.3
- `AGENTS.md`
- ADR 0002, ADR 0011 (why transitions are a pure function)

## In scope

### Dependencies

Add to `domovoi-core` (all already on its allowlist): `serde`, `serde_json`, `thiserror`, `jiff`, `uuid`, `sha2`, `hex`. Dev: `proptest`, `insta` (add `proptest` and `insta` as dev-dependencies via `{ workspace = true }`).

### Modules and types

Create these modules under `crates/domovoi-core/src/` and re-export the main types from `lib.rs`:

1. **`id.rs`**: a private `macro_rules!` that defines UUIDv7 newtypes with `new()`, `from_uuid`, `as_uuid`, `Display`, `FromStr`, `serde(transparent)`, `Copy`, `Ord`, `Hash`. Define `HostId`, `AgentId`, `IncidentId`, `SignalId`, `ActionId`, `UserId`, `NoteId`, `RequestId`.
2. **`host.rs`**:
   - `HostName`: validated slug `^[a-z0-9][a-z0-9-]{0,62}$`, implemented by hand (no `regex` yet). `HostName::parse(&str) -> Result<HostName, ValidationError>`, `as_str()`, serde via `try_from = "String"`.
   - `HostKind { Local, Remote, External }` (External = only known from webhooks, no agent).
   - `Host { id, name, kind, labels: BTreeMap<String, String>, created_at }`.
3. **`severity.rs`**: `Severity { Info, Warning, Critical }` with `Ord` (Info < Warning < Critical), lowercase serde, `Display`.
4. **`signal.rs`**:
   - `CheckKey`: dotted lowercase segments, each `[a-z0-9_]+`, 1–8 segments, total ≤ 64 chars (e.g. `container.restart_loop`, `disk.free`). Validated by hand.
   - `TargetKey`: 1–128 chars, no control characters, trimmed. Examples: `plex`, `/dev/sdc`, `/mnt/data`.
   - `SourceKind { BuiltIn, Webhook { kind: String }, Probe, Agent }` (serde tagged `{"type": "...", ...}`).
   - `SignalStatus { Firing, Resolved }`, `SignalClass { Normal, Immediate }`.
   - `EvidenceValue { Text(String), Int(i64), Float(f64), Bool(bool) }` (serde untagged) and `Evidence(BTreeMap<String, EvidenceValue>)` with `insert`, `get`, `iter`. Reserved key `value`: the tracked numeric value for delta checks.
   - `Signal { id, source, host: HostId, check, target: Option<TargetKey>, status, severity, class, summary: String (≤ 300 chars, enforced by constructor), evidence, observed_at: jiff::Timestamp }` with constructors `Signal::firing(...)` and `Signal::resolved(...)` taking the required fields; summary is truncated with `…` if longer.
5. **`fingerprint.rs`**: `Fingerprint([u8; 32])`, computed as SHA-256 over `"fp:v1\n{host_id}\n{check}\n{target or empty}"`. `Fingerprint::of(host, check, target)`, `Signal::fingerprint()`, hex `Display`, serde as hex string. The doc comment states the rule: evidence, counts, IDs and timestamps never enter a fingerprint.
6. **`verdict.rs`**: `Verdict { ActionNeeded, Monitor, Expected, Fixed }`, serde `SCREAMING_SNAKE_CASE`, `label()` returning `"ACTION NEEDED"`, `"MONITOR"`, `"EXPECTED"`, `"FIXED"`.
7. **`history.rs`**: `EntityHistory` bounded to 64 `(Timestamp, f64)` points (oldest dropped). Methods: `push`, `latest`, `delta_over(window: SignedDuration, now) -> Option<f64>`, `rate_per_day(now) -> Option<f64>` (least-squares slope over points in the last 30 days, `None` with fewer than 2 points).
8. **`incident/` (`mod.rs`, `state.rs`, `transition.rs`)**:
   - `IncidentStatus { Pending, Open, Acked, Muted { until: Timestamp }, Resolved }`.
   - `Incident { id, fingerprint, host, check, target, title, status, severity, verdict: Option<Verdict>, opened_at, confirmed_at: Option, resolved_at: Option, last_firing_at, firing_streak: u8, occurrences: u32, history: EntityHistory }`.
   - `IncidentPolicy { confirm_runs: u8 (default 2), reopen_window: SignedDuration (default 6h), ttl: Option<SignedDuration> (None = only explicit resolve) }`.
   - `IncidentEvent`: `Firing { severity, class, observed_at, value: Option<f64> }`, `Resolved { observed_at }`, `Ack { by: UserId }`, `Mute { until }`, `Unmute`, `Tick` (time passes; used for mute expiry and TTL).
   - `IncidentChange` (what happened, for the caller to persist and publish): `Confirmed`, `StatusChanged { from, to }`, `SeverityChanged { from, to }`, `Folded { occurrences }` (another firing on an open incident), `Reopened`, `Dropped` (pending resolved before confirmation), `ValueRecorded { value }`.
   - `pub fn apply(incident: &mut Incident, event: IncidentEvent, policy: &IncidentPolicy, now: Timestamp) -> Result<Vec<IncidentChange>, TransitionError>` implementing exactly the state diagram in `ARCHITECTURE.md` §5.3. Rules:
     - `Pending` + `Firing` increments `firing_streak`; reaching `confirm_runs` (or `class == Immediate`) moves to `Open` with `Confirmed`.
     - `Pending` + `Resolved` → `Dropped` (caller deletes the pending incident).
     - `Open`/`Acked`/`Muted` + `Firing` → `Folded`, severity may rise (never falls while firing), `value` recorded in history.
     - Any non-resolved state + `Resolved` → `Resolved`, `resolved_at` set.
     - `Resolved` + `Firing` within `reopen_window` of `resolved_at` → `Reopened`, status `Open` (skips confirmation), `occurrences` incremented. After the window the caller must create a new incident: return `TransitionError::ReopenWindowElapsed`.
     - `Ack` only from `Open` or `Muted`; `Mute` from `Open` or `Acked`; `Unmute` from `Muted`.
     - `Tick`: `Muted` past `until` returns to `Open`; with `ttl` set, `Open`/`Acked`/`Muted` whose `last_firing_at + ttl <= now` resolve.
     - Illegal combinations return `TransitionError::Illegal { status, event }` and leave the incident unchanged.
   - `Incident::new_pending(signal: &Signal, now) -> Incident` builds the initial incident from a firing signal (title from the signal summary).
9. **`error.rs`**: `ValidationError` (field, reason) and `TransitionError`.

Keep each file under 500 lines; split `incident/transition.rs` further if needed.

## Out of scope

- Skills, actions, triage, cards, digests (P02). Ports and traits (P03). Any `async`.
- Persistence, correlation across fingerprints, notification rules (engine prompts).

## Tests required

Unit tests next to the code, plus `crates/domovoi-core/tests/incident_transitions.rs` and `tests/serde_snapshots.rs`:

1. `host_name_accepts_valid_and_rejects_invalid`: table test, including uppercase, leading dash, 64 chars, empty.
2. `check_key_validation_table`: valid dotted keys, rejects uppercase, empty segments, 9 segments, 65 chars.
3. `target_key_rejects_control_characters_and_length`.
4. `fingerprint_ignores_evidence_and_timestamps`: proptest over random evidence maps, summaries, severities and timestamps; fingerprints for the same host/check/target are equal.
5. `fingerprint_differs_by_host_check_and_target`.
6. `fingerprint_is_stable`: a fixed input produces a fixed hex string (guards accidental algorithm changes).
7. One test per arrow in the state diagram (named after the transition), e.g. `pending_confirms_after_confirm_runs_firings`, `immediate_signal_opens_without_confirmation`, `pending_resolved_before_confirmation_is_dropped`, `open_firing_folds_and_raises_severity`, `severity_never_falls_while_firing`, `muted_returns_to_open_after_until`, `ttl_resolves_quiet_incident`, `resolved_reopens_within_window`, `resolved_after_window_reports_elapsed`.
8. `illegal_transitions_leave_incident_unchanged`: table of illegal (status, event) pairs.
9. `transitions_never_panic_and_keep_invariants`: proptest generating random event sequences; after each step: `resolved_at.is_some() == (status == Resolved)`, `confirmed_at` set once open, severity ≥ every firing severity since last resolve.
10. `entity_history_is_bounded_and_computes_rate`: 100 pushes keep 64; known linear series gives the expected slope.
11. Insta JSON snapshots for a `Signal` (with every `EvidenceValue` variant) and an `Incident`, using fixed IDs and timestamps.

## Architecture gates

- `cargo xtask arch-check`: core depends only on allowlisted crates; no `std::fs`/`std::net`/`std::process`.
- No `async`, no `tokio` in core.
- Every public item documented.

## Acceptance criteria

- `cargo xtask ci` passes.
- `apply` is the only place incident status changes; `Incident` fields that encode state are not `pub` mutable from outside the module (expose getters; `apply` and `new_pending` are the only mutators).
- Snapshots committed under `crates/domovoi-core/tests/snapshots/`.

## Deliverables

- `crates/domovoi-core/src/{id,host,severity,signal,fingerprint,verdict,history,error}.rs`, `src/incident/*`
- `crates/domovoi-core/tests/{incident_transitions,serde_snapshots}.rs` and snapshots

## PR

Title: `feat(core): identifiers, signals, fingerprints and incident state machine`
