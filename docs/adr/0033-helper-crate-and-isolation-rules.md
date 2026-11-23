# 0033 The helper crate and the isolation rules

- Status: proposed
- Date: 2026-11-23
- Extends [0030](0030-three-identities.md) (the three identities; this record decides its open question A2) and [0031](0031-helper-and-skill-runner-delivery.md) (units, tiers and self-test checks; this record turns its proposed rules into enforced ones).
- Changes `xtask/src/arch/rules.rs` and the crate table of `ARCHITECTURE.md` §4, which is why it exists.

This record is **proposed**. It makes no statement of platform support: the published list of supported platforms is an empty constant.

## Context

ADR 0030 left open where the root helper code lives (A2) and proposed a set of `arch-check` rules without implementing them. ADR 0031 proposed unit rules and the self-test checks. The first milestone of the helper puts the accounts, the directories, a socket-activated helper skeleton and the self-test on a host, so both questions have to be answered now.

The helper is a root process. The agent is a network program with a hub link, a spool and a package service. A root process must not link that code. Adapters may not depend on each other (R1), so the agent cannot reuse a helper adapter and the helper cannot reuse the agent's atomic-write code. The skill runner, which comes later, needs the same host inspection port, and the binary can inject it.

## Decision

1. **A new crate, `domovoi-helper`, with edges to `domovoi-core` and `domovoi-config` only.** It holds the helper runtime (adopting the connection from fd 0, the peer check, the closed request dispatch), the start checks, the repair command, the state lock and atomic write, the real host inspection adapter and the isolation self-test. Pure types, the tier function and the check evaluators live in `domovoi-core` (no I/O). The agent gets a small client module and a state-ownership module. The `domovoi` binary is the composition root: it gains the edge to `domovoi-helper` and injects the process executor (R3, R6). `ARCHITECTURE.md` §4 gains one row and the binary one edge. The external allowlist of the crate is `async-trait`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `uuid`, `jiff` and `nix`; no new workspace dependency is added and there is no `unsafe`.

2. **Host paths are described by `HostDirs`, not by an agent-specific directory type in core.** The path table (who owns which directory, with which mode) is one list in `domovoi-core`. The installer, the start check, the self-test and the repair command all read that one list, so they cannot disagree.

3. **The helper spawns nothing in this milestone.** Only `domovoi-runner` spawns processes (R3, R4, unchanged). The self-test reads systemd state through the injected `ProcessExecutor`, and the child that runs the agent-side checks as the agent account goes through the same executor with the existing `run_as` mechanism. **Series rule for later work:** a later change may let the helper spawn only the fixed programs it names, only through the injected `ProcessExecutor`, never a shell and never from a path that arrived in a request. Each such change states this in its own record.

4. **The identity migration is explicit.** A routine rerun of the installer never moves an existing single-account install to the three-identities layout, because the migration removes root access from checks the owner may rely on. It needs the flag `--migrate-identity`, a printed plan and a typed confirmation at a terminal, and it can be rolled back with `--rollback-identity`. Until then the host is labelled "privilege boundary not in effect".

5. **A wrong directory, key or state mode refuses start and is never repaired at start.** The only repair is the owner-confirmed local command `domovoi helper repair` (ADR 0031 Decision 6).

