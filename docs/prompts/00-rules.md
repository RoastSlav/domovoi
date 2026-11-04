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
    `INSTA_UPDATE=always cargo test -p domovoi-proto`. If `PROTOCOL_VERSION` in
    `crates/domovoi-proto/src/version.rs` is still `1.0`, bump it to `1.1`; if it is
    already `1.1`, leave it.
11. Changing a rule or invariant in `ARCHITECTURE.md` needs a new ADR in `docs/adr/`
    (copy `0000-template.md`, next free number).
12. Config change (`crates/domovoi-config`): the reference page is generated. Run
    `cargo xtask docs-config` and commit `docs/book/src/reference/configuration.md`.
    Book pages you edit must keep `cargo xtask docs-links` passing.
13. Tests use fakes from `domovoi-testkit` and the fake clock (`clock.advance(..)`);
    never sleep in tests.

## Before you finish, run

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <each crate you changed>
cargo xtask arch-check
```

If you changed `web/`:

```sh
cd web && npm run lint && npm run typecheck && npm test
```

## How to work

- Read only the files the task lists, plus what they directly call if you must.
- Make the smallest change that meets **Done when**. Do not refactor unrelated code.
- Match the surrounding style: short plain-English doc comments, tests named after the
  behaviour (`resolved_incidents_show_a_resolved_badge`).
- User-visible behaviour changed? Update the matching page in `docs/book/src/`.
- Commit with a Conventional Commit title, for example `fix(web): ...` or
  `feat(engine): ...`.
