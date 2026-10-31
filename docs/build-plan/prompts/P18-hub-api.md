# P18 — Hub HTTP API, authentication and OpenAPI

| | |
| --- | --- |
| Phase | 5, Surfaces |
| Depends on | P10, P17 |
| Crates | `domovoi-hub`, `domovoi` (hidden `openapi` subcommand), `xtask` (`openapi` task), `web/src/api` (generated files), CI |
| Size | L |
| Invariants touched | INV-1, INV-5, INV-6, INV-8 |

## Goal

Expose the hub over a JSON API with SSE: single-admin authentication with sessions and CSRF, endpoints for incidents, actions, hosts and enrollment, skills, notes, settings and digest, security headers and limits, and an OpenAPI document that generates the TypeScript types for the web UI. This is where web approvals create `HumanApproval`.

## Read first

- `ARCHITECTURE.md` §10, §15
- ADR 0006, ADR 0007
- P13 functions (tokens, revoke), P14 webhook routes, P17 `ApprovalService`

## In scope

Add `axum`, `tower`, `tower-http` (`trace`, `set-header`, `limit`, `timeout`, `compression-gzip`), `utoipa`, `utoipa-axum`, `argon2`, `rand`, `subtle`, `serde`, `serde_json`, `metrics`.

### Auth (`crates/domovoi-hub/src/auth/`)

- **Users:** one admin account in v0.1. Password hashed with argon2id (m = 19 MiB, t = 2, p = 1). No default password.
- **First run:** if no user exists, generate a one-time setup token (32 bytes), log `Open http(s)://<public_url>/setup?token=...` once, and accept `POST /api/v1/auth/setup { token, username, password }` (password ≥ 12 chars). The token is single use and expires after 24 h.
- **Sessions** (`session.rs`, no third-party session crate): random 32-byte ID in cookie `domovoi_session` (`HttpOnly`, `SameSite=Strict`, `Path=/`, `Secure` when `public_url` is HTTPS), SHA-256 of the ID stored with user, created, last seen, expiry (idle 12 h, absolute 7 d) and a per-session CSRF token. Logout deletes the row.
- **CSRF** (`csrf.rs`): `GET /api/v1/session` returns `{ user, csrf_token }`. Every `POST`, `PUT`, `PATCH` and `DELETE` under `/api/v1` requires header `X-CSRF-Token` equal to the session's token (constant time) **and**, when an `Origin` header is present, it must match `public_url`. Failure → 403. (INV-8)
- **Login** `POST /api/v1/auth/login`: rate-limited per IP (5/min, then exponential lockout up to 15 min); generic error messages.
- **Approvals** (`approval.rs`): the web approve handler builds `HumanApproval::new(user, ApprovalChannel::Web { typed_confirmation }, now)` only after the session and CSRF checks pass; `typed_confirmation` is true only if the request body's `confirm_text` equals the skill name exactly. This file and P21's interaction authorizer are the only places allowed to call `HumanApproval::new` (INV-1).
- Trusted proxies: honour `X-Forwarded-For`/`X-Forwarded-Proto` only from `[web] trusted_proxies`.

### Endpoints (`api/`)

DTOs live in `api/dto.rs` (core types are mapped, never exposed directly). All list endpoints use cursor pagination.

