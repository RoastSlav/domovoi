# Testing Domovoi locally

This page covers running every CI check on your own machine, and running the hub, agents and web UI locally to try a change by hand. [CONTRIBUTING.md](CONTRIBUTING.md) has the rules the checks enforce.

CI runs on Linux (Ubuntu 24.04). The hub and the agent are Linux programs, so on Windows run everything below inside WSL 2.

## Prerequisites

| Tool               | Needed for                                          | Install                                                                                         |
| ------------------ | --------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Rust via rustup    | everything                                          | <https://rustup.rs>; the pinned toolchain in `rust-toolchain.toml` installs itself on first use |
| Node.js 22 and npm | web UI checks and dev server                        | <https://nodejs.org> or your package manager                                                    |
| cargo-nextest      | tests with per-test timeouts, as in CI              | `cargo install cargo-nextest --locked`                                                          |
| cargo-deny         | supply-chain check                                  | `cargo install cargo-deny --locked`                                                             |
| cargo-insta        | reviewing snapshot changes                          | `cargo install cargo-insta --locked`                                                            |
| Docker             | container smoke test, installer test, workflow lint | optional                                                                                        |
| shellcheck         | shell script lint                                   | optional, e.g. `sudo apt install shellcheck`                                                    |
| cargo-llvm-cov     | coverage                                            | optional, `cargo install cargo-llvm-cov --locked`                                               |

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

When a changed crate has a `tests/gates.rs`, run it too; it is part of the crate's tests already. The engine's and the hub's execution-boundary gates compile `xtask` as a dev-only dependency and call the same checker `cargo xtask arch-check` runs, so a boundary finding arrives from one implementation to both the tool and the gate. The checker follows `mod`, `#[path]` and `include!` from the Cargo targets to tell test-only files from production ones, and refuses (`SRC-1`) Rust it cannot inspect; its unit tests build real files in temporary directories (`cargo test -p xtask`).

`domovoi-hub`'s `test-support` feature and `domovoi`'s matching dev-dependency are what let the hub and binary integration tests drive an agent's wire directly. The feature is non-default and those packages' test and development builds turn it on through their dev-dependencies, so the plain commands above need no feature flag, and a normal production build has no such accessor. The test builds in CI therefore compile it; selecting it explicitly (`--features test-support`, or `--all-features`) compiles it in any build, as with any Cargo feature.

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

