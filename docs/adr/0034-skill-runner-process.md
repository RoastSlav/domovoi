# 0034 The skill runner process: placement, protocol and rules

- Status: proposed
- Date: 2026-11-25
- Extends [0030](0030-three-identities.md) (the three identities; Decision 6, a missing runner never falls back to a local run) and [0031](0031-helper-and-skill-runner-delivery.md) (the runner's units and sandbox; Decision 3).
- Changes `xtask/src/arch/rules.rs` and the crate table of `ARCHITECTURE.md` §4, which is why it exists.

This record is **proposed**. It makes no statement of platform support. The unit text is the one measured with a stand-in program in 0031; its behaviour with the production runner is an exit criterion of the change, not a finding.

## Context

0030 and 0031 decided that skills run as the unprivileged `domovoi-skill` account, in a sandbox that systemd builds from a socket-activated unit. They left the program itself, the protocol between the agent and the runner, and the enforcement of the rules open. The agent already has two call sites that run skills, and its hub link already carries a `skill_runner_status`; the hub derives availability from it.

Adapters never depend on adapters (R1), so the runner cannot use anything the helper or the agent owns, and neither can the agent link the runner's server code.

## Decision

1. **Placement.** The server side lives in `domovoi-runner`, which already owns process spawning (R3), as the module `serve`. The client and the route that replaces direct skill spawning live in `domovoi-agent`, as the module `skill_runner`. The protocol types, the frame codec, the refusal codes and the socket path live in `domovoi-core`, as the module `skill_runner`, with no I/O. The `domovoi` binary is the composition root: its `skill-runner serve` subcommand adopts the connection systemd passed on fd 0 with the helper crate's std-only function and injects an already adopted stream, the host inspection port and a probe collector. No new internal edge is added; `domovoi-runner` does not depend on `domovoi-helper` or `domovoi-agent`.

2. **Protocol.** One request and at most one reply per connection, each a frame of a 4-byte big-endian length and UTF-8 JSON, with a version field in every request (`RUNNER_PROTOCOL = 1`) and unknown fields refused.
   - **One buffered reply.** The wire to the hub is a single `skill_result` message and the executor already buffers, so a streamed reply would add a frame type nothing reads.
   - **No `env`, no path, no user in the request.** The runner finds `<root>/<name>` itself, recomputes the implementation hash, validates the arguments again and derives the environment exactly as the agent's spawn code does. The roots arrive as `ExecStart=` arguments, rendered by the installer from `agent.toml`, because the runner cannot read `/etc/domovoi`; `health` returns them so that the agent notices a configuration edit made after the unit was rendered.
   - **Half-close is cancel.** The agent keeps its write side open until it has read the reply. End of file, a stray byte or an error ends the skill's process group (terminate, bounded wait, kill) and the runner exits 0. Every defined outcome (reply, refusal, timeout, cancel, vanished peer) exits 0, so no `failed` instance is left behind; only a defect exits non-zero. `RuntimeMaxSec=` is the backstop.
   - **Peer rule.** `run` is served to the uid of `domovoi-agent`, looked up by name at each request; `health` and `probe` are also served to root, because the installer and the self-test run as root. The socket mode and group, this peer check and the mount mask are three separate layers.
   - **The reply is untrusted data.** The agent parses it strictly, caps it, redacts it, and never lets its authority depend on it.

3. **`run_as` is wire-neutral.** The runner accepts a manifest `run_as` only when it is absent or `domovoi-skill`; the agent refuses first, before it connects. The refusal travels as the token `run_as_unsupported` at the start of the existing `exec` failure message, because adding a field or a failure code needs a wire revision, a UI vocabulary entry and an OpenAPI regeneration. `domovoi agent selftest` lists the affected skills. Carrying the reason to the hub is left to a later wire revision.

4. **The runner never falls back and never switches users.** An agent that does not run as root runs skills only through the runner, never in its own process; a runner that is not `ok` makes every skill unavailable. An agent that runs as root keeps the labelled legacy path.

5. **Units and installer.** The two unit files ship under `deploy/systemd/` with concrete default values; `install.sh` embeds the same text and renders the binary path and the two directories into the service unit, from `domovoi agent dirs`, which refuses any path a unit cannot hold safely. The installer creates the configured directories before it starts any socket, starts the runner socket only after the units are installed, the daemon reloaded and the self-test passed, runs the self-test once more with the socket up, and stops the socket again, naming the check, when that fails. `MemoryMax=`, `TasksMax=` and the other hardening candidates are not shipped: each needs a measurement under real skills first.

6. **Rules.** Each rule has a card in `rules.rs` and a test that feeds it a failing input. The unit rules reuse the parser of `xtask/src/arch/units.rs`; the checks live in `runner_rules.rs` (units and installer order) and `runner_scan.rs` (source scans).

   | Rule | What it checks | Evidence it rests on |
   |---|---|---|
   | `UNIT-1` | No `InaccessiblePaths=` entry sits at or under a `TemporaryFileSystem=` parent | 0031 Decision 3: the combination fails the start with `226/NAMESPACE` and `systemd-analyze verify` is silent |
   | `UNIT-2` (runner) | The runner unit has `Group=domovoi-skill` and no non-empty `SupplementaryGroups=` | 0030 Decision 3: the account carries the control |
   | `UNIT-3` (runner) | The runner unit carries every directive of 0031's unit with its value, no `SupplementaryGroups=` line, and bind sources equal to the two `ExecStart=` directories; no hardening candidate is required | 0031 Decision 3 |
   | `UNIT-5` (runner) | The runner socket exists, listens only on `/run/domovoi-skill-runner.sock`, and is `0660 root:domovoi-agent` | 0030 Decision 2 |
   | `UNIT-6` | No `BindPaths=` or `BindReadOnlyPaths=` entry names a `.sock` path or anything under `/run` | 0031 Decision 3: never bind a socket node |
   | `UNIT-7` | Covers `TemporaryFileSystem=` and both runner binds against the created set, as for the helper | 0031 Decision 3 |
   | `INSTALL-1` (runner) | `install.sh` reads the runner directories, creates them, installs both units and reloads before the self-test, and enables the runner socket only after the self-test | 0031 Decision 3 |
   | `RUNNER-1` | The literal `domovoi-skill-runner.sock` and the type `RunnerRequest` appear only in core's `skill_runner` module, the agent's `skill_runner` module, the runner's `serve` module, the binary's `skill-runner` command, testkit, tests and `deploy/` | 0030 Decision 8 |
   | `RUNNER-2` | The runner's `serve` module imports no `domovoi_helper` item, no helper client and no agent state type | 0030 Decision 8 |

   `ID-2` also allows the runner's `start_checks.rs` to call `supplementary_groups`, which is how ST-05 reads the account's groups at start.

## What the checker enforces

The rules above and the existing ones, on every pull request: the shipped units and every rendering the tests build for other directories, the installer's order, and the places that name the socket or the request type.

## What the checker cannot see

- That a mask or bind works on a host. `systemd-analyze verify` checks syntax only. `deploy/test/skill-runner.sh` measures it on a machine with systemd as PID 1 and cgroup v2, and the self-test (ST-07, ST-13) reads the effective view.
- That the rendered directories match `agent.toml` after a later edit. `health` returns the roots and the self-test reports `unit_modified` naming `bind_paths`.

## Unproven (residual items the owner accepts)

- The runner behind the real unit on the measured hosts, and whether a hosted CI image can run the privileged test at all.
- The behaviour at `MaxConnections=`; the agent holds at most eight runs at once.
- `RuntimeMaxSec=3720s` against a real one-hour skill, and the `/run` mask's effect on journald logging with the production binary.

## Consequences

- A migrated agent loses local skill execution until the runner socket is up. The hub already shows the skills as unavailable.
- Starter skills that need root or the `docker` group (`restart_container`, `restart_unit`, `vacuum_journal`, the journal readers) report errors until the helper has operations for them.
- A manifest `run_as` other than `domovoi-skill` stops working on a migrated agent.
- Package skills stay non-executable; the runner implements the `package` source and nothing sends it yet.

## Alternatives considered

- **A user namespace sandbox built by the runner.** Rejected: Ubuntu 24.04 and later restrict user namespaces for unprivileged accounts; systemd builds the view from the unit.
- **A runner that reads `agent.toml`.** Rejected: the mask hides `/etc/domovoi`, and the request would have to carry a path.
- **`Accept=no` with one long-lived runner.** Rejected (0031): it keeps skill state between requests and loses the per-connection sandbox.
- **Streaming the reply.** Rejected: nothing consumes partial output.
- **Falling back to a local run when the runner is down.** Rejected (0030 Decision 6): package code would reach the helper and agent state.
