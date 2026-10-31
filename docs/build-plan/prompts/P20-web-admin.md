# P20 — Web UI II: hosts and enrollment, skills, notes, settings, digest

| | |
| --- | --- |
| Phase | 5, Surfaces |
| Depends on | P19 |
| Crates | `web/`, `domovoi-hub` (one new endpoint: notes import) |
| Size | M |
| Invariants touched | INV-5 (settings show no secrets) |

## Goal

Finish the admin screens: host list with agent presence and a guided enrollment flow, the skill catalogue per host, notes with search and bulk import from an existing markdown docs folder, settings and status (usage, heartbeat, Discord, advisories), and the digest preview.

## Read first

- P18 endpoints, P19 structure and components
- `ARCHITECTURE.md` §5.7 (notes, advisories, digest)

## In scope

### Hub addition

- `POST /api/v1/notes/import` accepting `multipart/form-data` with up to 200 `.md` files (≤ 256 KiB each) and an optional host; the title comes from the first `#` heading or the file name. Returns created and skipped counts. Regenerate OpenAPI and TS types (`cargo xtask openapi && npm run gen:api`).

### Pages

- `features/hosts/HostsPage`: table of hosts (name, kind, agent status with last seen, agent version with an "outdated" badge, open incidents). "Add host" opens the enrollment flow: name input → create invite → show the exact command in a copyable code block, the hub fingerprint, and a countdown to expiry; the page polls presence and shows "Connected" when the agent completes the handshake. "Revoke" requires confirmation.
- `features/hosts/HostDetailPage`: facts from the latest digest, open incidents, skills.
- `features/skills/SkillsPage`: per host, skills grouped by risk with parameters and `applies_to`; destructive skills visually marked.
- `features/notes/NotesPage`: search (debounced), list, editor with live markdown preview (`Markdown` component, no raw HTML), host selector, delete with confirmation, and an import dialog for a folder of `.md` files (`<input type="file" multiple webkitdirectory>` with fallback to multiple files).
- `features/settings/SettingsPage`: read-only effective configuration (secrets shown only as `set` or `unset`), LLM usage this month against the budget, heartbeat status with the last ping time, Discord connection status, triage status, and active advisories with their detail.
- `features/digest/DigestPage`: preview of today's digest as it will be sent, and "Send now".

## Out of scope

- Editing configuration from the UI (later). Users and roles.

## Tests required

1. `enrollment_flow_shows_command_fingerprint_and_countdown_then_connected`.
2. `revoke_requires_confirmation`.
3. `outdated_agent_badge_shown`.
4. `skills_grouped_by_risk_with_destructive_marked`.
5. `notes_search_debounces_and_renders_snippets`.
6. `notes_editor_previews_markdown_without_raw_html` (a `<script>` in the body is rendered as text).
7. `notes_import_uploads_files_and_reports_counts`.
8. `settings_never_render_secret_values` (fixture contains a fake secret-looking string only in a field the API marks as secret → not shown).
9. `digest_preview_and_send_now`.
10. Hub: `notes_import_limits_file_count_and_size` and `notes_import_derives_titles`.

## Architecture gates

- `cargo xtask ci --web` passes; generated API files fresh.

## Acceptance criteria

- Manual run in the PR: enroll an agent from the UI on a second machine or container and see it connect.

## Deliverables

- `web/src/features/{hosts,skills,notes,settings,digest}/**`
- `crates/domovoi-hub/src/api/notes_import.rs`, regenerated API files

## PR

Title: `feat(web): hosts and enrollment, skills, notes, settings and digest pages`
