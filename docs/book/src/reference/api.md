# HTTP API

The hub serves a JSON API under `/api/v1` on its web listener (`[web] bind`). The web UI uses the same API, so anything the UI can do, a script can do too.

The full OpenAPI 3.1 document is committed as [`web/src/api/openapi.json`](https://github.com/RoastSlav/domovoi/blob/main/web/src/api/openapi.json). To print it from your hub's own binary, run `domovoi openapi`. It is generated from the hub's routes, so it always matches the version you run.

## First run

The admin account does not exist yet when the hub first starts. The hub logs a one-time setup link like this:

```text
Open https://domovoi.example.net/setup?token=3f9c…
```

The link works once, for 24 hours. Opening it in the web UI asks for a username and a password of at least 12 characters. To do the same from a script:

```bash
curl -c cookies.txt -X POST https://domovoi.example.net/api/v1/auth/setup \
  -H 'Content-Type: application/json' \
  -d '{"token":"3f9c…","username":"admin","password":"a long passphrase"}'
```

Once an account exists, setup is closed. A restart of a hub that already has an account issues no new link.

## Sessions

`POST /api/v1/auth/login` with `{"username", "password"}` sets a `domovoi_session` cookie:
- It is `HttpOnly`, `SameSite=Strict`, `Path=/`, and `Secure` when `public_url` uses https.
- A session ends after 12 hours without use, and 7 days after login in any case.
- `POST /api/v1/auth/logout` ends it.

After five failed logins in a minute from one address, that address is locked out for a minute. Each further lockout doubles, up to 15 minutes. Error messages never say whether the username or the password was wrong.

Behind a reverse proxy, list the proxy's address in `[web] trusted_proxies`, so `X-Forwarded-For` is used to tell clients apart.

## CSRF

Every `POST`, `PUT`, `PATCH` and `DELETE` under `/api/v1` must carry the session's CSRF token:

1. `GET /api/v1/session` returns `{"user": …, "csrf_token": "…"}`. The login and setup responses also include the token.
2. Send it as the `X-CSRF-Token` header.

The only exceptions are login and setup, which happen before a session exists. A request with an `Origin` header must also come from the hub's own origin, the scheme and host of `public_url`. A missing or wrong token, or a foreign origin, gets `403 Forbidden`.

```bash
csrf=$(curl -s -b cookies.txt https://domovoi.example.net/api/v1/session | jq -r .csrf_token)
curl -b cookies.txt -X POST https://domovoi.example.net/api/v1/incidents/$ID/ack \
  -H "X-CSRF-Token: $csrf"
```

## Approving actions

`POST /api/v1/actions/{id}/approve` approves a proposed action as the logged-in user. It requires `{"plan_hash": "<64 hex characters>"}`: the plan hash shown on the card, also returned as `plan_hash` by the action endpoints. Without a valid hash the request gets a 400; with a hash that differs from the stored one it gets a 409, and the action stays proposed. Destructive skills also need `{"confirm_text": "<skill name>"}` with the skill's name typed exactly. Chat buttons can never approve them.

## Operation controls

Four requests control an operation. Each needs a logged-in session and the CSRF header, none needs your password again, and each answers `{"as_of_seq": <number>, "operation": {...}}` with the refreshed operation (the same object the operation list returns). `as_of_seq` is the event-stream position the answer was read at. While operations are off, all four answer 503 "orchestration is not enabled".

- `POST /api/v1/operations/{id}/cancel` with `{"reason": "wrong host"}` (send `{}` for no reason; at most 500 characters). Pending steps and effects are cancelled. A run already handed to an agent is not recalled; its result is still recorded. Cancelling a cancelled operation changes nothing and answers 200. A finished operation answers 409 "the operation has already finished".
- `POST /api/v1/operations/{id}/pause` with no body. The operation stops adding steps until you resume it, and its pause reason is `manual`. Pausing a paused operation answers 200 without a change. An operation that has not started answers 409 "the operation has not started yet; cancel it instead".
- `POST /api/v1/operations/{id}/resume` with `{"extend": [{"kind": "executions", "limit": 6}]}` (send `{}` to raise nothing). `kind` is `model_turns`, `evidence_requests`, `executions`, `cycles`, `wall_time` (seconds), `tokens` or `cost` (millionths of a currency unit); each kind at most once, at most 7 entries, and `limit` from 1 to 10000 for a count or 1 to 10000000000 otherwise. A limit can be raised here but never removed. An operation paused because a limit ran out stays paused, with a 409, until that limit has room again. An operation that is not paused answers 409 "the operation is not paused".
- `POST /api/v1/operations/{id}/steps/{step}/answer` with `{"text": "That disk is the backup disk; ignore it."}`, where `{step}` is the question's step id. The text is trimmed, must have 1 to 2000 characters, and is stored with secrets removed. A step that is not a question, a question that is no longer waiting, a paused operation and a finished one answer 409 with a sentence saying which.

An unknown operation or step is 404, and a bad reason, limit or answer is 400 with the reason in `message`.

## Policy

The policy routes read and change the policy draft, publish numbered versions and show shadow decisions. See [Policy](../concepts/policy.md) for what the terms mean.

Reads need a logged-in session. Every change needs the session and the CSRF header.

- `GET /api/v1/policy` returns the published version (its number, rule counts and hash) and the draft, if one exists.
- `GET /api/v1/policy/versions` lists published versions, newest first.
- `GET /api/v1/policy/versions/{number}` returns one published version with its rules and its acknowledgements. `404` for a number that was never published.
- `GET /api/v1/policy/draft` returns the draft and its rules.
- `PUT /api/v1/policy/draft` saves changes to the draft. A change to a locked rule also needs the admin password in the body.
- `DELETE /api/v1/policy/draft` discards the draft. The published version is not affected.
- `POST /api/v1/policy/preview` shows what the draft would change against the published version. The answer includes the `preview_hash` that publishing needs.
- `POST /api/v1/policy/draft/publish` turns the draft into the next version. It needs the admin password and the `preview_hash` from the preview. A hash that does not match the current draft is refused, so preview again.
- `POST /api/v1/policy/simulate` shows what the draft and the published version would decide for one described step. It changes nothing.
- `GET /api/v1/policy/shadow` returns the shadow summary: decision rows in a time window, counted by status and by enforced outcome, with the number of degraded and differing rows.
- `GET /api/v1/policy/shadow/{execution_id}` returns one decision row: both answers, the inputs, and the gap list. `404` for an execution with no row.

## Errors and paging

Errors are JSON, with an HTTP status to match:

```json
{ "error": "not_found", "message": "not found" }
```

| Status | `error` | Meaning |
| --- | --- | --- |
| 400 | `bad_request` | invalid input |
| 401 | `setup_required` | no account exists yet; open the setup link |
| 401 | `unauthorized` | not logged in |
| 401 | `bad_credentials` | wrong username or password |
| 403 | `forbidden` | CSRF token, origin or confirmation missing or wrong |
| 404 | `not_found` | no such object |
| 409 | `conflict` | not possible in the current state |
| 429 | `too_many_requests` | locked out; see `Retry-After` |
| 503 | `unavailable` | a needed part, such as triage, is not configured |

Lists return `{"items": […], "next_cursor": "…"}`, newest first. Pass `cursor=<next_cursor>` for the next page; `limit` is 1 to 200 and defaults to 50.

## Hosts

`GET /api/v1/hosts` lists every host with its agent's presence, whether the agent is older than the hub (`outdated`), the protocol state (`compatible`, `incompatible`, `expected_incompatible` or `unknown`) with the reported and hub protocol versions, and how many of its incidents are not yet resolved (`open_incidents`). `GET /api/v1/hosts/{name}` adds the latest value of each digest fact the host reported in the last 24 hours.

`GET /api/v1/hosts/{name}/capabilities` returns the host's capability inventory: one row per contract the hub knows, with its state (`available`, `not_installed`, `disabled_by_hub`, `not_allowed_on_host`, `unavailable`, `host_unknown`, `host_absent` or `host_incompatible`), the reason code of a refusal, how much of the host's implementation list was read (`complete`, `partial` or `stale`), the effect class, the target kind, the execution path, and whether the agent runs as root or reported an isolated skill runner. `unknown_contracts` lists ids the agent reported that no contract describes. The endpoint is read-only: it enables, disables and approves nothing.

## Asking about an incident

`POST /api/v1/incidents/{id}/ask` asks the triage model one question about an incident. It reads with the same read-only tools triage has, answers in plain text, and never proposes or runs actions.

```bash
curl -b cookies.txt -X POST -H "X-CSRF-Token: $csrf" -H 'Content-Type: application/json' \
  -d '{"question":"Did the restart help?"}' \
  https://domovoi.example.net/api/v1/incidents/$ID/ask
```

- The question, trimmed, must be 1 to 1000 characters: otherwise `400`.
- `200 {"answer": "…"}`. `404` when the id is not a stored incident (a malformed one, or a well-formed one nobody saved), `503` when triage is off or the model could not answer.
- The question and its answer are remembered for that incident and shown to the model with the next question. Each question sees the conversation as it was when the question started.

`GET /api/v1/incidents/{id}/followups` answers `{"turns": [{"question", "answer", "asked_by", "at"}]}`, oldest first: what the model still sees. `at` is when the answer was recorded, and `asked_by` is `user:` and the id of the person who asked. Turns written before the last clear are left out, so the list is empty after one. `404` for an id that is not a stored incident, `503` when triage is off.

`POST /api/v1/incidents/{id}/followups/clear` forgets the conversation: the next question is shown none of the earlier ones. It answers `204`, or `404` for an id that is not a stored incident, and works whether triage is on or off. A clear also wins over a question that was still being answered: that answer is never remembered, and the next question does not see it.

## Settings

`GET /api/v1/settings` returns, under `editable`:

- the merged settings as `values`, shaped like `hub.toml`, with every secret shown as `"[set]"`;
- the `revision` a save must name;
- `fields`, mapping each field some layer set to `ui`, `file` or `env:<VAR>`.

`GET /api/v1/settings/schema` is the JSON schema with every field's description and default.

`PATCH /api/v1/settings` saves a change and restarts the hub with it:

```json
{
  "base_revision": 3,
  "changes": { "notify": { "digest": { "time": "08:00" } }, "discord": { "bot_token": { "set": "…" } } },
  "password": "…"
}
```

- `null` removes a value, so the lower layers apply.
- A secret takes `{ "keep": true }`, `{ "set": "…" }` or `{ "clear": true }`. No response ever contains a secret.
- The password is needed when the change touches `discord.allowed_user_ids`, `web.trusted_proxies`, `webhooks` or any secret. Wrong passwords count against the login limit.
- Answers:
  - `200 { revision, applied }`.
  - `400` with an `issues` list of `{ path, message, severity }`.
  - `403` when the password is missing or wrong.
  - `409` for a stale revision, or a field set in `hub.toml` or the environment.

`POST /api/v1/settings/test/{discord,llm,heartbeat}` takes `{ changes }` in the same shape and tries the connection with those candidate values without saving them. A Discord test whose changes list no `discord.allowed_user_ids` is tried with a placeholder approver, so Test in the first run works before you choose approvers. A secret left as `{ "keep": true }` uses the stored one. Each session may run 10 tests a minute, and each test times out after 15 seconds.

`GET` and `PUT /api/v1/hosts/{name}/checks` read and replace a host's check overrides. Each override has `enabled`, `interval_seconds` and `thresholds`. A saved change goes to the connected agent at once, or when it reconnects.

## Importing notes

`POST /api/v1/notes/import` takes a folder of Markdown runbooks as `multipart/form-data`: one `files` field per file, and an optional `host` field naming the host they are about.

```bash
curl -b cookies.txt -H "X-CSRF-Token: $csrf" \
  -F host=nas -F files=@docs/zfs.md -F files=@docs/backups.md \
  https://domovoi.example.net/api/v1/notes/import
```

- At most 200 files per request; more is refused with `400` and nothing is imported.
- Each note's title is the file's first `# ` heading, or else its name without `.md`.
- A file is skipped, with the reason in `skipped_files`, when it is not `.md`, is over 256 KiB, is not UTF-8, is empty, or a note with the same title already exists for that host. Importing the same folder twice adds nothing.

The reply counts what happened: `{"created": 2, "skipped": 0, "skipped_files": []}`. Notes may be up to 256 KiB, whether imported or written in the UI.

## Past operations

Every request needs a logged-in session, and the `POST` also needs the CSRF header. Every body that lists past operations carries `"label": "historical evidence, not authorization"`. See [Experience](../concepts/experience.md).

- `GET /api/v1/operations/{id}/summary` returns the current revision of each summary field with its source, the revision history and a `state` of `not_ready`, `drafting` or `ready`.
- `POST /api/v1/operations/{id}/summary/{field}` with `{"value": "...", "expected_revision": 1}` writes or confirms `problem`, `cause` or `fix`. A code-owned field answers 403, an unfinished operation or a stale revision 409, and an empty or too long text 400.
- `GET /api/v1/operations/{id}/similar` and `GET /api/v1/incidents/{id}/similar-operations` list similar past operations from the same host, with `limit` from 1 to 20.
- `GET /api/v1/experience/search` takes `q`, `scope` (`host`, the default, with `host` naming the host, or `global`) and `limit` up to 50. A global search takes no host.

The operation list and detail rows carry a `resolution` object (`outcome`, `verified`, `ended_at`), `null` until the summary exists.

## Knowledge

The knowledge store is under `/api/v1/knowledge`. Every request needs a logged-in session. Every `POST`, `PUT` and `DELETE` also needs the CSRF header. See [Knowledge](../concepts/knowledge.md) for what the fields mean.

- `GET /facts` lists facts. Filters: `host`, `subject`, `predicate`, `source`, `status`, `q` (full-text search), `limit` and `cursor`.
- `GET /facts/{id}` returns one fact.
- `POST /facts` asserts a fact. The source is always forced to `user`, whatever the request says.
- `POST /facts/{id}/accept` turns an inferred fact into a `user` fact.
- `POST /facts/{id}/retract` removes a fact from search and from model context. It never needs the password.
- `POST /facts/{id}/verify` re-verifies a fact.
- `POST /facts/{id}/confirm` confirms a management path.
- `GET /hosts/{name}/management-paths` lists a host's management paths.
- `GET /relations` and `POST /relations` list and create relations. `POST /relations/{id}/retract` retracts one.
- `GET /entities` and `POST /entities` list and create entities. `PUT /entities/{id}` and `DELETE /entities/{id}` change and remove one.

Step-up rule: `POST /facts`, `accept` and `verify` on `management_path` or `trusted_cidr` need the admin password. Every `confirm` needs it too. Retract never does.

Confirmation is not available yet. `confirm` fails with the message `the helper's coverage cannot be verified yet` until the helper attestation feature ships, because a confirmation needs a verified, fresh attestation that shows the path covered.

## Events

`GET /api/v1/events` streams committed journal rows as server-sent events. It needs a session and serves at most 32 streams at once (`503` beyond that).

- `topics`: comma-separated, at most 16. Default `incidents,approvals,operations`. Also `operation:<id>`; `hosts` and `conversations:<id>` are accepted and deliver nothing yet. An unknown topic is `400`.
- `after` or the `Last-Event-ID` header (which wins): resume after this journal position. Without either, the stream starts at the head.

Frames:

- `ready`: `{"head_seq": 4821, "resumed": false, "topics": ["approvals", "incidents", "operations"]}`. Sent first. Only a fresh connection carries `id: <head_seq>`.
- `journal`: one row, with `id` equal to its `seq`.

  ```json
  {"seq": 4821, "aggregate_type": "operation", "aggregate_id": "0190a0a0-0000-7000-8000-0000000000a1",
   "kind": "operation.step_finished", "payload_v": 1, "payload": {"outcome": "succeeded"},
   "actor": "system:operations", "at": "2026-11-18T10:00:00Z",
   "topics": ["operations", "operation:0190a0a0-0000-7000-8000-0000000000a1"]}
  ```

  `kind` and `aggregate_type` are open strings: show an unknown one generically. Incident timeline rows carry only `kind` and a redacted `summary`; fetch the incident for the rest. A payload over 16 KiB is sent as `null` with `"payload_truncated": true`.
- `reset`: `{"reason": "pruned" | "ahead" | "lagged", "head_seq": 4821}`, with `id: <head_seq>`. What you hold may be incomplete, so refetch it. `pruned`: the position is older than the journal keeps. `ahead`: it is newer than the journal (a restored backup). `lagged`: the client fell more than 5,000 rows behind. The stream then continues from the head.
- `: keepalive`, a comment sent after 15 seconds of silence.

Incident lists, an incident, action lists and the overview each return `as_of_seq`, the journal position they were read at. Apply only events with a greater `seq`.

## Logs

`GET /api/v1/system/logs` answers the hub's own recent log lines, oldest first, as a JSON array of `{"at", "level", "target", "message"}`. `lines` is how many to return (1 to 2000, default 200) and `level` is the least severe level to include (`error`, `warn`, `info`, `debug`, `trace`; default `info`). Only the newest 2000 lines are kept in memory, and every message was scrubbed while the hub captured it, so replacing a secret later cannot make an older line show the value it replaced. When the process keeps no lines, the endpoint answers 503 with `logs are not captured`.

## Health and metrics

- `GET /healthz` answers `ok` and needs no login: the process is serving requests.
- `GET /readyz` answers whether the hub can do its job: the store takes a write, every migration is applied, and the incident manager and the action service are running. It needs no login, and reveals nothing but check names.

  ```json
  {"ready": true, "failing": []}
  ```

  When something is wrong it answers `503` and names only the checks that failed, in a fixed order: `store_writable`, `migrations`, `incident_manager`, `action_service`. Every check runs even after an earlier one fails.

  ```json
  {"ready": false, "failing": ["store_writable", "action_service"]}
  ```

  Use `/readyz` to gate traffic and `/healthz` to decide whether to restart the process. Details of a failure go to the hub's log, never to the response.
- `GET /metrics` serves Prometheus text to a logged-in session, or to a request carrying `Authorization: Bearer <token>` when `[metrics] token` is set. Without either it answers `401` with no metrics text. See the [CLI reference](cli.md#metrics) for a scrape configuration.
- Bodies are limited to 1 MiB, and requests other than the event stream to 120 seconds. The retry-triage endpoint runs a full triage inside its response, so a local model needs the request to outlive the per-run triage budget (default 90 seconds).
- Every response carries a strict `Content-Security-Policy` and the other security headers. The policy allows scripts from the hub itself plus the one inline theme script by its SHA-256 hash, computed from the built page.
