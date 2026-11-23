# 0030 Three identities: control agent, skill runner, root helper

- Status: proposed
- Date: 2026-11-22
- Extends [0007](0007-human-only-approvals.md), [0021](0021-plan-hash-v2-and-authorization.md), [0022](0022-single-execution-boundary.md), [0028](0028-one-execution-path.md) and [0029](0029-package-relay-boundary.md) with an operating-system boundary: who may reach what on one host.
- Siblings: [0031](0031-helper-and-skill-runner-delivery.md) (how the two privileged services are delivered and sandboxed, including the unit files and the self-test checks `ST-nn` used below) and [0032](0032-helper-safety-attestation.md) (the signed safety report).

This record is **proposed**. It does not settle the open questions on helper delivery, skill-runner isolation or safety attestation, and it makes no statement of platform support. The decisions below are taken; they rest on stand-in programs and probe accounts on four hosts, not on the production helper, the production runner or the installer, none of which exists. Acceptance needs the items listed under "What acceptance still needs".

## Evidence used

Every measurement below was made with stand-in programs (a root process holding a key file, small scripts for the runner) and probe accounts, in real systemd units, on these hosts. "Demonstrated" means seen as stated on the hosts named; it is not a statement about production code.

| Host | OS and systemd | Kernel, arch | Notes |
|---|---|---|---|
| Raspberry Pi 5 | Debian 13, systemd 257.13 | 6.18 (Raspberry Pi), aarch64 | accounts with the final names `domovoi-agent` and `domovoi-skill`; no AppArmor, no Yama |
| Desktop | Ubuntu 26.04, systemd 259.9 | 7.0, x86_64 | AppArmor and Yama active |
| Desktop | Ubuntu 24.04.5, systemd 255.4 | 6.8, x86_64 | AppArmor and Yama active; `/run` mounted `noexec`; first run of the unit files of 0031 as written |
| WSL 2 | Ubuntu 26.04.1, systemd 259.5 | 6.18 (Microsoft), x86_64 | research environment, two runs: the first at user scope, the second at system scope with real disposable accounts |

Five runs in total on four hosts; two of the runs share one WSL machine, so independent hosts number four. Where a result needs host independence this record says "four hosts". Probe accounts were disposable system accounts with a primary group only, except where a test added a group on purpose.

## Context

Today the agent unit has no `User=` and runs as root (`deploy/systemd/domovoi-agent.service`; `install.sh`), and a skill's `run_as` is switched to by the runner only when the agent is root. Packages (0029) will put signed code on the host. If packages and skills ran as the same uid that a privileged helper accepts, any package could call the helper directly and skip plan hash, approval, ceiling and rate limit. A peer-uid check tells users apart, not processes (RW-39 in the architecture-decisions plan). The design therefore uses three operating-system identities, and keeps the layers that bound them distinct.

Facts checked against the repository:

- No `helper` subcommand, no helper or skill-runner unit, and no use of `SO_PEERCRED` or `LISTEN_FDS` exist under `crates/`. The only matches for `helper_identity` are the optional announce field in `domovoi-agent/src/link/handshake.rs`, set to `None`.
- The user is spelled `domovoi-skills` in **five** places in the code: the doc comment `crates/domovoi-core/src/skill/manifest.rs:102`, the tests `crates/domovoi-core/tests/manifests.rs:16` and `crates/domovoi-runner/tests/runner/skill_run.rs:16,74`, and the snapshot `crates/domovoi-core/tests/snapshots/manifests__skill_manifest_json_snapshot.snap:53`. The planning text says `domovoi-skill`.
- `Unavailable(run_as_unsupported)` does not exist in `crates/` yet. It is a proposed reason.

## Decision

