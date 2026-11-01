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

`POST /api/v1/actions/{id}/approve` approves a proposed action as the logged-in user. Destructive skills also need `{"confirm_text": "<skill name>"}` with the skill's name typed exactly. Chat buttons can never approve them.

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

`GET /api/v1/hosts` lists every host with its agent's presence, whether the agent is older than the hub (`outdated`), and how many of its incidents are not yet resolved (`open_incidents`). `GET /api/v1/hosts/{name}` adds the latest value of each digest fact the host reported in the last 24 hours.

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

`POST /api/v1/settings/test/{discord,llm,heartbeat}` takes `{ changes }` in the same shape and tries the connection with those candidate values without saving them. A secret left as `{ "keep": true }` uses the stored one. Each session may run 10 tests a minute, and each test times out after 15 seconds.

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

## Events

`GET /api/v1/events` is a server-sent event stream of `incident` and `action` events. Each event's data is a JSON object naming the incident, the action and the new status. The UI reloads what changed. A comment is sent every 15 seconds to keep proxies from closing the stream.

## Health and metrics

- `GET /healthz` answers `ok` and needs no login.
- `GET /metrics` serves Prometheus text.
- Bodies are limited to 1 MiB, and requests other than the event stream to 30 seconds.
- Every response carries a strict `Content-Security-Policy` and the other security headers.
