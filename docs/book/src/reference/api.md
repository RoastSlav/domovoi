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

## Approving steps

`POST /api/v1/operations/{id}/steps/{step}/approve` approves an Execution step as the logged-in user. It requires `{"plan_hash": "<64 hex characters>"}`: the plan hash shown on the step. Without a valid hash the request gets a 400; with a hash that differs from the stored one it gets a 409, and the step stays proposed. A destructive skill, or one whose risk is not known, also needs `{"confirm_text": "<skill name>"}` with the skill's name typed exactly. A containment step also needs `password`, and `unverified_acknowledged: true` when the helper's coverage could not be verified. Chat buttons can never approve a destructive step. `POST .../deny` denies a step and takes no body.

`POST /api/v1/operations/{id}/steps/{step}/resolve-unknown` records that you checked the host after a step with an unknown outcome (`{"note": "...", "password": "..."}`, a note of 1 to 280 characters). `POST .../containment/release` releases a block early (`{"confirm_source": "...", "password": "..."}`).

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
| 400 | `invalid` | a settings change failed validation; the body has an `issues` list |
| 401 | `setup_required` | no account exists yet; open the setup link |
| 401 | `unauthorized` | not logged in |
| 401 | `bad_credentials` | wrong username or password |
| 403 | `forbidden` | CSRF token, origin or confirmation missing or wrong |
| 404 | `not_found` | no such object |
| 409 | `conflict` | not possible in the current state |
| 429 | `too_many_requests` | locked out; see `Retry-After` |
| 500 | `internal` | the hub failed; details go to its log |
| 502 | `delivery_failed` | a message could not be delivered, such as a digest |
| 503 | `unavailable` | a needed part is not configured |
| 503 | `triage_off` | triage is not configured |
| 503 | `budget_spent` | the model budget is used up |

Some routes, such as the package and helper routes, answer with a more specific `error` code. Its HTTP status is always one of those above.

Lists return `{"items": […], "next_cursor": "…"}`, newest first. Pass `cursor=<next_cursor>` for the next page; `limit` is 1 to 200 and defaults to 50.

## Overview

`GET /api/v1/overview` returns the counts the web UI shows in its navigation and on the Control room. Pass `host=<name>` to count for one host; without it every host is counted. The fields are:

- `incidents_action_needed` and `incidents_open`, with `incident_groups` (`needs_action`, `monitor`, `pending`, `expected` and `handled`);
- `operations_running`, `operations_waiting_approval`, `operations_waiting_user` and `operations_paused`;
- `hosts_offline` and `hosts_total`;
- `attention_total`: `incident_groups.needs_action` + `operations_waiting_approval` + `operations_waiting_user` + `operations_paused` + `hosts_offline`;
- `as_of_seq`: the journal position the counts were read at.

See [Control room, attention and search](../guide/control-room.md) for how the pages use them.

## Hosts

`GET /api/v1/hosts` lists every host with its agent's presence, whether the agent is older than the hub (`outdated`), the protocol state (`compatible`, `incompatible`, `expected_incompatible` or `unknown`) with the reported and hub protocol versions, and how many of its incidents are not yet resolved (`open_incidents`). `GET /api/v1/hosts/{name}` adds the latest value of each digest fact the host reported in the last 24 hours.

`GET /api/v1/hosts/{name}/capabilities` returns the host's capability inventory: one row per contract the hub knows, with its state (`available`, `not_installed`, `disabled_by_hub`, `not_allowed_on_host`, `unavailable`, `host_unknown`, `host_absent` or `host_incompatible`), the reason code of a refusal (`agent_in_container` when a capability needs the helper or skill runner and the agent runs in a container), how much of the host's implementation list was read (`complete`, `partial` or `stale`), the effect class, the target kind, the execution path, and whether the agent runs as root or reported an isolated skill runner. `unknown_contracts` lists ids the agent reported that no contract describes. The endpoint is read-only: it enables, disables and approves nothing.

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

Operator conversations are read-only questions about the hosts. See [Operator](../concepts/operator.md). Every route needs a logged-in session, and every `POST`, `PUT` and `DELETE` also needs the CSRF header. Questions to the Operator go through these routes.

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
  - `400` with error `invalid` and an `issues` list of `{ path, message, severity }`.
  - `403` when the password is missing or wrong.
  - `409` for a stale revision, or a field set in `hub.toml` or the environment.