1. **Three identities.**

   | Component | OS user and groups | Owns | Reaches | Must not reach |
   |---|---|---|---|---|
   | control agent (`domovoi agent run`) | `domovoi-agent`; supplementary groups exactly `systemd-journal`; never `docker` | its `state_dir` (0700): agent key, spool, journal; `packages_dir` (writes) | hub link; helper socket; skill-runner socket | helper state and key; `/etc/domovoi/helper.toml`; skill sources for writing |
   | skill runner (`domovoi skill-runner`) | `domovoi-skill`, **no supplementary groups of any kind** | nothing of Domovoi's | skill and package files read-only; a private tmp | agent state; helper state and key; both sockets; `/etc/domovoi` except `skills/` |
   | helper (`domovoi helper serve`) | root | `/var/lib/domovoi-helper` (0700): `attest.key`, `state`, `state.lock`; `/etc/domovoi/helper.toml` | allowlisted privileged operations | executing arbitrary commands, `run_as`, `sudo`, `systemd-run` |

   How each "must not reach" is enforced differs by row. The agent is kept from the helper state and key by file permissions alone (a root-owned 0700 directory and 0600 files). Demonstrated: with that layout, two unprivileged accounts were refused list, read, write, copy, create, rename, link, chmod, unlink and `flock` on the directory, key, state and lock on all four hosts; `/proc/<helper pid>/{environ,mem,maps,fd,exe,cwd}`, `ptrace` and `process_vm_readv` against the root stand-in were refused to non-root accounts on all four hosts, with `gdb -p` also refused on Ubuntu 24.04. The Raspberry Pi has no Yama and refused too, so the cross-uid denial is plain file and process permission; whether Yama adds anything on hosts that have it was not separated. A mask in the agent unit would also remove the agent's own sockets, so the earlier wording of the planning text, "masked in the agent unit", is wrong and is withdrawn. The skill runner is kept out by file permissions and by the mask in its unit (0031).

2. **Three separate layers decide who may connect to which socket.** They are distinct and none substitutes for another:
   - *Socket mode and group*: `SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660` on both sockets. Demonstrated on all five runs: root and group members connected, any other account got `EACCES` at connect and never reached the service. A `0666` socket admitted an unrelated account on four hosts and is not protected by any unit directive, so `0660` is a hard maximum.
   - *`SO_PEERCRED`*: the service checks the peer uid. Demonstrated on all five runs: the kernel reports the real uid and gid of every peer that does connect, and `unshare -U` does not change it (`unshare -Ur` was blocked for unprivileged accounts by AppArmor on the two AppArmor hosts and worked on the Raspberry Pi). With a stand-in uid check on the real socket path, the agent was allowed and the skill account, once given the group and with no mask, was denied by uid. It separates users and is not the isolation: a compromised `domovoi-agent` passes it.
   - *Mount-namespace masking in the skill runner's unit*: the sockets and the state directories are invisible to `domovoi-skill`, even when a group would otherwise allow a connection. Demonstrated on all five runs: with the mask, both 0660 sockets gave `ENOENT` although the group was granted. The directives are in 0031.

3. **`run_as`.** The only user a skill may name is `domovoi-skill`. The skill runner refuses any other value; the agent reports `Unavailable(run_as_unsupported)` for such a skill; `domovoi agent selftest` lists the affected skills. There is no per-skill user switching and no `sudo` inside skills: every privileged effect goes through the typed helper. The spelling is decided: `domovoi-skill`, with the five code sites above edited in one commit and the snapshot reviewed with `cargo insta review`. That edit is made by the implementation, not by this record.

4. **`domovoi-skill` has no supplementary groups, enforced by the account and checked at start.** `SupplementaryGroups=` empty does not drop an account's own groups. Demonstrated on all five runs: a unit for an account with a stray group kept it under an empty directive (group list `[stray, primary]`, and a group named by the unit was added on top), while a clean account (primary group only) gave a list of its primary gid only. The agent unit without the directive got the account's groups (the agent group plus `systemd-journal` on the Raspberry Pi). So the control is the account, not the unit:
   - the installer creates `domovoi-skill` with `useradd --system --user-group --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin` and never adds it to any other group, in particular not `docker`, `domovoi-agent`, `systemd-journal`, `sudo` or `adm`; no Domovoi account is a member of `docker`, because that membership is root (0031, Decision 9);
   - numeric ids are never hard-coded; `useradd --system` allocates them. Ids seen: 999/984 and 997/983 on the Raspberry Pi, 995/994 on both Ubuntu desktops, 999/997 on WSL 2;
   - the runner's start check (`ST-05` in 0031) reads `getgroups()` and fails closed unless it is exactly the primary gid, and `domovoi agent selftest` checks the accounts (`ST-02` to `ST-04`) and the Docker-group rule; each failure reaches `skill_runner_status` with a reason. The check was seen to separate a clean account from a polluted one in real units.

   The accounts used were probe accounts with disposable names and, on the Raspberry Pi, accounts named `domovoi-agent` and `domovoi-skill`; none was created by an installer, and the production start check has not run.

5. **The helper allowlist bounds operations; it is not execution authorization.** The helper has no verifiable authorization input in v1. Plan hash, `allow_risk`, ceiling and rate limiter live in the agent and the hub. A compromised control agent can call the helper directly and is then bounded only by the allowlist and the helper-enforced safety rules (RW-39, residual risk). This record does not change that.