| CI job                                      | Runs when                                                      | Local command                                                                                                                                       |
| ------------------------------------------- | -------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| PR title follows Conventional Commits       | every PR                                                       | Title like `feat(engine): …`, `fix(web): …`, `docs: …`                                                                                              |
| Rust (fmt, clippy, tests, docs, arch-check) | `crates/`, `xtask/`, `Cargo.*`, `ARCHITECTURE.md`, `docs/adr/` | `cargo xtask ci`, plus `arch-check --base` and rustdoc above                                                                                        |
| Web (lint, typecheck, test, build)          | `web/`                                                         | `cargo xtask ci --web`, plus `format:check` and `gen:api` above                                                                                     |
| Build web UI for embedding                  | hub UI embedding files                                         | `cd web && npm ci && npm run build`                                                                                                                 |
| Book links, anchors and pages               | `docs/book/`, `web/src/`                                       | `cargo xtask docs-links`                                                                                                                            |
| Locked workspace metadata                   | version-only `Cargo.*` bumps                                   | `cargo metadata --locked --no-deps --format-version 1 > /dev/null`                                                                                  |
| Supply chain (cargo-deny)                   | `Cargo.*`, `deny.toml`                                         | `cargo deny check`                                                                                                                                  |
| MSRV                                        | `Cargo.*`, `rust-toolchain.toml`                               | `rustup toolchain install 1.94 --profile minimal && cargo +1.94 check --workspace --locked` (the version is `rust-version` in `Cargo.toml`)         |
| Static musl build check                     | `Cargo.*`                                                      | `sudo apt install musl-tools && rustup target add x86_64-unknown-linux-musl && cargo check --workspace --locked --target x86_64-unknown-linux-musl` |
| Workflow lint                               | `.github/workflows/`                                           | `docker run --rm -v "$PWD:/repo" --workdir /repo rhysd/actionlint:1.7.12 -color`                                                                    |
| Shell scripts lint                          | `install.sh`, `deploy/test/`, skill `run` scripts              | `shellcheck install.sh deploy/test/*.sh examples/skills/*/run`                                                                                      |
| Installer on ubuntu:24.04 / debian:12       | `install.sh`, `deploy/test/installer.sh`                       | see [Installer test](#installer-test)                                                                                                               |
| systemd units verify                        | `deploy/systemd/`                                              | see [systemd units](#systemd-units)                                                                                                                 |
| Real accounts and systemd units (not required) | `install.sh`, `deploy/`, `crates/domovoi-helper/`; nightly and on demand | see [Privileged identity test](#privileged-identity-test)                                                                            |
| Containment namespace tests (not required)  | `crates/domovoi-core/src/containment/`, `crates/domovoi-helper/src/nft/`, `crates/domovoi-agent/src/containment/`, the workflow | see [Containment namespace tests](#containment-namespace-tests) |
| Coverage                                    | Rust changes                                                   | `cargo llvm-cov nextest --workspace --locked --no-tests=pass --html`, then open `target/llvm-cov/html/index.html`                                   |

Other workflows:

| Workflow                                | Local command                                                                                |
| --------------------------------------- | -------------------------------------------------------------------------------------------- |
| Container smoke test (`docker.yml`)     | see [Docker image](#docker-image)                                                            |
| Docs (`docs.yml`)                       | `cargo install mdbook --locked && mdbook serve docs/book`, then open <http://localhost:3000> |
| Drills (live model) (`drills-live.yml`) | see [Triage drills](#triage-drills)                                                          |
| Containment leak tests (`containment-leak.yml`) | see [Containment namespace tests](#containment-namespace-tests)                       |
| CodeQL, OpenSSF Scorecard, Release      | GitHub only; nothing to run locally                                                          |

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

**Browser smoke test.** `npm run smoke` (in `web/`) drives the built bundle in a real browser against a mock hub, to check what jsdom cannot: layout, focus, the right-hand slot, reduced motion and colour contrast. It serves the hub's real Content-Security-Policy (the `BASE_POLICY` constant read from `crates/domovoi-hub/src/csp.rs`, plus `script-src 'self'` and the hash of each inline script of `dist/index.html`), proves it is enforced with a blocked inline script, and fails on any violation; for that reason axe is injected with `page.evaluate`, not a script tag. It also checks the operation graph: the default view, the keyboard, selection shared with the list, reduced motion, and that the graph writes no `style` attribute. It also replays an operation: the mock hub serves `GET /api/v1/operations/{id}/history` (built from the trace fixtures) and the checks open the replay bar, scrub, step, autoplay, check that the rest of the page stays live, and press the replay keys, with and without reduced motion. It needs a built `dist/`, so run `npm run build` first. It uses Playwright's Chromium (`npx playwright install chromium`), or set `SMOKE_CHANNEL=chrome` to drive an installed Chrome. CI runs it in job `web`. `SCREENS_OPERATION=<id>` makes `scripts/screens.mjs` also capture `/operations/<id>`.

### Crash sweep

A crash can hit any commit: before it becomes durable, or after it is durable but before the caller sees the receipt. The sweep proves that neither leaves a state without its journal and audit rows. For each of two lifecycles (one incident's, one action's) it runs the scenario fault-free to learn its commit sequence, then re-runs it once per commit and per crash point, restarts the services over what survived, and checks three things: the durable state is exactly the committed prefix, the cross-table invariants hold before and after recovery, and the services still work. `CrashingStore` in `domovoi-testkit::crash` makes the chosen commit fail; the scenarios live in `crates/domovoi-engine/tests/crash/` and the hub runs the same files against real SQLite, reopening the database file as the restart.

```sh
cargo nextest run -p domovoi-engine -E 'binary(crash)'       # in-memory sweep and its gates
cargo nextest run -p domovoi-hub -E 'test(crash_sqlite)'     # the same sweep on SQLite
```

Failures name the scenario, the commit index, the crash point and both snapshots or every invariant violation, so the first line usually says which write broke. To add a scenario for a new writer: write it in `crates/domovoi-engine/tests/crash/scenarios.rs` against `World`, add a test that runs it to `memory.rs` and to the hub's `crash_sqlite.rs` with a `min_commits` floor, and add the file of the new commit site to `COMMIT_SITES` in `coverage.rs` — `commit_sites_are_the_ones_the_sweep_exercises` fails until you do, and `sweep_covers_every_kind_of_write` names any write kind, status, journal kind or audit kind no scenario produces.

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

CI runs it on `ubuntu:24.04` too. It has no systemd, so for an agent it checks the path of a host that cannot get the privilege boundary: no accounts, no helper, today's modes and the "privilege boundary not in effect" notice. The three-identities install is tested by the next script.

### Privileged identity test

`deploy/test/identities.sh` installs with the real accounts, directories and systemd units: the accounts and every path of the layout with its mode, the root helper behind its socket, the helper's read operations under the widened unit, the self-test. It cannot run in a container. It needs a **disposable machine that boots with systemd as PID 1** (a virtual machine), root, and a built binary. It creates and deletes the accounts `domovoi-agent` and `domovoi-skill`, changes owners and modes, and starts the helper socket, so never run it on a machine you care about. It refuses to run when any of those names or paths already exists, and it exits 2 unless `DOMOVOI_PRIVILEGED_TESTS=1` is set.

```sh
cargo build --package domovoi
sudo env "PATH=$PATH" "HOME=$HOME" DOMOVOI_PRIVILEGED_TESTS=1 \
  DOMOVOI_BIN="$PWD/target/debug/domovoi" sh deploy/test/identities.sh
```

It also needs `python3`, `curl`, `useradd`, `userdel`, `groupadd`, `groupdel`, `gpasswd`, `runuser`, `systemctl` and `journalctl`. The release is a fake one served on `127.0.0.1:18734`, as in the installer test. Each check prints `ok`, `FAIL` or `skip` with its name; the row that needs a `docker` group is skipped on a machine without one. The read-operation rows (`helper_unit_starts_under_the_widened_directives`, `helper_smart_report_runs_in_the_shipped_unit`, `helper_kernel_log_reads_with_dmesg_restrict_on`, `helper_net_listeners_show_the_owning_process_of_another_user`, `helper_docker_read_works_through_the_real_socket_as_domovoi_agent`, `agent_with_boundary_in_effect_reads_docker_and_smart_through_the_helper`, `selftest_still_passes_on_a_clean_host_with_the_widened_unit`) are the measurement of the widened helper unit. They set `kernel.dmesg_restrict=1` for one row and put it back, start a listener under a `domovoi-probe-ci-listen` account, and skip with a notice where there is no SMART disk or no Docker socket. A trap deletes only what the script recorded, and the last check fails when a `domovoi-probe-ci-*` account or one of the real names is left behind.

### Privileged skill runner test

`deploy/test/skill-runner.sh` tests the skill runner with real accounts and real systemd. It renders the socket and service units of `deploy/systemd` under `domovoi-probe-ci-*` names, runs the built binary behind the socket, and measures the systemd behaviours the unit design relies on: the mount mask, the read-only binds, the private `/tmp`, the cgroup kill, `226/NAMESPACE` for an absent bind source. It has the same needs as the identity test: a disposable machine with systemd as PID 1 and cgroup v2, root, and a built binary. It uses the existing `domovoi-agent` and `domovoi-skill` accounts as found and creates, then deletes, one that is missing. It exits 2 unless `DOMOVOI_PRIVILEGED_TESTS=1` is set.

```sh
cargo build --package domovoi
sudo env "PATH=$PATH" DOMOVOI_PRIVILEGED_TESTS=1 \
  DOMOVOI_BIN="$PWD/target/debug/domovoi" sh deploy/test/skill-runner.sh
```

It also needs `python3`, `useradd`, `userdel`, `groupadd`, `groupdel`, `gpasswd`, `runuser`, `systemctl`, `journalctl` and `findmnt`. The rows that run a skill through the runner need `DOMOVOI_IMPL_HASH_CMD`, a command that takes a skill folder and prints its implementation hash; without it they are skipped and say so. The unprivileged runner tests (the protocol, the serve loop, the cancel and the client) are ordinary `cargo nextest run -p domovoi-runner -p domovoi-agent` tests.

The installer test also covers the runner steps without systemd: the rendering of the unit for custom directories, the refusal of a `--prefix` the runner cannot execute from, the `agent dirs` helper and the order of the sockets. `shellcheck install.sh deploy/test/*.sh examples/skills/*/run` covers `skill-runner.sh` as well.

CI runs both privileged scripts in the `Real accounts and systemd units` job on `ubuntu-24.04`, which first prints `systemctl --version`, the cgroup filesystem and the name of PID 1 and fails with a clear message when PID 1 is not systemd. The job runs on pull requests that touch `install.sh`, `deploy/` or `crates/domovoi-helper/`, nightly, and on demand (`workflow_dispatch`). It is not in the required checks yet.

### Containment namespace tests

The helper's `nft` group is tested against a real `nft` in a throwaway network namespace. Linux only, root, with `nftables`, `iproute2` and `conntrack` installed. The tests skip with a printed reason, and pass, unless `DOMOVOI_NS_TESTS=1` is set; a test that needs a missing prerequisite (`nf_flow_table`, `conntrack`) says so and skips.

```sh
cargo test --package domovoi-helper --test nft_ns --no-run
sudo env "PATH=$PATH" "HOME=$HOME" DOMOVOI_NS_TESTS=1 \
  unshare --net --mount sh -c 'mount -t tmpfs tmpfs /run && ip link set lo up && cargo test --package domovoi-helper --test nft_ns -- --nocapture'
```

Without root, `unshare -Urnm sh -c 'mount -t tmpfs tmpfs /run && DOMOVOI_NS_TESTS=1 cargo test --package domovoi-helper --test nft_ns'` works where the kernel allows unprivileged user namespaces; CI does not use it, because recent Ubuntu refuses it under AppArmor. The tests that need a real kernel timer set 3 to 5 second timeouts and poll for at most 15 seconds; none waits for the helper's own 30 seconds.

The load tests, which flood a namespace with UDP while the helper replaces an element, also need `DOMOVOI_LOAD_TESTS=1` and `DOMOVOI_BIN`, the path of a built `domovoi` (the self-test re-invokes it as its sender):

```sh
cargo build --package domovoi
sudo env "PATH=$PATH" "HOME=$HOME" DOMOVOI_NS_TESTS=1 DOMOVOI_LOAD_TESTS=1 \
  DOMOVOI_BIN="$PWD/target/debug/domovoi" \
  unshare --net --mount sh -c 'mount -t tmpfs tmpfs /run && ip link set lo up && cargo test --package domovoi-helper --test nft_ns -- --nocapture load'
```

CI runs the first group in the `Containment namespace tests` job on pull requests that touch the files named in the table above. The second group is the separate `Containment leak tests` workflow (`containment-leak.yml`), started by hand and weekly; it prints the runner's kernel release and `nft` version. It records what that one runner does and says nothing about other kernels or `nft` versions. Neither is in the required checks: the first joins them after three green runs in a row and the owner's agreement, the second never does.

### systemd units

`systemd-analyze verify` checks that `ExecStart` exists, so CI puts a stub binary at `/usr/local/bin/domovoi` first. On a systemd machine that has no real `domovoi` installed:

```sh
printf '#!/bin/sh\n' | sudo tee /usr/local/bin/domovoi >/dev/null && sudo chmod 0755 /usr/local/bin/domovoi
systemd-analyze verify --man=no deploy/systemd/domovoi-hub.service deploy/systemd/domovoi-agent.service \
  deploy/systemd/domovoi-helper.socket deploy/systemd/domovoi-helper@.service \
  deploy/systemd/domovoi-helper-selftest.service deploy/systemd/domovoi-skill-runner.socket \
  deploy/systemd/domovoi-skill-runner@.service
sudo rm /usr/local/bin/domovoi
```

The helper service is a template (`domovoi-helper@.service`) and the agent unit wants the helper socket, so all seven units go in one run.

## Troubleshooting

- **Port 7420 or 7421 is in use.** Another hub or the dev hub is still running. Stop it, or set `DOMOVOI_WEB__BIND` / `DOMOVOI_AGENT_SERVER__BIND` to other addresses.
- **`error: a hub is already running on …`.** `hub token create`, `hub set-password` and similar commands need the hub stopped. Stop it, or use the web UI.
- **The hub refuses a database from an earlier development build.** The schema starts at one baseline migration, so a database created before it holds migration versions this build does not know. Delete the database file (and its `-wal` and `-shm` files) and let the hub create a new one.
- **Tests hang.** Install cargo-nextest: `.config/nextest.toml` kills a test after 2 minutes, while plain `cargo test` waits forever.
- **A web check says generated API types are stale.** Run the [generated files](#running-part-of-the-test-suite) commands, then commit the results.