`POST /api/v1/settings/test/{discord,llm,heartbeat}` takes `{ changes }` in the same shape and tries the connection with those candidate values without saving them. A Discord test whose changes list no `discord.allowed_user_ids` is tried with a placeholder approver, so Test in the first run works before you choose approvers. A secret left as `{ "keep": true }` uses the stored one. Each session may run 10 tests a minute, and each test times out after 15 seconds.

`GET` and `PUT /api/v1/hosts/{name}/checks` read and replace a host's check overrides. Each override has `enabled`, `interval_seconds` and `thresholds`. A saved change goes to the connected agent at once, or when it reconnects.

## Audit log

Both routes only read. See [Audit log](../guide/audit.md) for what the check can and cannot show.

`GET /api/v1/audit` lists entries, newest first.

- `kind` filters on the entry kind.
- `cursor` is a `next_cursor` from an earlier answer.
- `limit` defaults to 50 and is at most 200.

```json
{ "items": [{ "seq": 1515, "at": "2026-01-01T09:00:00Z", "actor": "admin", "kind": "setting.changed", "subject": "notify", "detail": "{…}", "detail_truncated": false, "hash": "9f0e…" }], "next_cursor": null }
```

`detail` is redacted compact JSON text, cut at 4096 bytes, and `detail_truncated` says whether it was cut. `hash` is 64 hex characters. `400` with `kind is too long`.

`GET /api/v1/audit/verification?scope=recent|full` checks the log. `recent` checks the newest 10,000 entries. `full` checks every retained entry.

```json
{
  "scope": "recent", "recent_limit": 10000, "checked_at": "2026-01-01T09:00:00Z",
  "chained": { "status": "intact", "checked": 311, "head_seq": 1515, "head_hash": "9f0e…", "first_broken_seq": null, "break_reason": null, "break_text": null },
  "anchor": { "install_id": "…", "anchor_hash": "…" },
  "head": { "seq": 1515, "hash": "9f0e…" }
}
```

- `chained.status` is `empty`, `intact` or `broken`. When it is broken, `first_broken_seq`, `break_reason` and `break_text` describe the first failing entry.
- `head` is `null` when there are no entries.
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

- `topics`: comma-separated, at most 16. Default `incidents,approvals,operations`. Also `operation:<id>` and `conversations:<id>`. `hosts` is accepted and delivers nothing. An unknown topic is `400`.
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

Incident lists, an incident and the overview each return `as_of_seq`, the journal position they were read at. Apply only events with a greater `seq`.

## Logs

`GET /api/v1/system/logs` answers the hub's own recent log lines, oldest first, as a JSON array of `{"at", "level", "target", "message"}`. `lines` is how many to return (1 to 2000, default 200) and `level` is the least severe level to include (`error`, `warn`, `info`, `debug`, `trace`; default `info`). Only the newest 2000 lines are kept in memory, and every message was scrubbed while the hub captured it, so replacing a secret later cannot make an older line show the value it replaced. When the process keeps no lines, the endpoint answers 503 with `logs are not captured`.

## Health and metrics

- `GET /healthz` answers `ok` and needs no login: the process is serving requests.
- `GET /readyz` answers whether the hub can do its job: the store takes a write, every migration is applied, and the incident manager is running. It needs no login, and reveals nothing but check names.

  ```json
  {"ready": true, "failing": []}
  ```

  When something is wrong it answers `503` and names only the checks that failed, in a fixed order: `store_writable`, `migrations`, `incident_manager`. Every check runs even after an earlier one fails.

  ```json
  {"ready": false, "failing": ["store_writable", "incident_manager"]}
  ```

  Use `/readyz` to gate traffic and `/healthz` to decide whether to restart the process. Details of a failure go to the hub's log, never to the response.