6. **Failure does not fall back.** With the skill runner absent, every skill is unavailable. It never runs as `domovoi-agent`, because that would reopen the helper and the agent state to package code. With the helper absent, helper capabilities are unavailable.

7. **Adopting an inherited socket: std only, from fd 0.** Both unit shapes are `Accept=yes` with `StandardInput=socket`, so the connection is the process's standard input and `LISTEN_FDS` is 0. The process adopts it with `Stdin::as_fd().try_clone_to_owned()` into a `UnixStream`, refuses a non-zero `LISTEN_FDS`, and never resolves a path from `LISTEN_*`. This needs no `unsafe`, no new dependency and no exception to R7 or R11. A non-socket on fd 0 fails on `local_addr()` with `ENOTSOCK`, which is a free validation. The skill runner spawns children with a cleared environment and a non-inherited stdin, so a skill never inherits `LISTEN_*` or the connection; a test must pin that. `listenfd` stays the documented fallback only if fd 3 or `Accept=no` is ever needed (one entry under `[workspace.dependencies]`, behind the application's own strict `LISTEN_PID` check). The mechanism was measured with `systemd-socket-activate --inetd` and the unit shape in a real unit on Ubuntu 24.04 with a stand-in, one run; the adoption code itself has not run in a real unit. The wrapper crates `sd-listen-fds`, `libsystemd`, `systemd`, `sd-notify` and `tokio-listener` were evaluated on paper and rejected (Alternatives).

8. **Proposed `arch-check` rules** (not implemented; each needs the implementation to add it with a card and a gate test, and `xtask/src/arch/rules.rs` changes need an ADR):
   - `HELPER-1`: the helper socket path and the construction of any helper request value appear in one agent module that builds them from an authorized value, and in the helper's own code; the hub crates, the engine and the skill runner never name them.
   - `HELPER-2`: nothing outside the helper's code touches `/var/lib/domovoi-helper`, `attest.key` or `state.lock`.
   - `RUNNER-1`: the skill-runner socket path and the runner request type appear in one agent module and in the skill runner's own code.
   - `RUNNER-2`: the skill runner imports no helper client and no agent state type.
   - `IDENT-1`: the literal `domovoi-skill` is the only accepted `run_as` and appears in one constant.
   - `ID-2`: the account names and the group-membership check live in one module, with no numeric uid or gid literal next to them.
   - `ID-3`: no code parses, builds or compares a systemd instance name. Measured: the instance name of an `Accept=yes` connection has the form `<counter>-<pid>-<uid>` on systemd 255.4 and 257.13 and a longer form with extra numeric fields on 259.5 and 259.9, so the format differs between builds and nothing may depend on it.
   - `UNIT-2` and `UNIT-8` (defined in 0031): no non-empty `SupplementaryGroups=` in the runner unit and no `docker` in any `Group=`; the agent unit has `User=domovoi-agent` and no file binds `docker.sock` read-write into the agent. They back this record's identity table and are listed here so the identity rules are in one place.

   Placement of the helper and runner code (new crates, or modules of `domovoi-agent` and `domovoi-runner`) changes the crate edges of `ARCHITECTURE.md` §4 and needs its own ADR (R1). Both processes are subcommands of the `domovoi` binary in RW-39.

## What the checker enforces

Nothing in this record is enforced yet. Today only R3 and R4 apply to these processes (only `domovoi-runner` spawns processes, argv only), plus EXEC-2 for the existing relay and dispatch sites. The rules in Decision 8 are proposals. Until they exist, "the hub never names the helper socket" is a design statement, not a checked fact.

## What the checker cannot see

- Whether the unit files and the installer really create `domovoi-skill` without groups, with the modes of 0031 Decision 6. That is a property of the host, visible only to `selftest` and the runner's start check at run time.
- Whether the agent process is really started as `domovoi-agent`: a unit that omits `User=` is valid (`ST-01` reports it).
- Whether a socket is `0660` and `root:domovoi-agent` on a given host, whether the mask directives are in the deployed unit, and whether a distribution or a local drop-in has changed them (`ST-07`, `ST-12`, `ST-13`).
- Whether the peer uid check and the group check agree with what the maintainer believes once a second local user is added to `domovoi-agent`.

## Unproven

