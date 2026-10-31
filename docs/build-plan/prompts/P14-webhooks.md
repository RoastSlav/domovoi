# P14 — Webhook ingest and local relay

| | |
| --- | --- |
| Phase | 3, Hub core |
| Depends on | P08, P13 |
| Crates | `domovoi-checks` (new feature `sources`), `domovoi-hub`, `domovoi-agent` |
| Size | M |
| Invariants touched | INV-12 (webhooks need a valid signature or token) |

## Goal

Accept alerts from tools people already run (generic JSON, Prometheus Alertmanager, Uptime Kuma) on the hub, authenticated by HMAC or bearer token, and let agents relay the same payloads from `localhost` so services on a guarded host never need network access to the hub.

## Read first

- `ARCHITECTURE.md` §7, §10, INV-12
- P08 (checks crate layout), P13 (hub `SignalSink`)

## In scope

### Parsers (`domovoi-checks`, feature `sources`, pure)

Add a `sources` feature with no extra dependencies beyond `serde_json`. Enable it from the agent and the hub.

- `generic::parse(body: &[u8], default_host: &HostName) -> Result<Vec<SourceSignal>, ParseError>` for the documented generic schema:

  ```json
  { "host": "nas", "check": "backup.nightly", "target": "photos", "status": "firing",
    "severity": "warning", "summary": "Backup failed", "evidence": { "exit_code": 2 } }
  ```

  An array of these is also accepted.
- `alertmanager::parse`: Alertmanager webhook v4: one `SourceSignal` per alert. `check` = `alertmanager.` + sanitised `alertname`; `target` from `instance` (or `job`); severity from the `severity` label (`critical`/`warning`/else info); status from each alert's `status`; summary from `annotations.summary` or `description`; all labels into evidence (excluding volatile ones such as `fingerprint`). Host from a configurable label (`host`, `instance` without port) or the webhook's `default_host`.
- `uptime_kuma::parse`: `heartbeat.status` 0 = firing, 1 = resolved; `check` = `uptime_kuma.monitor`; `target` = monitor name; summary from `msg`; URL and type into evidence.
- `SourceSignal` carries a host *name*; the receiver resolves it to a `HostId` (creating an `External` host if unknown).
- Size limit 1 MiB; at most 100 signals per payload; strings truncated to their model limits.

### Hub endpoints (`domovoi-hub`, `ingest/`)

- `POST /hooks/{name}` where `{name}` is a configured `[webhooks.<name>]`. Unknown name → 404.
- Auth per webhook config: `hmac` → header `X-Domovoi-Signature: sha256=<hex>` over the raw body with the webhook secret (HMAC-SHA256 via `hmac` + `sha2`), constant-time compare via `subtle`; `bearer` → `Authorization: Bearer <secret>` constant-time compare. Missing or wrong → 401 with no detail. (INV-12)
- Rate limit per webhook: 60 requests/minute (token bucket), 429 when exceeded.
- Parse → resolve hosts → `SignalSink::submit` → 202.
- Use axum for these routes now; P18 merges them into the full router.

### Agent relay (`domovoi-agent`, `relay.rs`)

- Optional listener from `[local_webhooks]` (default `127.0.0.1:7422`, loopback only; refuse to bind a non-loopback address unless `allow_non_loopback = true`). Same paths and parsers; optional bearer token.
- Relayed signals use this agent's host as the default host and go into the normal spool and `signals` batches.

## Out of scope

- Other source formats (Grafana, Netdata, Beszel): later prompts add parsers following the same pattern.

## Tests required

Realistic fixtures under `crates/domovoi-checks/tests/fixtures/webhooks/` (an Alertmanager group with one firing and one resolved alert; Uptime Kuma down and up; generic single and array).

1. Parser tests per format, including `volatile_labels_excluded_from_targets`.
2. `oversized_payload_is_rejected` and `too_many_signals_are_rejected`.
3. Hub: `valid_hmac_is_accepted`, `invalid_or_missing_signature_is_401`, `bearer_token_mode`, `unknown_webhook_is_404`, `rate_limit_returns_429`, `unknown_host_creates_external_host`.
4. `crates/domovoi-hub/tests/gates.rs`: `webhook_without_auth_never_reaches_signal_sink` (INV-12).
5. Agent: `relay_refuses_non_loopback_bind_by_default`, `relayed_alert_is_spooled_and_sent_with_agent_host`.

## Architecture gates

- Parsers are pure (no I/O) and live in `domovoi-checks`, shared by hub and agent without breaking R1.
- No new crate edges.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/guide/webhooks.md` shows configuration for Alertmanager, Uptime Kuma and the generic format, including how to compute the HMAC with `openssl dgst` for testing.

## Deliverables

- `crates/domovoi-checks/src/sources/*`, `crates/domovoi-hub/src/ingest/*`, `crates/domovoi-agent/src/relay.rs`
- Fixtures, tests, `docs/book/src/guide/webhooks.md`

## PR

Title: `feat(ingest): signed webhooks for generic, Alertmanager and Uptime Kuma, plus agent relay`
