# Testing Domovoi locally

This page covers running every CI check on your own machine, and running the hub, agents and web UI locally to try a change by hand. [CONTRIBUTING.md](CONTRIBUTING.md) has the rules the checks enforce.

CI runs on Linux (Ubuntu 24.04). The hub and the agent are Linux programs, so on Windows run everything below inside WSL 2.

## Prerequisites

| Tool | Needed for | Install |
| --- | --- | --- |
| Rust via rustup | everything | <https://rustup.rs>; the pinned toolchain in `rust-toolchain.toml` installs itself on first use |
| Node.js 22 and npm | web UI checks and dev server | <https://nodejs.org> or your package manager |
| cargo-nextest | tests with per-test timeouts, as in CI | `cargo install cargo-nextest --locked` |
| cargo-deny | supply-chain check | `cargo install cargo-deny --locked` |
| cargo-insta | reviewing snapshot changes | `cargo install cargo-insta --locked` |
| Docker | container smoke test, installer test, workflow lint | optional |
| shellcheck | shell script lint | optional, e.g. `sudo apt install shellcheck` |
| cargo-llvm-cov | coverage | optional, `cargo install cargo-llvm-cov --locked` |

`cargo xtask ci` works without nextest or cargo-deny. It falls back to `cargo test` and skips deny, and prints a note when it does.

## While you work

Run only the checks for what you changed. The full suite takes a long time, and CI runs all of it on every pull request, so there is no need to run it all by hand.

```sh
cargo fmt --all
cargo clippy -p <crate> --all-targets -- -D warnings
cargo nextest run -p <crate>                      # the crate you changed
cargo nextest run -E 'rdeps(<crate>)'             # the crate and every crate that depends on it
cd web && npx vitest run src/features/<area>      # web: only the tests for the area you changed
```

- Changed one crate and only its internals: test that crate.
- Changed a public type, function, trait, protocol message or migration of a crate other crates use (`domovoi-core`, `domovoi-proto`, `domovoi-store`, `domovoi-testkit`): test the crate and its dependents with `rdeps(<crate>)`.
- Changed several crates: run each one's tests.
- Added or moved a module, or touched a `Cargo.toml`: also run `cargo xtask arch-check`, which is quick.
- Touched `web/`: run the tests for the changed area, plus `npm run lint` and `npm run typecheck`.
- Without nextest, use `cargo test -p <crate>`.

When a changed crate has a `tests/gates.rs`, run it too; it is part of the crate's tests already.

CI on GitHub runs the whole workspace, the web checks, deny and the rest on the pull request. A failure there that your scoped run missed is expected now and then; fix it on the branch. Run the full `cargo xtask ci` below only when you want the whole picture before pushing, for example after a change to a shared crate.

## Before you push

To run everything CI runs, use one command from the repository root:

```sh
cargo xtask ci --fast   # fmt, clippy, tests, feature-gated checks, arch-check, generated-file checks
cargo xtask ci          # the above plus cargo-deny
cargo xtask ci --web    # the above plus the web UI and the hub with the UI embedded
```

Use `--web` whenever you touch `web/` or the hub's API. The run stops at the first failing step and names it. Each step is killed after 30 minutes so a hung test cannot block it forever; set `DOMOVOI_CI_STEP_TIMEOUT_MINS` to change the limit.

`cargo xtask ci --web` covers almost everything the main CI workflow runs. These checks are not in it; run them yourself when they apply:

```sh
# Architecture rules that compare against main (append-only migrations, ADR required)
git fetch origin main && cargo xtask arch-check --base origin/main

# Rustdoc with warnings as errors
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
RUSTDOCFLAGS="-D warnings" cargo doc --package domovoi-checks --all-features --no-deps --locked

# Web formatting and generated API types (in web/)
cd web && npm run format:check && npm run gen:api && git diff --exit-code -- src/api && cd ..

# The release binary with the UI embedded (needs web/dist from `npm run build`)
cargo build --package domovoi --features embed-ui --locked
```

## Every CI job and how to run it locally

CI (`.github/workflows/ci.yml`) only runs a job when files it covers change. The "Runs when" column says which.