- **Production accounts and installer.** No account was created by an installer. The production `getgroups` check has not run.
- **Root-owned state against another uid.** Refusal of a second uid held on four hosts at the OS level with a stand-in helper; the production helper is not covered. A pre-existing directory with mode 0755 was accepted by the stand-in's initialisation on one run (the check logic that would refuse it was exercised once, on Ubuntu 24.04, without the harness), and a lock file the process could not open was reported as a state fault on one run; both are the reason 0031 Decision 6 refuses wrong modes at start. Durability across power loss is not demonstrated.
- **Key custody across the boundary.** At the OS level it held on four hosts and, with the attestation harness run behind real users, once on WSL 2. The test `helper_attestation_key_not_readable_by_agent_or_skill` still needs the real helper.
- **Platforms.** Four systemd builds (255.4, 257.13, 259.5, 259.9) agreed on the directive results, except a user-scope result on 259.5 that is not a version difference (0031, Decision 3); nothing was run on systemd 252 or 249, SELinux, containers or non-systemd hosts.
- **Fd 0 adoption in a real unit.** Only the unit shape was measured, with a stand-in, once.
- **Yama's contribution.** Cross-uid denial held on a host without Yama, so custody does not depend on it; independence on a Yama host was not separated.

## Open questions

| # | Question | Status |
|---|---|---|
| A1 | Spelling of the user | **Decided**: `domovoi-skill`; edit five code sites (Decision 3). |
| A2 | Where do the helper and the runner code live? | **Open.** New crates need an ADR for the edges and `rules.rs`; modules of existing crates put the root helper beside the agent. Not decided here; it needs the edge analysis of R1. |
| A3 | How is "no supplementary groups" enforced if a distribution adds the user to a group? | **Decided**: fail closed at start (Decision 4). |
| A4 | How does the process adopt an inherited socket without `unsafe`? | **Decided**: std only from fd 0 (Decision 7). |

The platform tiers, `Accept=` and the corrected unit text are in 0031; the signed report is in 0032.

## What acceptance still needs

Before this ADR is accepted:

- a real-installer pass with the production helper, runner and installer on the measured hosts, creating the real accounts and running `selftest --installer`; a platform is promoted to Supported only after it;
- the production `getgroups` start check and the fd 0 adoption running in a real unit;
- the key-custody test with the real helper binary;
- A2 decided in its own ADR;
- the remaining unproven items above reviewed, or explicitly accepted as residual.

## Follow-up edits

None of these is made by this change.

- The architecture-decisions plan, RW-39: fix the line citation `manifest.rs:79` (now `:102`); replace the skill-runner unit text with the corrected form of 0031; delete "`SupplementaryGroups=` empty" in favour of the account rule. The target-architecture text on the attestation key: root 0700 directory, 0600 key, masked in the skill-runner unit only.
- The five `domovoi-skills` sites in the code, in one commit with one reviewed snapshot (Decision 3).
- `ARCHITECTURE.md` §4 and `xtask/src/arch/rules.rs`: only after A2 and the rules in Decision 8 are accepted, with their own ADR.

## Alternatives considered

- **Run skills as `domovoi-agent` and rely on `SO_PEERCRED`.** Rejected in RW-39: a package could call the helper directly.
- **Per-skill users or `sudo` switching.** Rejected: it would put privileged effects outside the typed helper.
- **A wrapper crate for socket adoption** (`sd-listen-fds`, `libsystemd`, `systemd`, `sd-notify`, `tokio-listener`). Rejected: unsound, no safe owned fd, licence, or wrong layer. `listenfd` is the fallback only.
- **An exception to R7 for one module, or binding the process's own socket.** Rejected: the first needs a separate crate and costs more than a dependency; the second loses socket activation and the `Accept=yes` model.
- **Keep `domovoi-skills`.** Rejected: it edits planning documents and contradicts the decided `domovoi-skill` of the design.

## Revision (2026-11-22)

Brought up to date with the measurements on four hosts and the maintainer's decisions. Changed: the evidence is stated inline with its hosts and scope; Decision 1 states how each "must not reach" is enforced and withdraws "masked in the agent unit"; Decision 4 uses the real-account results and adds the installer command, the `docker` rule and the check ids; the user's spelling and the inherited-socket question (A1, A4) are decided and A3 is recorded as decided; code sites corrected from four to five; Decision 7 (socket adoption) added; rules `ID-2`, `ID-3`, `UNIT-2`, `UNIT-8` added to the proposed rules; Unproven rewritten (the claims that no real accounts were created and that every result came from one systemd version are no longer true); "What acceptance still needs" added. Status stays proposed.

## Revision (2026-11-23)

Open question A2 is decided: the helper code lives in a new crate, `domovoi-helper`, with edges to `domovoi-core` and `domovoi-config` only, and the rules of Decision 8 listed as proposed are given their enforced form in [0033](0033-helper-crate-and-isolation-rules.md). The runner's own rules (`RUNNER-1`, `RUNNER-2`) stay proposed. Status stays proposed.
