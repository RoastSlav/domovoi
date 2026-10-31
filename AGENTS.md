# Instructions for coding agents

You are working on **Domovoi**, a self-hosted server overseer written in Rust with an embedded React UI. Most of the code is written by coding agents executing the prompts in [`docs/build-plan/prompts`](docs/build-plan/prompts), one prompt per branch and pull request. These rules apply to every session.

## Read before you write

1. [`ARCHITECTURE.md`](ARCHITECTURE.md): the crates, what each may depend on, the domain model, the protocol and the security invariants. It is the contract.
2. The prompt you were given, completely, including **Out of scope**.
3. The ADRs the prompt links to, and any existing code in the crates you will touch.

If the prompt and `ARCHITECTURE.md` disagree, stop and say so. Do not pick one silently.

## Commands

| Task | Command |
| --- | --- |
| Everything CI runs (Rust) | `cargo xtask ci` (add `--web` for the UI, `--fast` to skip deny) |
| Architecture rules only | `cargo xtask arch-check` (CI also runs `--base origin/main`) |
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Tests | `cargo nextest run --workspace` or `cargo test --workspace` |
| Snapshot review | `cargo insta review` (only accept snapshots you have read) |
| Web | `cd web && npm ci && npm run lint && npm run typecheck && npm test && npm run build` |
| Regenerate API types | `cargo xtask openapi && cd web && npm run gen:api` (from prompt P18 on) |

## Hard rules (CI enforces these; do not work around them)

1. **Dependency direction** (R1, R2). Only the internal edges in `ARCHITECTURE.md` §4. `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. If you think you need a new edge, stop and propose an ADR instead.
2. **Processes** (R3, R4). Only `domovoi-runner` spawns processes, with argv vectors. Never `sh -c`. Other crates use the `ProcessExecutor` port.
3. **Approvals** (INV-1, ADR 0007). `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`. The LLM tool catalogue is the closed `ReadTool` enum. Never add a tool, endpoint, chat command or code path that lets anything but an authenticated human approve an action.
4. **Dependencies** (R11). Add versions only to `[workspace.dependencies]` in the root `Cargo.toml`; crates use `{ workspace = true }`. rustls only, never OpenSSL. Prefer crates already in the workspace.
5. **Lints** (R7). Do not add `#[allow(...)]` to silence clippy except where a prompt explicitly says so. No `unwrap`, `expect` (outside tests), `panic!`, `todo!`, `unimplemented!`, `dbg!` in library code.
6. **Size** (R8). Non-test source files stay at or below 500 lines. Split modules by responsibility.
7. **Errors** (R9). `thiserror` enums in libraries; `anyhow` only in `domovoi` and `xtask`.
8. **Docs** (R10). Every public item has a doc comment saying what it is for, not restating its name.
9. **Migrations** (R13). Never edit a merged migration; add a new one.
10. **Generated files** (R12). Never hand-edit `web/src/api/openapi.json` or `web/src/api/schema.d.ts`; regenerate them.
11. **Time.** Anything time-dependent takes the `Clock` port. Tests never sleep; they advance the fake clock from `domovoi-testkit`.
12. **Secrets.** Secret config values are `secrecy::SecretString`. Never log, format into prompts, or return them from the API.

## Tests

- Every behaviour a prompt lists under **Tests required** gets a named test. Name tests after the behaviour (`resolves_incident_after_ttl_without_firing`), not the function.
- Gate tests in any `tests/gates.rs` protect invariants. Never delete or weaken them; extend them.
- Prefer fakes from `domovoi-testkit` over mocks. Add a fake to testkit when a new port appears.
- Snapshot tests (`insta`) for rendered text and wire formats. Review every snapshot change.
- Tests that assert a shell string or an approval is rejected live under `tests/`, because source scans exempt only test paths.

## Workflow for each prompt

1. Create a branch `pNN-short-name`.
2. Plan first: list the files you will create or change and the tests you will write. If anything in the prompt is ambiguous, ask before coding.
3. Implement in small commits using Conventional Commits (`feat(engine): ...`, `fix(runner): ...`, `test(store): ...`, `docs: ...`, `chore: ...`).
4. Run `cargo xtask ci` (and `--web` if you touched `web/`) until it passes. Do not stop with red gates.
5. Update docs the prompt names (book pages, `ARCHITECTURE.md` only if the prompt says so).
6. Write the PR description: what changed, which acceptance criteria are met and how, anything deferred.

## When something does not fit

- A rule blocks a reasonable design: stop and write an ADR proposal in `docs/adr` instead of bending the code.
- A prompt depends on something that does not exist yet: implement the minimal port or stub inside the prompt's scope and say so in the PR, or stop and ask.
- You find a bug outside your scope: note it in the PR description; do not fix it in the same change.

## Style

- Small modules with one responsibility. Pure functions for decisions; side effects at the edges.
- Explicit types at crate boundaries; no `impl Trait` in public port signatures.
- Prefer `jiff` for time, UUIDv7 for IDs, `BTreeMap` where order matters for snapshots.
- User-facing text (cards, digests, errors) is short, plain and specific.
