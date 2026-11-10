# 29 · Security: authenticate webhooks before rate limiting them

## Background (verified on `main`)

- `crates/domovoi-hub/src/ingest/handler.rs` `receive()` (`:15-60`):
  1. unknown name → `404`;
  2. takes a token from the webhook's bucket (60 burst, 1/s, `ingest/limit.rs`) → `429`;
  3. only then checks the HMAC or bearer token (`ingest/auth.rs`) → `401`.
- So anyone who can reach `/hooks/{name}` can drain a real webhook's bucket with junk and
  make the real sender (Alertmanager, Uptime Kuma) get 429s. The 404/401 difference also
  tells a scanner which webhook names exist.
- Auth itself is fine: constant-time HMAC (`verify_slice`) and `ct_eq` on the bearer
  hash. The body is capped at 1 MiB before the handler runs.

## Steps

1. Reorder `receive`: look up the webhook; if missing **or** `!authorized(..)`, return
   `401` (same status, same empty body for both). Then take from the bucket.
2. Unauthenticated attempts get their own limit so a scanner cannot make the hub hash
   1 MiB bodies at full speed: one global `Mutex<limit::Bucket>` in `IngestState` for
   failed attempts (capacity 30, 1 per second). When it is empty, answer `429` before
   reading auth. Reuse the bucket type in `ingest/limit.rs`; add a `Clock`-based test.
3. Log a failed attempt at `debug` with the peer address, never the body or headers.
4. Book: `docs/book/src/guide/webhooks.md`: an unknown name and a bad signature both
   answer 401.

## Tests (`crates/domovoi-hub/tests/ingest/main.rs`)

- `an_unknown_webhook_and_a_bad_signature_look_the_same` (both 401).
- `unsigned_requests_do_not_use_up_a_webhooks_rate_limit`: 100 bad requests, then a
  signed one is accepted (202).
- `too_many_failed_attempts_are_slowed_down`: the 31st bad request in one second is 429.
- Keep the INV-12 gate tests in `crates/domovoi-hub/tests/gates.rs` passing.

## Done when

- Only authenticated requests spend a webhook's budget, and webhook names cannot be told
  apart from outside.
