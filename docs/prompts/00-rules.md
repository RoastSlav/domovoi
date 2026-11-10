# Rules for every task (paste this before each prompt)

You are working in the Domovoi repository: a Rust workspace (`crates/*`) and a React +
TypeScript web UI (`web/`). One binary has two roles: **hub** (web UI, API, triage, chat)
and **agent** (runs on each watched host, runs checks and skills).

## Crate map (do not add other internal dependencies)

- `domovoi-core`: domain types and port traits. Pure: no I/O, no tokio.
- `domovoi-proto`: hub ↔ agent wire messages. Pure.
- `domovoi-engine`: incident manager, triage, notifications. Uses tokio only for tasks,
  channels and timers. Time only through the `Clock` port (`clock.now()`,
  `clock.sleep_until(t)`), never `tokio::time::sleep`.
- `domovoi-store` (SQLite), `domovoi-llm` (model adapters), `domovoi-chat` (Discord),
  `domovoi-checks` (checks), `domovoi-runner` (the only crate that spawns processes):
  each depends on `core` only.
- `domovoi-agent`: agent runtime. `domovoi-hub`: hub runtime, HTTP API, settings.
- `domovoi`: the binary and CLI (composition root). `domovoi-testkit`: fakes, dev-only.

## Hard rules (CI fails otherwise)

1. A non-test `.rs` file stays at or under 500 lines. If a file would grow past that, put
   the new code in a new sibling module.
2. No `unwrap()`, `expect()`, `panic!`, `todo!`, `unimplemented!` or `dbg!` outside tests.
   Use `?`, `thiserror` enums, `unwrap_or*`, `.ok()`.
3. Every `pub` item has a `///` doc comment. New modules start with a `//!` line.
4. Never construct `std::process::Command` or `tokio::process::Command`, never `sh -c`.
5. The model (LLM) never approves or runs anything. Actions come only from an
   `ActionProposal { skill, args }` validated against a skill manifest, and only a human
   approves them.
6. Files in `crates/domovoi-store/migrations/` are append-only. Add a new numbered file;
   never edit an existing one.
7. Add no new crate or npm dependency unless the task says so. Versions go in the root
   `Cargo.toml` `[workspace.dependencies]`.
8. Web: never call `fetch` outside `web/src/api`. Use `api` and `call()` from
   `web/src/api/client.ts` / `types.ts` in a hook under `web/src/api/hooks/`. Query keys
   live in `web/src/api/queryClient.ts`.
9. API change: annotate the handler with `#[utoipa::path(...)]`, register it in
   `routes()` in `crates/domovoi-hub/src/api/mod.rs`, then run:
   `cargo xtask openapi`, `cd web && npm run gen:api`, and
   `INSTA_UPDATE=always cargo test -p domovoi-hub --test hub api_routes` (updates the
   OpenAPI snapshot). Commit the regenerated files.
10. Protocol change (`crates/domovoi-proto`): additive only (new optional fields with
    `#[serde(default)]`, new message kinds). Add a sample to
    `crates/domovoi-proto/tests/protocol/samples.rs` and update snapshots with
    `INSTA_UPDATE=always cargo test -p domovoi-proto`. Bump the minor of
    `PROTOCOL_VERSION` in `crates/domovoi-proto/src/version.rs` by one (it is `1.1` on
    `main` today), unless a change since the last release tag already bumped it
    (`git diff <last tag> -- crates/domovoi-proto/src/version.rs`). Update
    `docs/book/src/reference/protocol.md` with the new kind and version.
11. Changing a rule or invariant in `ARCHITECTURE.md` needs a new ADR in `docs/adr/`
    (copy `0000-template.md`, next free number; `0018` is the last one on `main`, and
    prompts 25 and 31 plan `0019` and `0020`: check `docs/adr/` before picking).
12. Config change (`crates/domovoi-config`): the reference page is generated. Run
    `cargo xtask docs-config` and commit `docs/book/src/reference/configuration.md`.
    Book pages you edit must keep `cargo xtask docs-links` passing.
13. Tests use fakes from `domovoi-testkit` and the fake clock (`clock.advance(..)`);
    never sleep in tests.
14. Web UI (once the `ui/` series is merged): read `web/DESIGN.md` before changing
    `web/`. Build pages from the kit in `web/src/components/` (`PageHeader`,
    `QueryBoundary`, `DataTable`, `Field`, `StatusPill`, `Tooltip`, `InfoTip`,
    `ConfirmDialog`, …); colours only through tokens in `web/src/styles/tokens.css`;
    every status's label, tone and hint only from `web/src/design/tone.ts`; every
    mutation reports its result through `useActionFeedback` or an inline message, with
    error text from `describeError`; every disabled control has a `disabledReason`;
    every page has loading, empty, error and offline states and works at 375 px in
    both themes. Add new components and states to `/dev/design`. Do not invent new
    styled primitives in feature code: extend the kit and `DESIGN.md` instead.

## Before you finish, run

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <each crate you changed>
cargo xtask arch-check
```

If you changed `web/`:

```sh
cd web && npm run lint && npm run typecheck && npm test && npm run build
```

`npm run build` includes the bundle size check once the `ui/` series added it.

## How to work

- Read only the files the task lists, plus what they directly call if you must.
- Make the smallest change that meets **Done when**. Do not refactor unrelated code.
- Match the surrounding style: short plain-English doc comments, tests named after the
  behaviour (`resolved_incidents_show_a_resolved_badge`).
- User-visible behaviour changed? Update the matching page in `docs/book/src/`.
- Files near the 500-line limit on `main` (plan around them, or do prompt 39 first):
  `engine/src/manager/decide.rs` (500), `hub/src/api/dto.rs` (490),
  `hub/src/api/settings_write.rs` (486), `hub/src/runtime/mod.rs` (483),
  `hub/src/api/incidents.rs` (474), `hub/src/settings/service.rs` (472),
  `config/src/hub/mod.rs` (467), `engine/src/actions/executor.rs` (454).
- Work on a branch named after the change (`action-writer`), never after the prompt.
- Commit messages are one line: the Conventional Commit subject only, no body, no
  trailers, for example `fix(web): ...` or `feat(engine): ...`. PR titles have at most
  one scope.
- No committed file, commit or PR mentions prompts, prompt numbers, the build plan or AI
  assistance. Describe the change itself.