6. **Rules.** Each rule has a card in `rules.rs` and a test that feeds it a failing input. `UNIT-1`, `UNIT-6`, `RUNNER-1` and `RUNNER-2` and the runner clauses of `UNIT-2`, `UNIT-3` and `UNIT-5` belong to the skill-runner milestone.

   | Rule | What it checks | Evidence it rests on |
   |---|---|---|
   | `HELPER-1` | The helper socket path, the helper request types and the client appear only in the core helper module, the helper crate, the agent's client module (and the one probe call in its runtime), the binary's helper command, the testkit, tests and `deploy/`; never in the hub, engine, store, llm, chat, checks, config, proto, runner or the rest of the agent | ADR 0030 Decision 8 |
   | `HELPER-2` | `/var/lib/domovoi-helper`, `attest.key`, `state.lock` and `platform.json` appear only in the helper crate, the core path table, tests, `deploy/`, `install.sh` and docs | ADR 0030 Decision 8 |
   | `IDENT-1` | The literal `"domovoi-skill"` appears in non-test Rust only in the core accounts module | ADR 0030 Decision 3 |
   | `ID-2` | The account names and the group-membership calls live in the core identity module and in the helper's inspection, self-test and start-check modules; no numeric uid or gid other than 0 sits next to an account constant | ADR 0030 Decision 4; accounts created by hand with other ids on four hosts |
   | `ID-3` | No code in the helper, the agent's helper client or the runner parses or compares a systemd instance name; the one allowed literal is the self-test instance constant | ADR 0030 Decision 8: the instance name differs between systemd builds (255.4 and 257.13 against 259.5 and 259.9) |
   | `UNIT-2` (docker clause) | No `Group=` or `SupplementaryGroups=` of a shipped unit contains `docker` | ADR 0031 Decision 9: membership in `docker` is root |
   | `UNIT-3` (helper) | `domovoi-helper@.service` carries each directive of the measured baseline with its value, and has no `User=` and no `SupplementaryGroups=` | ADR 0031 Decision 5; unit measured on Ubuntu 24.04 with a stand-in |
   | `UNIT-4` | A unit with `RuntimeMaxSec=` has `Type=simple` | ADR 0031 Decision 2: `oneshot` ignored the limit on three hosts |
   | `UNIT-5` | Every shipped socket unit has `SocketMode=0660`, `SocketUser=root`, `SocketGroup=domovoi-agent` and a `ListenStream` from the known set | ADR 0030 Decision 2: a 0666 socket admitted an unrelated account on four hosts |
   | `UNIT-7` | Every path in a plain `InaccessiblePaths=`, `BindReadOnlyPaths=`, `TemporaryFileSystem=` or `ReadWritePaths=` is in the installer's created set or is exactly `/var/lib` or `/run`; a `-` prefix on a created path fails | ADR 0031 Decision 3: a plain absent path fails the start, a `-` prefix fails open |
   | `UNIT-8` | `domovoi-agent.service` has `User=domovoi-agent`; no compose file mounts `docker.sock` read-write | ADR 0031 Decision 9 |
   | `UNIT-9` | No `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` of a shipped unit has a path under `/run`, `/tmp` or `/dev/shm` | ADR 0031: `/run` was `noexec` on Ubuntu 24.04 (`203/EXEC`) |
   | `INSTALL-1` | In `install.sh`, the line that creates each path of the created set comes before the first `systemctl enable` or `systemctl start` | ADR 0031 Decision 3: `TemporaryFileSystem=` on an absent path creates the directory on the host |
   | `SELFTEST-1` | Every check id, check token and refusal token in an `ALL` constant is named in a `covers:` comment above a test | ADR 0031 Decision 7 |

   The unit and install-script rules read `deploy/systemd/` and `install.sh` with small parsers in `xtask`. A gate test runs all unit rules over the real files.

## What the checker enforces

The rules above and the existing R1 to R14. A shipped unit, the installer's order and the places that name the helper socket or the accounts are checked on every pull request.

## What the checker cannot see

- Whether a host really has the accounts, modes and units the files describe. Only the self-test on that host sees it, and only when it runs.
- Whether a distribution drop-in changed a unit (reported by the self-test as `unit_modified`).
- Whether the helper behaves on a given kernel, filesystem or systemd build beyond those measured.

## Unproven (residual items the owner accepts)

These are carried from ADR 0030 and ADR 0031 and stay open after this record:

- A real-installer pass with the production helper, runner and installer on the measured hosts. A platform is promoted to Supported only after it. Nothing is Supported.
- The key-custody test with the production key (the permission half is tested here, the key comes with the attestation work).
- The helper directive set per operation group (ADR 0031 B5). The measured baseline ships unchanged.
- Systemd 252 and 249, SELinux, other filesystems for `flock` and atomic rename, power-loss durability, `ENOSPC`.
- The behaviour at `MaxConnections`, and the reload-pending case of the unit check.
- The `/run/systemd` listing under the mask on systemd 257.13 and 259.9.
- Containers, Unraid and hosts without systemd: they keep today's agent, labelled, and are not touched.

## Consequences

- One more crate to build and test, in exchange for a root process that links no network code.
- A host that was installed before this change stays on the legacy layout and is labelled until the owner migrates it.
- Until the skill runner ships, every host is Blocked for skills by construction. That is the correct result, because no runner exists.
- The helper offers read groups only and answers every one of them as unavailable in this milestone.

## Alternatives considered

- **Modules of `domovoi-agent`.** Rejected: it links the agent's network stack into a root process and puts the helper beside package handling.
- **Migrate on a routine rerun of the installer.** Rejected: it removes root access from checks the owner may rely on, without being asked.
- **Repair a wrong mode at start.** Rejected (ADR 0031): it hides a possibly compromised directory.
- **Let the helper run `systemctl` and `nft` now.** Rejected: nothing in this milestone needs it, and each later program needs its own measured unit set.