| Method and path | Purpose |
| --- | --- |
| `GET /api/v1/session` | session info and CSRF token (401 if not logged in) |
| `POST /api/v1/auth/{login,logout,setup}` | auth |
| `GET /api/v1/incidents?status&host&severity&cursor` | list |
| `GET /api/v1/incidents/{id}` | detail: incident, latest report, card, timeline, related actions |
| `POST /api/v1/incidents/{id}/{ack,mute,unmute}` | lifecycle via the incident manager |
| `POST /api/v1/incidents/{id}/ask` | follow-up question (P16) |
| `GET /api/v1/actions?status&cursor` | list |
| `POST /api/v1/actions/{id}/approve` `{ confirm_text? }` | approve (INV-1, INV-6) |
| `POST /api/v1/actions/{id}/deny` | deny |
| `GET /api/v1/hosts` | hosts with presence, version, last seen |
| `POST /api/v1/hosts/{name}/enrollment` | create enrollment invite (P13) |
| `POST /api/v1/hosts/{name}/revoke` | revoke agent |
| `GET /api/v1/hosts/{name}/skills` | skill catalogue |
| `GET/POST/PUT/DELETE /api/v1/notes[/{id}]`, `GET /api/v1/notes/search?q` | notes |
| `GET /api/v1/settings` | effective config with every secret replaced by `"[set]"` or `"[unset]"`, LLM usage this month, heartbeat status |
| `GET /api/v1/advisories` | active advisories |
| `GET /api/v1/digest/preview`, `POST /api/v1/digest/send` | digest |
| `GET /api/v1/events` | SSE stream of UI events derived from `DomainEvent` (incident and action changes), keep-alive every 15 s, requires a session |
| `GET /healthz` | `ok` (no auth) |
| `GET /metrics` | Prometheus text (P23 fills metrics; here return what exists) |

### Middleware

- Security headers on every response: `Content-Security-Policy: default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`, `Permissions-Policy: interest-cohort=()`.
- Body limit 1 MiB (webhooks keep their own limit), request timeout 30 s (not for SSE), gzip compression, `tower-http` trace with secrets and cookies redacted.

### OpenAPI

- Annotate handlers and DTOs with `utoipa`; build the router with `utoipa-axum` so the spec cannot drift from the routes.
- Hidden CLI subcommand `domovoi openapi` prints the JSON spec (add to `crates/domovoi/src/main.rs`).
- `cargo xtask openapi` runs it and writes `web/src/api/openapi.json`; `--check` fails if the file differs.
- `web`: `npm run gen:api` regenerates `src/api/schema.d.ts`. Commit both files.
- CI (`.github/workflows/ci.yml`): add steps that run `cargo xtask openapi --check` and, in the web job, `npm run gen:api && git diff --exit-code src/api`.

## Out of scope

- The web UI itself (P19, P20). Discord approvals (P21). Multi-user and roles.

## Tests required

`tower::ServiceExt::oneshot` against the router with a temp store and fakes:

1. `setup_token_is_single_use_and_creates_admin`; `setup_is_closed_once_a_user_exists`.
2. `login_success_sets_hardened_cookie`; `login_rate_limit_locks_out`.
3. `unauthenticated_requests_get_401`.
4. Gates (`crates/domovoi-hub/tests/gates.rs`): `unsafe_methods_without_csrf_are_403` (every mutating route, generated from the router), `cross_origin_mutation_is_403` (INV-8), `destructive_approve_requires_exact_confirm_text` (INV-6), `settings_never_return_secret_values` (INV-5).
5. `approve_and_deny_flow_through_approval_service`.
6. `incident_list_pagination_and_filters`.
7. `sse_stream_emits_incident_update`.
8. `security_headers_present_on_api_and_healthz`.
9. Insta snapshot of the OpenAPI JSON (`openapi_snapshot`).
10. `xtask openapi --check` passes in CI.

## Architecture gates

- `HumanApproval::new` only in `crates/domovoi-hub/src/auth/` (arch-check).
- Generated files committed and fresh (R12).

## Acceptance criteria

- `cargo xtask ci` passes; CI includes the OpenAPI freshness checks.
- `docs/book/src/reference/api.md` links to the spec and documents auth and CSRF for API clients.

## Deliverables

- `crates/domovoi-hub/src/{auth/*,api/*,middleware/*,router.rs}`
- `crates/domovoi/src/main.rs` (`openapi` subcommand), `xtask/src/openapi.rs`
- `web/src/api/openapi.json`, `web/src/api/schema.d.ts`, CI updates, docs

## PR

Title: `feat(hub): JSON API with sessions, CSRF, SSE and generated OpenAPI`
