# 38 · Docs: bring ARCHITECTURE.md and the book back in line with the code

Run last in a batch, after the code prompts you plan to merge, so it describes the final
state. Docs only; changing an invariant still needs an ADR (none does here).

## Mismatches found (verified on `main`)

**ARCHITECTURE.md**

- §6 ports table lists 7 repos. Missing: `LogSource` (`ports/logs.rs`), `SignalSink`
  (`ports/agent_link.rs`), `InteractionSink` (`ports/chat.rs`), `SettingsRepo`,
  `UserRepo`, `SessionRepo` (`repos/accounts.rs`), `AdvisoryRepo`, `FactRepo`
  (`repos/knowledge.rs`), `NotifyStateRepo` (`repos/notify.rs`), `TriageCacheRepo`,
  `UsageRepo` (`repos/triage.rs`), plus anything added by 32 (`HostSettingsRepo`).
- §8 protocol table is missing `enroll`, `enrolled`, `challenge_response`,
  `signals_ack`, `get_logs`, `logs`, `run_check`, `check_result`, and does not mention
  the minimum-release rule (`MIN_AGENT_VERSION`, `MIN_HUB_VERSION`,
  `crates/domovoi-proto/src/version.rs:44-56`) or the `agent_too_old` reject reason.
- §9 lists a `digests` table; it is `digest_facts`. `notify_state` (migration 0002) is
  missing. "transcripts 90 days" matches nothing: the 90-day purge covers
  `incident_events` and `triage_cache` (`crates/domovoi-store/src/purge.rs`). Advisories
  (30 days after resolving), ended maintenance windows and expired sessions are purged
  too; incidents, `audit_log`, `actions`, `llm_usage`, `digest_facts`,
  `enrollment_tokens`, notes are kept.
- §10: add that `/healthz` and `/metrics` are unauthenticated **by design**, for
  Prometheus, Netdata and similar scrapers, and should be reached through the reverse
  proxy or the local network only.
- §11: follow-ups and the general chat share the triage budget and hourly cap (after 26).
- §13: says a generic JSON webhook notifier ships in v0.1; only Discord exists
  (`crates/domovoi-chat/src/discord/`). Move "generic webhook" to §19 with ntfy, Slack,
  Telegram and Matrix as planned notifiers and gateways.
- §16: add `domovoi_incident_flaps_total` and `domovoi_event_bus_lagged_total` to the
  metrics list.

**Book (`docs/book/src/`)**

- `reference/protocol.md:25` says protocol 1.0; the code is 1.1 (or later after 15a).
  The message table and examples omit `get_logs` and `logs`; the `hello` example shows
  `"minor": 0`.
- `reference/cli.md`: no `domovoi upgrade` section (skip if 31 already added it).
  Check the flap metric wording at `:182` against `concepts/incidents.md:27`.
- `reference/backup.md`: no restore procedure for `hub.db` (skip if 43 added it).
- `reference/api.md` is hand-written and partial. Rather than listing every route by
  hand, say the generated `web/src/api/openapi.json` is the full reference and keep
  prose only for auth, CSRF, errors, SSE and webhooks. Add one line each for the routes
  people script against: chat, ack rules, maintenance (after 33), host logs, updates.
- `reference/data-model.md`: add `llm_usage` and `enrollment_tokens` to "kept", and the
  tables added by 32 and 33.
- `guide/hardening.md`: the `/metrics` note from §10 above.

## Steps

1. Fix each item above in place. Keep the existing tone: short, plain sentences.
2. `cargo xtask docs-links` must pass. If a config field changed in a merged prompt, run
   `cargo xtask docs-config`.
3. Do not touch ADRs except to fix a broken link.

## Done when

- Every message kind, port, table and metric in the code appears in ARCHITECTURE.md, and
  no document claims a feature that does not exist.