- `GET /metrics` serves Prometheus text to a logged-in session, or to a request carrying `Authorization: Bearer <token>` when `[metrics] token` is set. Without either it answers `401` with no metrics text. See the [CLI reference](cli.md#metrics) for a scrape configuration.
- Bodies are limited to 1 MiB, except the package import, which allows `[packages] max_artifact_bytes` plus 64 KiB (at most 256 MiB plus 64 KiB). Requests other than the event stream time out after 120 seconds, with `408`. Two kinds get 15 minutes: the package import, and an incident's `ask` and `triage`, which run a full model conversation inside the response.
- Every response carries a strict `Content-Security-Policy` and the other security headers. The policy allows scripts from the hub itself plus the one inline theme script by its SHA-256 hash, computed from the built page.

## Route index

Every route in the OpenAPI document, by area. The sections above describe the routes whose behaviour needs more than a sentence.

### Audit routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/audit` | Audit entries, newest first, with their hash. |
| GET | `/api/v1/audit/verification` | Checks the audit log's hash chain. |

### Auth routes

| Method | Path | What it does |
| --- | --- | --- |
| POST | `/api/v1/auth/login` | Logs in and sets the session cookie. Five failures a minute from one address lock it out for a while. |
| POST | `/api/v1/auth/logout` | Ends the session and clears the cookie. |
| POST | `/api/v1/auth/setup` | Creates the admin account with the one-time token from the setup link, and logs in. Only works before the first user exists. |
| GET | `/api/v1/session` | The current session and its CSRF token. |

### Containment routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/hosts/{name}/containment` | What a host's blocks look like. Display only, and answered when containment is not available. |
| POST | `/api/v1/hosts/{name}/containment/refresh` | Asks the host's helper for its blocks, gate and drills. Owner-initiated only: nothing polls. |
| POST | `/api/v1/operations/{id}/steps/{step}/containment/release` | Releases a block early: asks for the password again and the source typed out, then removes the block under the plan hash of its approval. |

### Experience routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/experience/search` | Searches finished operations. One host by default; every host only when asked. |
| GET | `/api/v1/incidents/{id}/similar-operations` | Past operations that fixed something like an incident, on its host, best first. |
| GET | `/api/v1/operations/{id}/similar` | Past operations that resemble an operation, on its host, best first. |
| GET | `/api/v1/operations/{id}/summary` | The summary of one operation: each field's current revision with its source, and the whole history. |
| POST | `/api/v1/operations/{id}/summary/{field}` | Writes or confirms the problem, cause or fix of a finished operation's summary. |

### Helper routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/hosts/{name}/helper` | What the hub knows about host `name`'s helper. |
| DELETE | `/api/v1/hosts/{name}/helper/key` | Removes the pin of host `name`'s helper key. Containment is blocked until it is pinned again. Asks for the password again. |
| PUT | `/api/v1/hosts/{name}/helper/key` | Pins the key the hub holds from host `name`'s helper. Asks for the password again and for the key's full fingerprint, typed from `domovoi helper fingerprint` on the host. |
| POST | `/api/v1/hosts/{name}/helper/recheck` | Asks host `name`'s helper for a fresh attestation now. |

### Hosts routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/hosts` | Every host with its agent's presence, sorted by name. |
| GET | `/api/v1/hosts/{name}` | Host `name` with its agent and the latest value of each digest fact from the last day. |
| GET | `/api/v1/hosts/{name}/capabilities` | The capability inventory of host `name`. |
| GET | `/api/v1/hosts/{name}/ceiling` | The unattended limits host `name` reported. Read only. |
| GET | `/api/v1/hosts/{name}/checks` | A host's checks: what its agent reports and what the hub overrides. |
| PUT | `/api/v1/hosts/{name}/checks` | Replaces a host's check overrides and sends them to its agent; no restart. |
| GET | `/api/v1/hosts/{name}/docker` | A host's container runtime: what its agent found and the hub's choice. |
| PUT | `/api/v1/hosts/{name}/docker` | Switches a host's container runtime on or off from the hub, or leaves it to the agent; no restart. Refused when the agent's config fixes the setting or no engine was found. |
| POST | `/api/v1/hosts/{name}/enrollment` | A one-time enrollment invite for host `name`, creating the host if it is new. The token is shown only in this response. |
| GET | `/api/v1/hosts/{name}/logs` | The recent log lines host `name`'s agent keeps, scrubbed again here so the page is safe even if the agent was not. An agent too old to send them is refused with a message telling the person to update it. |
| GET | `/api/v1/hosts/{name}/profile` | What `host`'s machine looks like while it behaves normally; 404 before the hub has collected one. |
| POST | `/api/v1/hosts/{name}/profile` | Collects the profile of `host` again now, and answers with it. 409 when its agent is not connected: there is nobody to ask. |
| POST | `/api/v1/hosts/{name}/revoke` | Revokes the agent of host `name` and disconnects it. |
| GET | `/api/v1/hosts/{name}/skills` | The skills host `name`'s agent reported. |
| PUT | `/api/v1/hosts/{name}/skills/{skill}` | Turns one of a host's skills on or off. The switch is saved in the settings table and takes effect immediately: a disabled skill stays on the host, but the hub never runs it and never offers it to triage. |

### Incidents routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/ack-rules` | Every rule, expired ones included. Rules that never expire come last. |
| POST | `/api/v1/ack-rules` | Creates a rule. A check name that no agent reports, no built-in check has and no incident was raised for is refused with 400 and a message that says so. |
| PATCH | `/api/v1/ack-rules/{id}` | Edits a rule: `host`, `target_pattern`, `note`, `predicates`, `expires_at` (a time, or null for never) or `expires_in_minutes`. A field left out stays; null clears `host`, `target_pattern` and `expires_at`. The check cannot change. The same validation as create applies, and the change is audited as `ack_rule.updated`. |
| DELETE | `/api/v1/ack-rules/{id}` | Deletes a rule. |
| GET | `/api/v1/incidents` | Incidents, newest first, with optional filters. |
| GET | `/api/v1/incidents/{id}` | Everything about one incident: report, card, timeline and actions. |
| POST | `/api/v1/incidents/{id}/ack` | Acknowledges an incident. |
| POST | `/api/v1/incidents/{id}/ask` | Asks the triage model a follow-up question about an incident. It uses read-only tools and never proposes actions. |
| POST | `/api/v1/incidents/{id}/dismiss` | Dismisses an incident as a known condition: closes it now with the verdict `EXPECTED`, and when `ignore_for_minutes` is given, creates the rule that keeps the check quiet (0 means always). The rule is stored before the incident closes, so a check firing in the moment between the two is already silenced, and a dismiss that fails afterwards leaves a rule that a human can see and delete on the Known conditions page. The rule matches the incident's host, check and target; `note` is the reason, so a human can read later what it covers. A target with `*` in it cannot be matched exactly by a rule pattern, so dismissing such an incident with a period is refused. |
| GET | `/api/v1/incidents/{id}/followups` | The questions and answers the model still remembers for an incident, oldest first. Answers recorded before the last clear are not in the list. An id that parses but is not a stored incident gets 404, so a cleared conversation is never read as a mistyped one. |
| POST | `/api/v1/incidents/{id}/followups/clear` | Forgets the conversation: the next question will not be shown the earlier ones. Works whether triage is on or off, and on a resolved incident too; it writes one timeline line and publishes nothing. |
| POST | `/api/v1/incidents/{id}/mute` | Mutes an incident for a while. |
| POST | `/api/v1/incidents/{id}/triage` | Investigates an incident again as an operation, with the same caps, and reports how it went. A report already cached for unchanged evidence is reused unless `fresh` is true. |
| POST | `/api/v1/incidents/{id}/unmute` | Lifts a mute. |

### Knowledge routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/knowledge/entities` | Lists entities. |
| POST | `/api/v1/knowledge/entities` | Creates an entity. |
| DELETE | `/api/v1/knowledge/entities/{id}` | Deletes an entity nothing refers to. |
| PUT | `/api/v1/knowledge/entities/{id}` | Renames an entity. |
| GET | `/api/v1/knowledge/facts` | Lists facts, or searches them when `q` is given. |
| POST | `/api/v1/knowledge/facts` | States a fact as the owner. A safety-critical predicate needs the password. |
| GET | `/api/v1/knowledge/facts/{id}` | One fact. |
| POST | `/api/v1/knowledge/facts/{id}/accept` | Makes a fact the owner's own. A safety-critical predicate needs the password. |
| POST | `/api/v1/knowledge/facts/{id}/confirm` | Confirms a management path against the helper's verified coverage. Needs the password. |
| POST | `/api/v1/knowledge/facts/{id}/retract` | Withdraws a fact. It only removes trust, so it needs no password. |
| POST | `/api/v1/knowledge/facts/{id}/verify` | Says a fact still holds. A safety-critical predicate needs the password. |
| GET | `/api/v1/knowledge/hosts/{name}/management-paths` | A host's management paths. |
| GET | `/api/v1/knowledge/relations` | Lists relations. |
| POST | `/api/v1/knowledge/relations` | States a relation as the owner. |
| POST | `/api/v1/knowledge/relations/{id}/retract` | Withdraws a relation. |

### Me routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/me/preferences` | The signed-in person's display preferences. |
| PUT | `/api/v1/me/preferences` | Saves the signed-in person's display preferences. |

### Notes routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/notes` | Notes about one host, or global notes when no host is given. |
| POST | `/api/v1/notes` | Creates a note. |
| POST | `/api/v1/notes/import` | Imports Markdown files as notes. Each note's title is its first `# ` heading or the file name. Files that are not `.md`, too large, not UTF-8, empty, or titled like an existing note in the same scope are skipped, so importing a folder twice adds nothing. |
| GET | `/api/v1/notes/search` | Full-text search over notes. |
| DELETE | `/api/v1/notes/{id}` | Deletes a note. |
| GET | `/api/v1/notes/{id}` | One note. |
| PUT | `/api/v1/notes/{id}` | Replaces a note. |

### Operations routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/operations` | Operations, newest first. |
| GET | `/api/v1/operations/{id}` | One operation with its trace and executions. |
| POST | `/api/v1/operations/{id}/cancel` | Cancels an operation. Pending steps and effects are cancelled; a run already dispatched is not recalled and its result is still recorded. |
| GET | `/api/v1/operations/{id}/history` | How one operation unfolded, for replaying its graph. |
| POST | `/api/v1/operations/{id}/pause` | Pauses an operation until a person resumes it. |
| POST | `/api/v1/operations/{id}/resume` | Resumes a paused operation, raising the named limits first. |
| GET | `/api/v1/operations/{id}/steps/{step}` | One step with its stored intent and output. |
| POST | `/api/v1/operations/{id}/steps/{step}/answer` | Answers the question an operation is waiting on. |
| POST | `/api/v1/operations/{id}/steps/{step}/approve` | Approves an Execution step. The body names the v2 plan hash shown on the execution; a destructive skill, or one whose risk is not known, needs `confirm_text` equal to its name. |
| POST | `/api/v1/operations/{id}/steps/{step}/deny` | Denies an Execution step. |
| POST | `/api/v1/operations/{id}/steps/{step}/resolve-unknown` | Clears the unattended block of a step whose outcome is unknown. Needs the admin's password. |
| GET | `/api/v1/operations/{id}/trace` | The timeline and graph of one operation. |

### Operator routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/conversations` | Lists conversations, newest first. |
| POST | `/api/v1/conversations` | Starts a conversation. The scope is required: one host or all hosts. |
| DELETE | `/api/v1/conversations/{id}` | Deletes a conversation and its messages. Refused while an answer is being written. |
| GET | `/api/v1/conversations/{id}` | A conversation with all its kept messages. |
| POST | `/api/v1/conversations/{id}/messages` | Asks a question. The question and a pending answer are committed together and the call answers at once; the answer is written in the background and arrives as a journal event, preceded by `live.*` events on the event stream. Sending the same `turn` again returns the stored pair and asks the model nothing. |
| POST | `/api/v1/conversations/{id}/messages/{message}/retry` | Asks the last failed or stopped question again, under a new turn. |
| PUT | `/api/v1/conversations/{id}/operation` | Attaches an operation to a conversation, or detaches it with `null`. The Operator only reads a short summary of it. |
| POST | `/api/v1/conversations/{id}/turns/{turn}/cancel` | Stops the answer of a turn. A turn that already settled is left as it is. |

### Packages routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/hosts/{name}/packages` | The packages the hub last saw installed on host `name`. |
| POST | `/api/v1/hosts/{name}/packages/refresh` | Asks host `name`'s agent which packages it has and replaces what the hub holds with the answer. Needs the agent connected. |
| GET | `/api/v1/packages` | Every imported package. |
| POST | `/api/v1/packages/import` | Imports a signed package: the archive and its detached signature file as one multipart form with the fields `archive` and `signature`. The hub checks the signature file, the archive's digest and the signing key, then stores the archive under its digest. It never opens the archive. No password is asked for: importing installs nothing. |
| GET | `/api/v1/packages/jobs` | Install jobs, newest first. |
| POST | `/api/v1/packages/jobs` | Creates an install or uninstall job with one target per host. Asks for the password again and for the package name typed out, and refuses a plan that changed since the owner looked at it. A job is approved here and nowhere else. |
| POST | `/api/v1/packages/jobs/plan` | What a job would do on each host. Reads only; nothing is created. |
| GET | `/api/v1/packages/jobs/{id}` | One job with every attempt on every host. |
| POST | `/api/v1/packages/jobs/{id}/targets/{host_id}/cancel` | Cancels a host's target that has not started. Once it has started it cannot be cancelled. |
| POST | `/api/v1/packages/jobs/{id}/targets/{host_id}/retry` | Runs one host's target again as a new attempt, after a failure, a refusal or an unknown outcome. Asks for the password and the typed package name like a new job. |
| GET | `/api/v1/packages/status` | What the hub holds, how it is limited and which keys it trusts. |
| DELETE | `/api/v1/packages/{name}/{version}` | Removes an imported package from the hub. Copies already installed on hosts stay. |
| GET | `/api/v1/packages/{name}/{version}` | One imported package with its signature file, the hosts known to have it and whether its archive file is in the store. |

### Policy routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/policy` | The policy at a glance: the active version, the draft and the shadow recorder. |
| GET | `/api/v1/policy/autonomy` | The pause switch and the suspended rules. |
| POST | `/api/v1/policy/autonomy/pause` | Pauses autonomy: no new permit and no dispatch of one already issued. |
| POST | `/api/v1/policy/autonomy/resume` | Resumes autonomy. Needs the admin's password. |
| DELETE | `/api/v1/policy/draft` | Discards the draft. It changes nothing live, so it needs no password. |
| GET | `/api/v1/policy/draft` | The draft with its rules. |
| PUT | `/api/v1/policy/draft` | Creates the draft or replaces it. Changing a locked prohibition needs the password and the preview hash of exactly these rules. |
| POST | `/api/v1/policy/draft/publish` | Publishes the draft as the next version. Always needs the password; the acknowledged keys must be exactly the blocking findings, and the preview hash that of the draft's rules. |
| POST | `/api/v1/policy/preview` | What a candidate rule set would do: validation, the publication review, locked changes and a bounded simulation over recent recorded decisions. It writes nothing. |
| POST | `/api/v1/policy/rules/{rule}/reset` | Resets one rule's circuit breaker. Needs the admin's password. |
| GET | `/api/v1/policy/shadow` | Recorded shadow decisions, newest first. |
| GET | `/api/v1/policy/shadow/{execution_id}` | One recorded decision with its inputs, gaps and both decisions. |
| POST | `/api/v1/policy/simulate` | Replays or re-evaluates recorded Execution steps under a draft, a version or inline rules. It never writes. |
| GET | `/api/v1/policy/versions` | Published versions, newest first. |
| GET | `/api/v1/policy/versions/{number}` | One published version with its rules and acknowledgements. |

### System routes

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/advisories` | Open advisories, newest first. |
| GET | `/api/v1/digest/preview` | Today's digest as it would be sent now. |
| POST | `/api/v1/digest/send` | Sends today's digest now. |
| GET | `/api/v1/events` | Committed journal rows as server-sent events. |
| GET | `/api/v1/overview` | Counts for the navigation: incidents, approvals and offline hosts. |
| GET | `/api/v1/settings` | The effective configuration, secrets hidden. |
| PATCH | `/api/v1/settings` | Saves a change and restarts the hub with it. |
| POST | `/api/v1/settings/models` | Lists models available to the candidate provider of the requested slot. |
| GET | `/api/v1/settings/schema` | The JSON schema of `hub.toml`, for field help and defaults. |
| POST | `/api/v1/settings/test/discord` | Posts "Domovoi test message" with candidate Discord settings. |
| POST | `/api/v1/settings/test/heartbeat` | One GET of the candidate heartbeat URL. |
| POST | `/api/v1/settings/test/llm` | One minimal call to the candidate model provider of the requested slot. |
| GET | `/api/v1/system/logs` | The hub's recent log lines, oldest first, with secrets removed. |
| GET | `/api/v1/system/update` | The last release check, if the hub has made one. |
| POST | `/api/v1/system/update/check` | Checks for a release now, at most once a minute. |
| GET | `/healthz` | Liveness: `ok` while the hub serves requests. |
| GET | `/metrics` | Prometheus metrics: readable by a logged-in session, or by a scraper holding the configured bearer token. Without either, 401. |
| GET | `/readyz` | Readiness: 200 when the hub can do its job, 503 naming what it cannot. |