| CI job | Runs when | Local command |
| --- | --- | --- |
| PR title follows Conventional Commits | every PR | Title like `feat(engine): …`, `fix(web): …`, `docs: …` |
| Rust (fmt, clippy, tests, docs, arch-check) | `crates/`, `xtask/`, `Cargo.*`, `ARCHITECTURE.md`, `docs/adr/` | `cargo xtask ci`, plus `arch-check --base` and rustdoc above |
| Web (lint, typecheck, test, build) | `web/` | `cargo xtask ci --web`, plus `format:check` and `gen:api` above |
| Build web UI for embedding | hub UI embedding files | `cd web && npm ci && npm run build` |
| Book links, anchors and pages | `docs/book/`, `web/src/` | `cargo xtask docs-links` |
| Locked workspace metadata | version-only `Cargo.*` bumps | `cargo metadata --locked --no-deps --format-version 1 > /dev/null` |
| Supply chain (cargo-deny) | `Cargo.*`, `deny.toml` | `cargo deny check` |
| MSRV | `Cargo.*`, `rust-toolchain.toml` | `rustup toolchain install 1.94 --profile minimal && cargo +1.94 check --workspace --locked` (the version is `rust-version` in `Cargo.toml`) |
| Static musl build check | `Cargo.*` | `sudo apt install musl-tools && rustup target add x86_64-unknown-linux-musl && cargo check --workspace --locked --target x86_64-unknown-linux-musl` |
| Workflow lint | `.github/workflows/` | `docker run --rm -v "$PWD:/repo" --workdir /repo rhysd/actionlint:1.7.12 -color` |
| Shell scripts lint | `install.sh`, `deploy/test/`, skill `run` scripts | `shellcheck install.sh deploy/test/*.sh examples/skills/*/run` |
| Installer on ubuntu:24.04 / debian:12 | `install.sh`, `deploy/test/installer.sh` | see [Installer test](#installer-test) |
| systemd units verify | `deploy/systemd/` | see [systemd units](#systemd-units) |
| Coverage | Rust changes | `cargo llvm-cov nextest --workspace --locked --no-tests=pass --html`, then open `target/llvm-cov/html/index.html` |

Other workflows:

| Workflow | Local command |
| --- | --- |
| Container smoke test (`docker.yml`) | see [Docker image](#docker-image) |
| Docs (`docs.yml`) | `cargo install mdbook --locked && mdbook serve docs/book`, then open <http://localhost:3000> |
| Drills (live model) (`drills-live.yml`) | see [Triage drills](#triage-drills) |
| CodeQL, OpenSSF Scorecard, Release | GitHub only; nothing to run locally |

## Running part of the test suite

```sh
cargo nextest run -p domovoi-engine                        # one crate
cargo nextest run -p domovoi-hub --test hub                # one test binary
cargo nextest run --workspace -E 'test(enrollment)'        # tests whose name contains "enrollment"
cargo test -p domovoi-core --test gates                    # one crate's security gate tests
```

Without nextest, use `cargo test -p <crate> [--test <binary>] [name filter]`.

**Snapshots.** Some tests compare output with saved snapshots (insta). When a change is intended, run the tests, then accept or reject each change with:

```sh
cargo insta review
```

Commit the updated `.snap` files with the change. Never commit `*.pending-snap` files.

**Generated files.** If the OpenAPI check or the configuration reference check fails, regenerate the files and commit them:

```sh
cargo xtask openapi                 # writes web/src/api/openapi.json
cd web && npm run gen:api && cd ..  # writes web/src/api/schema.d.ts
cargo xtask docs-config             # writes the configuration reference in the book
```

**Web only** (in `web/`): `npm ci`, then `npm run lint`, `npm run typecheck`, `npm test` (or `npm run test:watch` while working), `npm run format:check` and `npm run build`. `npm run format` fixes formatting. `npm test` also proves colour contrast, runs axe over every page test's output and checks the voice rules; `npm run build` ends with `npm run size`, which fails when the first download, a chunk or the CSS is over budget (`web/DESIGN.md`).

To look at every page at 375, 768, 1280 and 1920 px in light and dark, start the dev hub and `npm run dev`, then run `node scripts/screens.mjs` in `web/` (it drives an installed Chrome through Playwright, writes pictures to `web/screens-out`, and lists pages that scroll sideways; the header of the script lists its settings). It is not part of CI.

### Triage drills

The drills replay incidents through the triage engine. They run with a scripted model as part of the normal tests. To score a real model, as the "Drills (live model)" workflow does:

```sh
DOMOVOI_DRILLS_LIVE=1 \
DOMOVOI_DRILLS_PROVIDER=anthropic \
DOMOVOI_DRILLS_MODEL=<model name> \
DOMOVOI_DRILLS_API_KEY=<your key> \
cargo test --locked -p domovoi-engine --test drills -- --ignored drills_live --nocapture
```

For an OpenAI-compatible server, such as a local Ollama, use `DOMOVOI_DRILLS_PROVIDER=openai` and set `DOMOVOI_DRILLS_BASE_URL`. The report is written to `target/drills/`. This calls a real API, so it can cost money.

## Running the app locally

### UI work: the dev hub and the Vite dev server

The dev example hub serves the real API over a temporary database. It comes with fake agents for two hosts, a seeded incident and two proposed actions:

```sh
cargo run -p domovoi-hub --example dev_hub   # 127.0.0.1:7420, agent server on 127.0.0.1:7421
```

In a second terminal:

```sh
cd web
npm ci
npm run dev   # http://localhost:5173, proxies /api and /hooks to 127.0.0.1:7420
```

Open the setup link the dev hub printed, with port `5173` in place of `7420`, and create the admin account. Press Enter in the dev hub's terminal to fire another incident. [web/README.md](web/README.md) has more detail.

### A real hub

Keep local data in `./data`, which git ignores, so nothing touches `/var/lib/domovoi`:

```sh
cargo build -p domovoi
DOMOVOI_HUB__DATA_DIR=./data/hub ./target/debug/domovoi hub run
```

The hub listens on <http://127.0.0.1:7420> for the web UI and `0.0.0.0:7421` for agents. A new hub prints a setup link: open it to create the admin account. Without `--features embed-ui` the hub serves a page explaining how to build the UI. Either build it in (`cd web && npm run build && cd .. && cargo build -p domovoi --features embed-ui`) or use the Vite dev server above against this hub.

To start over, stop the hub and delete `./data`.

### A hub with an agent in one process

Write `./data/agent.toml`:

```toml
[agent]
host_name = "dev"
hub_url = "wss://127.0.0.1:7421"   # not used in this mode, but must be a valid wss:// URL
state_dir = "./data/agent"
skills_dir = "./examples/skills"

[docker]
enabled = false                    # "auto" looks for Docker or Podman; true or false fixes it
```

Then:

```sh
./target/debug/domovoi check config data/agent.toml --kind agent
DOMOVOI_HUB__DATA_DIR=./data/hub ./target/debug/domovoi hub run --with-local-agent data/agent.toml
```

The log shows `local agent started` and `agent connected`. Without root, checks that need it, such as SMART and the journal, report errors.

### A separate agent

This runs the same enrollment path as a real remote host. Write `./data/laptop.toml` like the file above, with `host_name = "laptop"` and `state_dir = "./data/laptop"`. Writing it first keeps the agent's state out of `/var/lib/domovoi-agent`; `enroll` only updates `hub_url` in an existing file.

With the hub stopped (the command opens its database), create a token:

```sh
DOMOVOI_HUB__DATA_DIR=./data/hub ./target/debug/domovoi hub token create laptop
```

While the hub runs you can instead use **Hosts → Add host** in the web UI. Start the hub again, then enroll the agent. Use `wss://127.0.0.1:7421`, not the host name in the printed command:

```sh
./target/debug/domovoi agent enroll --config data/laptop.toml \
  --hub wss://127.0.0.1:7421 --token <token> --fingerprint <fingerprint> --name laptop
./target/debug/domovoi agent run --config data/laptop.toml
```

### End-to-end self-test

This pushes a synthetic critical signal through the whole pipeline, from incident and triage to card and delivery. It uses a temporary data directory and needs no network:

```sh
./target/debug/domovoi selftest --offline
```

Every stage should print `PASS`. Without `--offline` and with `--config hub.toml`, it uses your configured model and chat gateways and posts a real card.

### Docker image

These are the steps the container smoke test runs:

```sh
docker build -t domovoi:dev .
docker run --rm domovoi:dev version
docker run --rm domovoi:dev selftest --offline
docker volume create domovoi-dev
docker run -d --name domovoi-dev -p 127.0.0.1:7420:7420 -v domovoi-dev:/var/lib/domovoi domovoi:dev
curl -fsS http://127.0.0.1:7420/healthz
docker logs domovoi-dev 2>&1 | grep '/setup?token='
docker stop -t 20 domovoi-dev && docker rm domovoi-dev && docker volume rm domovoi-dev
```

### Installer test

`deploy/test/installer.sh` installs and upgrades Domovoi from a fake release served locally, checks that config edits survive and that a bad checksum is refused. It creates users and systemd units, so run it only in a throwaway container:

```sh
docker run --rm -v "$PWD:/src" -w /src debian:12 \
  sh -c 'apt-get update -qq && apt-get install -y -qq ca-certificates curl python3 >/dev/null && sh deploy/test/installer.sh'
```

CI runs it on `ubuntu:24.04` too.

### systemd units

`systemd-analyze verify` checks that `ExecStart` exists, so CI puts a stub binary at `/usr/local/bin/domovoi` first. On a systemd machine that has no real `domovoi` installed:

```sh
printf '#!/bin/sh\n' | sudo tee /usr/local/bin/domovoi >/dev/null && sudo chmod 0755 /usr/local/bin/domovoi
systemd-analyze verify --man=no deploy/systemd/domovoi-hub.service deploy/systemd/domovoi-agent.service
sudo rm /usr/local/bin/domovoi
```

## Troubleshooting

- **Port 7420 or 7421 is in use.** Another hub or the dev hub is still running. Stop it, or set `DOMOVOI_WEB__BIND` / `DOMOVOI_AGENT_SERVER__BIND` to other addresses.
- **`error: a hub is already running on …`.** `hub token create`, `hub set-password` and similar commands need the hub stopped. Stop it, or use the web UI.
- **Tests hang.** Install cargo-nextest: `.config/nextest.toml` kills a test after 2 minutes, while plain `cargo test` waits forever.
- **A web check says generated API types are stale.** Run the [generated files](#running-part-of-the-test-suite) commands, then commit the results.
