# Instructions for coding agents

You are working on **Domovoi**, a self-hosted server overseer written in Rust with an embedded React UI. Most of the code is written by coding agents executing the prompts in [`docs/build-plan/prompts`](docs/build-plan/prompts), one prompt per branch and pull request. These rules apply to every session.

## Read before you write

1. [`ARCHITECTURE.md`](ARCHITECTURE.md): the crates, what each may depend on, the domain model, the protocol and the security invariants. It is the contract.
2. The prompt you were given, completely, including **Out of scope**.
3. The ADRs the prompt links to, and any existing code in the crates you will touch.

If the prompt and `ARCHITECTURE.md` disagree, stop and say so. Do not pick one silently.

## Commands

[`TESTING.md`](TESTING.md) is the full reference: prerequisites, every CI job and its local command, partial test runs, running the hub, agents and web UI by hand, and troubleshooting. CI runs on Linux; the hub and agent are Linux programs, so on Windows run them inside WSL 2. This machine's WSL has no Rust or Node, so run checks on Windows and treat CI as the final word.

| Task | Command |
| --- | --- |
| Rust CI checks (everything CI runs; do not run it by default) | `cargo xtask ci` (`--fast` skips deny and the web checks; `--web` adds the UI and the hub with it embedded) |
| Architecture rules only | `cargo xtask arch-check` (CI also runs `--base origin/main`) |
| Format | `cargo fmt --all` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Tests for what you changed | `cargo nextest run -p <crate>`, or `cargo nextest run -E 'rdeps(<crate>)'` when a public item changed; `cargo test -p <crate>` without nextest |
| Snapshot review | `cargo insta review` (only accept snapshots you have read) |
| Web | `cd web && npm ci && npm run lint && npm run typecheck && npm test && npm run format:check && npm run build` |
| Regenerate API types | `cargo xtask openapi && cd web && npm run gen:api` |
| Regenerate the config reference | `cargo xtask docs-config` |
| Book links, anchors and pages | `cargo xtask docs-links` |
| Checks `cargo xtask ci` leaves out | Rustdoc, web `format:check` and `gen:api`, `arch-check --base`: see [Before you push](TESTING.md#before-you-push) |
| Run the UI against a dev hub | `cargo run -p domovoi-hub --example dev_hub`, then `cd web && npm run dev` |

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
- Snapshot tests (`insta`) for rendered text and wire formats. Review every snapshot change. A breaking protocol change bumps the protocol major version (R14).
- Tests that assert a shell string or an approval is rejected live under `tests/`, because source scans exempt only test paths.

## Debugging and delegation in this repository

General rules for debugging, delegation and staging live in the global `~/.config/opencode/AGENTS.md`. Here they mean:

- Run one failing test with `cargo test -p <crate> --test <bin> <name> -- --nocapture` (`--lib` in place of `--test <bin>` for unit tests).
- Temporary debug output means `eprintln!` or `dbg!` in test code. Hard rule 5 still applies to library code.
- The checks that go through `general` are `cargo xtask ci`, `cargo test`, `cargo nextest`, `npm test` and `git diff`. Scoped runs (one crate, or one web area) are short: run those yourself when you are fixing a failure.

## Workflow for each prompt

1. Create a branch named after the change (`incident-ttl`, `runner-argv`), never after the prompt number.
2. Plan first: list the files you will create or change and the tests you will write. If anything in the prompt is ambiguous, ask before coding.
3. Implement in small commits using Conventional Commits (`feat(engine): ...`, `fix(runner): ...`, `test(store): ...`, `docs: ...`, `chore: ...`).
4. Run only the checks for what you changed, as in [While you work](TESTING.md#while-you-work): `cargo fmt --all`, `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>` for each crate you edited, `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item other crates use, `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`, and for `web/` the vitest run for the changed area plus `npm run lint` and `npm run typecheck`. Do not run the whole workspace suite or `cargo xtask ci`: CI on GitHub runs everything on the pull request. Do not stop with a red scoped check.
5. Update docs the prompt names (book pages, `ARCHITECTURE.md` only if the prompt says so).
6. Write the PR description: what changed, which acceptance criteria are met and how, anything deferred.

## Commits, PRs and committed files

- Commit messages are a single line: the Conventional Commits subject, nothing else. No body, no trailers.
- Commits are authored as `Rostislav <raikov0411@gmail.com>`.
- PR titles have at most one scope, because CI rejects anything else: `feat(web): ...`, never `feat(hub,web): ...`.
- `AGENTS.md` is committed. Keep it in step with the rest of the repository: when a command, rule or workflow changes, update it in the same change.
- `CLAUDE.md`, `.claude/`, `.opencode/`, `graphify-out/` and `docs/build-plan/` are local only (excluded in `.git/info/exclude`). Never commit them, and never `git add -f` them.
- `AGENTS.md` is the one exception to the next rule: it may name coding agents, prompts and the build plan, because it exists to instruct them.
- Every other committed file, every commit and every PR never mentions prompts, prompt numbers (`P07`), the build plan, coding agents or AI assistance. Write "coming in v0.1" rather than "added by prompt P24", and describe a PR by what it changes.

## When something does not fit

- A rule blocks a reasonable design: stop and write an ADR proposal in `docs/adr` instead of bending the code.
- A prompt depends on something that does not exist yet: implement the minimal port or stub inside the prompt's scope and say so in the PR, or stop and ask.
- You find a bug outside your scope: note it in the PR description; do not fix it in the same change.

## Style

- Small modules with one responsibility. Pure functions for decisions; side effects at the edges.
- Explicit types at crate boundaries; no `impl Trait` in public port signatures.
- Prefer `jiff` for time, UUIDv7 for IDs, `BTreeMap` where order matters for snapshots.
- User-facing text (cards, digests, errors) is short, plain and specific.

## graphify (code map)

`graphify-out/graph.json` is an AST graph of the repo (functions, types, files, calls, imports). It is a map for finding where things are, not a source of truth. Always read the real lines before editing.

Use it when you would otherwise grep or read several files to answer a structural question:

- Who calls or uses X, what breaks if I change X: `graphify affected "X"`. This fails with "No unique node match" when several files share a name (most types do). Run `graphify explain "X"` first to get the node `ID`, then pass that ID (for example `action_approval_humanapproval`) to `affected` and `path`.
- `affected` and `explain` miss uses the AST cannot see (macros, trait dispatch through generics). Confirm with grep before claiming nothing else uses X.
- How are A and B connected, across crates: `graphify path "A" "B"`.
- What is X and what touches it: `graphify explain "X"`.
- Where does a concept live, before a cross-crate change: `graphify query "<identifiers>"`.

Known weaknesses (checked against this repo):

- Every `use` of a type creates its own stub node, so a name like `ReadTool` or `HumanApproval` has several nodes, and the real definition's edges only point outward. `explain "Name"` silently picks one of them, often an import stub with 1 to 3 connections, and `affected` on the real node returns almost nothing (for `HumanApproval` it listed only `ApprovalChannel`, while grep finds 14 files using it).
- So treat `query` as the useful command and `explain` and `affected` as hints. Judge a node by its `Source:` line: if it is an import or a local stub, not the definition, it told you nothing. Find the definition with `query` or grep, and confirm users with grep before claiming nothing else uses X.
- `explain` and `path` need exact identifiers, not sentences.

Query with identifiers (`HumanApproval`, `TriageReport`, `ReadTool`), not sentences. Sentences match common words like "how" or "web" and return noise. Add `--budget 1500` to keep output small.

Skip it when:

- You already know the file, or the change is inside one file or one function.
- The task is docs, config, migrations or `web/` styling.
- You need the exact current text. Use grep and read `offset`/`limit` instead.
- Output looks wrong or stale. Say so, run `graphify update .`, retry once, then fall back to grep.

Rules:

- Do not read `GRAPH_REPORT.md` or `graph.json` whole. Use it only for a broad architecture review.
- The graph does not replace `ARCHITECTURE.md` §4 or `cargo xtask arch-check`. Those stay the authority on allowed dependencies.
- The git post-commit and post-checkout hooks refresh the graph, code files only. After editing code and before querying it again in the same session, run `graphify update .` (AST only, free, about a minute).
- `graphify-out/` is local only and excluded from git. Never commit it.
