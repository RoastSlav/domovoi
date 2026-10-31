# Contributing to Domovoi

Thanks for helping. Domovoi runs with root-level reach on people's servers, so the bar for correctness and safety is high, and much of it is enforced automatically.

## Before you start

- Read [ARCHITECTURE.md](ARCHITECTURE.md). Its rules (R1–R14) and invariants (INV-1–INV-12) are the contract.
- For anything larger than a bug fix, open an issue first so we can agree on the approach.
- Changes to a rule, a crate boundary, a wire format or an invariant need an ADR in [`docs/adr`](docs/adr).

## Development setup

You need Rust (the toolchain in `rust-toolchain.toml` installs automatically through rustup) and Node.js 22 for the web UI.

```sh
git clone https://github.com/RoastSlav/domovoi && cd domovoi
cargo xtask ci          # fmt, clippy, tests, arch-check, cargo-deny
cargo xtask ci --web    # also lint, typecheck, test and build the web UI
```

Optional but recommended: `cargo install cargo-nextest cargo-deny cargo-insta --locked`.

## Rules CI enforces

| Check | What it protects |
| --- | --- |
| `cargo xtask arch-check` | crate dependency direction, no I/O in pure crates, process spawning only in the runner, no shell, approval call sites, manifest hygiene, file size, append-only migrations, ADRs for rule changes |
| `cargo clippy -- -D warnings` | no `unwrap`/`panic` in library code, disallowed APIs (`Command::new`, `env::set_var`, `thread::sleep`) |
| Gate tests (`tests/gates.rs`) | security invariants; never delete or weaken them |
| `cargo deny check` | licenses, advisories, OpenSSL ban, unknown sources |
| Web checks | ESLint (no `fetch` outside `src/api`), types, tests, build, fresh generated API types |

## Pull requests

- One logical change per PR. Title in [Conventional Commits](https://www.conventionalcommits.org/) form, e.g. `feat(engine): fold repeated signals into the incident thread`; the release changelog is built from these.
- Include tests that would fail without your change. Name tests after behaviour.
- Update docs (`docs/book`) when behaviour users see changes.
- Keep `cargo xtask ci` green locally before pushing.

## Commit sign-off and licensing

By contributing you agree your contribution is dual licensed under MIT OR Apache-2.0, as stated in the README.

## Maintainers

Repository settings and release steps: [`docs/maintainers/repository-setup.md`](docs/maintainers/repository-setup.md).
