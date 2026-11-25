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

## Operation history

`GET /api/v1/operations/{id}/history` answers how an operation unfolded, for the replay in the graph view. It needs a logged-in session. The answer has `as_of_seq` (the event-stream position it was read at), `complete`, `incomplete_reason` and `events`, oldest first. Each event is `{"seq", "at", "kind", "step", "state"}`: the journal row's time, its kind, the step it names (or `null`) and the new state it records (an execution state or an operation status, otherwise `null`). It never carries intent, summary or any free text. `incomplete_reason` is `null` when the history starts at the operation's creation and holds every row up to `as_of_seq`; otherwise it is `pruned` (the oldest rows were removed), `capped` (there are more than 500 rows, so the newest are missing) or `both`. An unknown operation answers 404, and while operations are off the endpoint answers 503 "orchestration is not enabled".

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

## Overview

`GET /api/v1/overview` returns the counts the web UI shows in its navigation and on the Control room. Pass `host=<name>` to count for one host; without it every host is counted. The fields are:

- `incidents_action_needed` and `incidents_open`, with `incident_groups` (`needs_action`, `monitor`, `pending`, `expected` and `handled`);
- `approvals_waiting`: classic actions waiting for a decision;
- `operations_running`, `operations_waiting_approval`, `operations_waiting_user` and `operations_paused`;
- `actions_outcome_unknown`: classic actions whose result is unknown;
- `hosts_offline` and `hosts_total`;
- `attention_total`: `incident_groups.needs_action` + `approvals_waiting` + `operations_waiting_approval` + `operations_waiting_user` + `operations_paused` + `actions_outcome_unknown` + `hosts_offline`;
- `as_of_seq`: the journal position the counts were read at.

See [Control room, attention and search](../guide/control-room.md) for how the pages use them.

## Hosts

`GET /api/v1/hosts` lists every host with its agent's presence, whether the agent is older than the hub (`outdated`), the protocol state (`compatible`, `incompatible`, `expected_incompatible` or `unknown`) with the reported and hub protocol versions, and how many of its incidents are not yet resolved (`open_incidents`). `GET /api/v1/hosts/{name}` adds the latest value of each digest fact the host reported in the last 24 hours.

`GET /api/v1/hosts/{name}/capabilities` returns the host's capability inventory: one row per contract the hub knows, with its state (`available`, `not_installed`, `disabled_by_hub`, `not_allowed_on_host`, `unavailable`, `host_unknown`, `host_absent` or `host_incompatible`), the reason code of a refusal, how much of the host's implementation list was read (`complete`, `partial` or `stale`), the effect class, the target kind, the execution path, and whether the agent runs as root or reported an isolated skill runner. `unknown_contracts` lists ids the agent reported that no contract describes. The endpoint is read-only: it enables, disables and approves nothing.

`GET /api/v1/hosts/{name}/ceiling` returns what the host's agent reported as its unattended limits: `reporting` is `reported`, `not_reported` or `unreadable`, and only a reported answer carries `unattended_enabled`, `hash` and `entries` (per capability: targets, `max_per_hour`, `max_active`, `max_ttl_secs`, argument bounds), sorted by contract. `connected` says whether the host has a live agent. The endpoint is read-only: the host owns these limits and the hub cannot change them.

