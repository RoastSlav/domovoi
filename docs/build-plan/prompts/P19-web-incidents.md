# P19 — Web UI I: shell, login, incidents and approvals

| | |
| --- | --- |
| Phase | 5, Surfaces |
| Depends on | P18 |
| Crates | `web/`, `domovoi-hub` (SPA serving behind `embed-ui`) |
| Size | L |
| Invariants touched | INV-6 (typed confirmation UI), INV-8 (CSRF header on every mutation) |

## Goal

Build the React app's foundation and its most important screens: first-run setup, login, the live incident list, incident detail with the triage card, timeline and follow-up questions, and the approvals queue with a typed-confirmation dialog for destructive actions. Embed the built app in the hub.

## Read first

- `ARCHITECTURE.md` §10
- ADR 0006
- `web/README.md`, `web/eslint.config.js` (the `fetch` restriction), the generated `web/src/api/schema.d.ts`

## In scope

Use what the scaffold already has: Vite, React 19, TypeScript, React Router, TanStack Query, `openapi-fetch`, Vitest, Testing Library. Add only what you need and justify each addition in the PR.

### Structure

- `src/api/client.ts`: one `openapi-fetch` client (`credentials: "same-origin"`). Middleware adds `X-CSRF-Token` to every non-GET request from the session query cache; on 401 it redirects to `/login` (or `/setup` when the API says setup is required). No other file calls `fetch` (ESLint enforces).
- `src/api/hooks/*.ts`: typed TanStack Query hooks per resource (`useIncidents`, `useIncident`, `useActions`, `useApprove`, ...).
- `src/api/events.ts`: `useLiveEvents()` opens `EventSource("/api/v1/events")` and invalidates the matching queries; reconnects with backoff; shows a "live" indicator.
- `src/app/`: router, providers, layout (top bar with host status pills and the live indicator; navigation: Incidents, Approvals, Hosts, Notes, Settings; the last three are placeholders until P20).
- `src/components/`: `Badge` (verdict and severity colours matching the cards), `RelativeTime`, `EmptyState`, `ConfirmDialog` (accessible: focus trap, Escape closes, labelled), `Markdown` (no raw HTML).
- `src/features/auth/`: `SetupPage`, `LoginPage`.
- `src/features/incidents/`: `IncidentListPage` (filters for status, host and severity in the URL query; newest first; infinite scroll with the cursor), `IncidentDetailPage` (card at the top as it appears in chat: badge, headline, three bullets, next step; then details, timeline, related actions with approve/deny, an "Ask" box for follow-up questions, Ack and Mute buttons).
- `src/features/approvals/`: `ApprovalsPage` listing proposed actions; Approve for read/write; for destructive actions the dialog requires typing the skill name exactly and sends it as `confirm_text`.
- Styling: CSS modules with CSS variables; light and dark via `prefers-color-scheme`; readable at 360 px width.
- Accessibility: semantic landmarks, visible focus, buttons not divs, colour is never the only signal (badges carry text).

### Hub serving (`domovoi-hub`, feature `embed-ui`)

- `rust-embed` over `web/dist`; serve files with correct MIME types; `index.html` fallback for unknown non-API paths; `Cache-Control: public, max-age=31536000, immutable` for hashed assets under `/assets/`, `no-cache` for `index.html`.
- Without the feature, `/` returns a short HTML page explaining how to build the UI.
- CI: the Rust job builds with `--features embed-ui` after downloading the `web-dist` artifact (update `ci.yml`).

### Dev workflow

- `vite.config.ts` proxies `/api` and `/hooks` to `http://127.0.0.1:7420`. Document `npm run dev` with a running hub in `web/README.md`.

## Out of scope

- Hosts, enrollment, skills, notes, settings pages (P20). Charts.

## Tests required

Vitest + Testing Library; mock the API client module, never the network:

1. `client_adds_csrf_header_to_mutations_only`.
2. `unauthenticated_response_redirects_to_login`.
3. `setup_page_validates_password_length_and_submits`.
4. `login_page_shows_generic_error`.
5. `incident_list_renders_filters_from_url_and_paginates`.
6. `live_event_invalidates_incident_queries`.
7. `incident_detail_shows_card_fields_and_timeline`.
8. `destructive_approval_requires_exact_skill_name` (button disabled until it matches; `confirm_text` sent).
9. `write_approval_uses_simple_confirm`.
10. Hub: `spa_fallback_serves_index_for_client_routes_and_not_for_api` and `hashed_assets_are_immutable` (tests run with a tiny fixture `dist` when the feature is enabled in CI).

## Architecture gates

- ESLint passes, including the `fetch` restriction.
- `npm run typecheck` passes against the generated schema.
- `cargo xtask ci --web` passes.

## Acceptance criteria

- A full manual run is described in the PR: `domovoi hub run` with `embed-ui`, setup, login, a test incident (from `domovoi selftest` once P23 lands, or a webhook now) visible and live-updating.
- Lighthouse accessibility score ≥ 90 on the incident list (state the number in the PR).

## Deliverables

- `web/src/**`, `web/README.md`
- `crates/domovoi-hub/src/spa.rs`, CI update

## PR

Title: `feat(web): app shell, auth, live incidents and approvals`
