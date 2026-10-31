# 0006. React SPA embedded in the hub; OpenAPI-generated client

- Status: Accepted
- Date: 2026-10-31

## Context

The web UI needs a live incident list, a chat view, approvals and settings. The maintainer knows React. The product must stay one binary.

## Decision

- `web/` is a React + TypeScript app built with Vite. Its `dist` folder is embedded in the hub with `rust-embed` behind the `embed-ui` feature, served with an `index.html` fallback and a strict CSP.
- The API is JSON under `/api/v1` with SSE for live updates. Handlers are annotated with `utoipa`; `cargo xtask openapi` writes `web/src/api/openapi.json`; `openapi-typescript` generates `schema.d.ts`; the SPA calls the API only through `openapi-fetch` in `web/src/api`.
- Both generated files are committed. CI regenerates them and fails on a diff.
- Session cookie plus a synchronizer CSRF token sent as `X-CSRF-Token` on every unsafe request.

## Consequences

- CI gains a Node job (lint, typecheck, test, build) and the release build needs the web artifact before compiling the hub.
- Frontend and backend cannot drift silently: a changed endpoint changes the generated types and breaks the TypeScript build.

## Alternatives considered

- htmx with server templates: no Node build, but a weaker fit for the chat UI and the maintainer's skills.
- Separately deployed frontend: breaks the one-binary goal.
