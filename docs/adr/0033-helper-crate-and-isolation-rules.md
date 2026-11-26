# 0033. The helper crate and the isolation rules

- Status: Accepted
- Date: 2026-11-23

## Context

The root helper is a root process. The agent is a network program with a hub link, a spool and a package service. A root process must not link that code. Adapters may not depend on each other (R1), so the agent cannot reuse a helper adapter and the helper cannot reuse the agent's atomic-write code. The identities are in [0030](0030-three-identities.md) and the units and self-test checks in [0031](0031-helper-and-skill-runner-delivery.md); this record places the helper's code and turns the isolation requirements into `arch-check` rules.

## Decision

1. **A crate, `domovoi-helper`, with edges to `domovoi-core` and `domovoi-config` only.** It holds the helper runtime (adopting the connection from fd 0, the peer check, the closed request dispatch), the start checks, the repair command, the state lock and atomic write, the host inspection adapter, the isolation self-test and the safety attestation. Pure types, the tier function and the check evaluators live in `domovoi-core` and do no I/O. The agent has a small client module and a state-ownership module. The `domovoi` binary is the composition root: it has the edge to `domovoi-helper` and injects the process executor (R3, R6). The external allowlist of the crate is in `xtask/src/arch/rules.rs`; the crate has no `unsafe`.
2. **Host paths are one list.** Who owns which directory, with which mode, is one table in `domovoi-core` read by the installer, the start check, the self-test and the repair command, so they cannot disagree.
3. **The helper spawns only through the `ProcessExecutor` port.** Only `domovoi-runner` spawns processes (R3, R4). The helper reads systemd state and runs its fixed programs (`ip`, `smartctl`, `dmesg`, `ss`, `nft`) through the injected executor, never a shell and never from a path that arrived in a request ([0036](0036-helper-read-operations.md)). The self-test runs agent-side checks as the agent account through the same executor, started with `setpriv --reuid --regid --init-groups`, because the executor's `run_as` would leave the account without its supplementary groups.
4. **Existing single-account installs are not converted.** The installer sets up the three-identity layout on a fresh agent host and leaves a host with a hub, or without systemd, with the root agent layout, labelled "privilege boundary not in effect".
5. **A wrong directory, key or state mode refuses start and is never repaired at start.** The only repair is the owner-confirmed local command `domovoi helper repair`.
6. **Rules.** Each has a card in `xtask/src/arch/rules.rs` and a test that feeds it a failing input.

   | Rule | What it checks |
   | --- | --- |
   | `HELPER-1` | The helper socket path, the helper request types and the client appear only in the core helper module, the helper crate, the agent's client module (and its one probe call), the binary's helper command, the testkit, tests and `deploy/`; never in the hub, engine, store, llm, chat, checks, config, proto, runner or the rest of the agent |
   | `HELPER-2` | `/var/lib/domovoi-helper`, `attest.key`, `state.lock` and `platform.json` appear only in the helper crate, the core path table, tests, `deploy/`, `install.sh` and docs |
   | `IDENT-1` | The literal `"domovoi-skill"` appears in non-test Rust only in the core accounts module |
   | `ID-2` | The account names and the group-membership calls live in the core identity module and the helper's inspection, self-test and start-check modules (and the runner's start check); no numeric uid or gid other than 0 sits next to an account constant |
   | `ID-3` | No code in the helper, the agent's helper client or the runner parses or compares a systemd instance name; the one allowed literal is the self-test instance constant |
   | `UNIT-2` | No `Group=` or `SupplementaryGroups=` of a shipped unit contains `docker`; the runner unit has `Group=domovoi-skill` and no non-empty `SupplementaryGroups=` |
   | `UNIT-3` | `domovoi-helper@.service` carries each required directive with its value (the baseline of [0036](0036-helper-read-operations.md)) and has no `User=` and no `SupplementaryGroups=`; the runner unit carries the directives of [0031](0031-helper-and-skill-runner-delivery.md) |
   | `UNIT-4` | A unit with `RuntimeMaxSec=` has `Type=simple` |
   | `UNIT-5` | Every shipped socket unit has `SocketMode=0660`, `SocketUser=root`, `SocketGroup=domovoi-agent` and a `ListenStream` from the known set |
   | `UNIT-7` | Every path in a plain `InaccessiblePaths=`, `BindReadOnlyPaths=`, `TemporaryFileSystem=` or `ReadWritePaths=` is in the installer's created set or is exactly `/var/lib` or `/run`; a `-` prefix on a created path fails |
   | `UNIT-8` | `domovoi-agent.service` has `User=domovoi-agent`; no compose file mounts `docker.sock` read-write |
   | `UNIT-9` | No `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` of a shipped unit has a path under `/run`, `/tmp` or `/dev/shm` |
   | `INSTALL-1` | In `install.sh`, the line that creates each path of the created set comes before the first `systemctl enable` or `systemctl start` |
   | `SELFTEST-1` | Every check id, check token and refusal token in an `ALL` constant is named in a `covers:` comment above a test |

   `UNIT-1`, `UNIT-6`, `RUNNER-1` and `RUNNER-2` are in [0034](0034-skill-runner-process.md). The unit and install-script rules read `deploy/systemd/` and `install.sh` with small parsers in `xtask`, and a gate test runs them over the real files.

## Consequences

- One more crate to build and test, in exchange for a root process that links no network code.
- A shipped unit, the installer's order and the places that name the helper socket or the accounts are checked on every pull request.
- The checker cannot see whether a host really has the accounts, modes and units the files describe, whether a distribution drop-in changed a unit (the self-test reports `unit_modified`), or how the helper behaves on a kernel, filesystem or systemd build outside those measured. Containers, Unraid and hosts without systemd keep the labelled agent and are not touched.

## Alternatives considered

- Modules of `domovoi-agent`: links the agent's network stack into a root process and puts the helper beside package handling.
- Converting an existing root agent in place: the project has no released installs to convert.
- Repairing a wrong mode at start: hides a possibly compromised directory.