`GET /api/v1/hosts/{name}/helper` returns what the hub knows about a host's helper: its key, the owner's pin and the verified attestation. `POST /api/v1/hosts/{name}/helper/recheck` asks the helper again. `PUT` and `DELETE` on `/api/v1/hosts/{name}/helper/key` pin and unpin its key and need the account password again. See [Helper trust](hub.md#helper-trust).

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

## Conversations

Operator conversations are read-only questions about the hosts. See [Operator](../concepts/operator.md). Every route needs a logged-in session, and every `POST`, `PUT` and `DELETE` also needs the CSRF header. `POST /api/v1/chat` is removed; use these routes instead.

- `GET /api/v1/conversations` lists conversations, newest first, as `{"items": […], "next_cursor": "…", "as_of_seq": 0}`. Filters: `host` (a host name), `operation` (an operation id), `cursor` and `limit`. Each item has `id`, `title` (the first question, empty before one is asked), `scope` (`{"kind": "global"}` or `{"kind": "host", "host_id", "host_name"}`), `operation_id`, `message_count`, `created_at`, `updated_at` and `version`.
- `POST /api/v1/conversations` starts one and answers `201` with the conversation. The body is `{"scope": {"kind": "global"}}` or `{"scope": {"kind": "host", "host": "nas"}}`, with an optional `operation_id`. `scope` is required: there is no implicit "all hosts". `400` when it is missing or malformed, `404` for an unknown host or operation.
- `GET /api/v1/conversations/{id}` returns the summary fields with `messages` (oldest first), `turn_in_progress` (`{"turn", "message"}` while an answer is being written, else `null`) and `as_of_seq`.
- `DELETE /api/v1/conversations/{id}` answers `204`. `409` while an answer is being written: stop it first.
- `PUT /api/v1/conversations/{id}/operation` with `{"operation_id": "<uuid>"}` attaches an operation, or `{"operation_id": null}` detaches it. Answers the updated summary.
- `POST /api/v1/conversations/{id}/messages` with `{"turn": "<uuid>", "text": "…"}` asks a question. `turn` is a client-generated uuid, and sending the same turn again changes nothing. The question is 1 to 1000 characters. A new turn answers `202` with `{"user", "assistant", "as_of_seq"}`: the committed question and the pending answer. A turn that was already sent answers `200` with the stored pair, whatever state the answer is in. `409` while another answer is being written or the conversation is full.
- `POST /api/v1/conversations/{id}/turns/{turn}/cancel` stops an answer that is being written. It answers `200 {"status": "cancelled"}` when this call stopped it; the status is otherwise the answer's current state.
- `POST /api/v1/conversations/{id}/messages/{message}/retry` asks the same question again and answers `202` like a new turn. Only the last answer can be retried, and only after it failed or was stopped: `409` otherwise.

A message is `{"id", "ordinal", "role", "status", "turn", "text", "failure", "failure_detail", "tools", "created_at", "completed_at"}`. `role` is `user` or `assistant`; `status` is `pending`, `complete`, `failed` or `cancelled`. `text` is empty while the answer is pending, failed or cancelled. `tools` lists the read tools the model used, as `{"tool", "failed"}`.

## Settings

`GET /api/v1/settings` returns, under `editable`:

- the merged settings as `values`, shaped like `hub.toml`, with every secret shown as `"[set]"`;
- the `revision` a save must name;
- `fields`, mapping each field some layer set to `ui`, `file` or `env:<VAR>`.

`GET /api/v1/settings/schema` is the JSON schema with every field's description and default.

The settings answer also includes a `retention` block: the nine periods, each with `days`, `default_days`, `applies` (`now` or `later`) and `applies_once`, plus `backups_keep` and `backups_keep_pre_migrate`.

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

## Audit log

Both routes only read. See [Audit log](../guide/audit.md) for what the check can and cannot show.

`GET /api/v1/audit` lists entries, newest first.

- `history` is `legacy` or `chained`; leave it out for both.
- `kind` filters on the entry kind.
- `cursor` is a `next_cursor` from an earlier answer.
- `limit` defaults to 50 and is at most 200.

```json
{ "items": [{ "seq": 1515, "at": "2026-01-01T09:00:00Z", "actor": "admin", "kind": "setting.changed", "subject": "notify", "detail": "{…}", "detail_truncated": false, "history": "chained", "hash": "9f0e…" }], "legacy_last_id": 1204, "next_cursor": null }
```

`detail` is redacted compact JSON text, cut at 4096 bytes, and `detail_truncated` says whether it was cut. `hash` is 64 hex characters, or `null` for a legacy entry. `legacy_last_id` is the last legacy row's id. `400` with ``unknown history `x`; use legacy or chained`` or `kind is too long`.

`GET /api/v1/audit/verification?scope=recent|full` checks the log. `recent` checks the newest 10,000 chained entries and compares the legacy row count only. `full` also recomputes the legacy digest.

```json
{
  "scope": "recent", "recent_limit": 10000, "checked_at": "2026-01-01T09:00:00Z",
  "legacy": { "status": "count_only", "count": 1204, "expected_count": 1204, "actual_count": 1204, "digest_matches": null },
  "chained": { "status": "intact", "checked": 311, "head_seq": 1515, "head_hash": "9f0e…", "first_broken_seq": null, "break_reason": null, "break_text": null },
  "anchor": { "install_id": "…", "legacy_last_id": 1204, "legacy_count": 1204, "legacy_digest": "…", "anchor_hash": "…" },
  "head": { "seq": 1515, "hash": "9f0e…" }
}
```

- `legacy.status` is `verified`, `count_only`, `mismatch` or `not_verifiable`.
- `chained.status` is `empty`, `intact` or `broken`. When it is broken, `first_broken_seq`, `break_reason` and `break_text` describe the first failing entry.
- `head` is `null` when there are no chained entries.
- `400` with ``unknown scope `x`; use recent or full``. `500` when the check cannot run; a failed check is never reported as a status.

## Preferences

`GET /api/v1/me/preferences` returns the signed-in person's display preferences. There is no id in the path: it is always the person the session belongs to.

```json
{ "version": 2, "updated_at": "2026-01-01T09:00:00Z", "presentation": { "profile": "standard", "pages": { "operations": "detailed" } } }
```

`PUT /api/v1/me/preferences` saves them and answers the same shape. The body may carry `presentation`; leaving it out changes nothing.

```json
{ "presentation": { "profile": "standard", "pages": { "operations": "detailed" } } }
```

- `profile` is `detailed`, `standard` or `essential`. A person with nothing saved has `detailed`.
- `pages` maps a page key to a profile that overrides `profile` on that page. A key matches `^[a-z][a-z0-9_-]{0,31}$`, and there are at most 32.
- `400 bad_request` with a message for an unknown profile, a bad page key or too many pages.

Preferences change only what the web UI draws. They do not change what a person may do, what is recorded, or what the UI asks the hub for. See [Detail level](../guide/display.md).

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

A confirmation needs a verified, fresh helper attestation that shows the path covered. `confirm` fails with `the helper's coverage cannot be verified yet` when the host's helper is not attested, and with the unverified reason (such as `key_not_pinned` or `attestation_stale`) otherwise. A confirmation also stops being current when the helper's configuration or routes change, or after 30 days. See [Helper trust](hub.md#helper-trust).

## Events

`GET /api/v1/events` streams committed journal rows as server-sent events. It needs a session and serves at most 32 streams at once (`503` beyond that).

- `topics`: comma-separated, at most 16. Default `incidents,approvals,operations`. Also `operation:<id>` and `conversations:<id>`. `hosts` is accepted and delivers nothing yet. An unknown topic is `400`.
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
- Live answer frames, sent only on a stream that follows `conversations:<id>`. They carry no `id` line, are never replayed after a reconnect, may be dropped when a client is slow, and are followed by the committed message as a `journal` row. Each carries the `conversation` id and `turn`:
  - `live.start`: `{"conversation", "turn", "message"}`, when an answer begins. `message` is the pending assistant message.
  - `live.text`: `{"conversation", "turn", "attempt", "text"}`, a snapshot of the safe text written so far for that attempt, not a delta. A later frame with the same attempt replaces the earlier one. The attempt number rises when the model starts again, or when redaction changes text already shown; the text then starts over, and may be empty.
  - `live.tool`: `{"conversation", "turn", "tool", "state", "failed"}`, when a read tool starts (`state` `started`) or finishes (`finished`). `tool` is a read tool name, or `unknown tool`.
  - `live.end`: `{"conversation", "turn", "status"}`, when the answer settles: `complete`, `failed`, `cancelled` or `discarded`.
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
