# 0030. Three identities: control agent, skill runner, root helper

- Status: Accepted
- Date: 2026-11-22

## Context

If the agent, the skills it runs and a privileged helper all ran as one uid, any skill or package could call the helper directly and skip the plan hash, the approval, the ceiling and the rate limit. A peer-uid check tells users apart, not processes. The design therefore uses three operating-system identities and keeps the layers that bound them distinct. Delivery and sandboxing are in [0031](0031-helper-and-skill-runner-delivery.md); the signed safety report is in [0032](0032-helper-safety-attestation.md).

## Decision

1. **Three identities.**

   | Component | OS user and groups | Owns | Must not reach |
   | --- | --- | --- | --- |
   | control agent (`domovoi agent run`) | `domovoi-agent`; supplementary groups exactly `systemd-journal`; never `docker` | its `state_dir` (0700: agent key, spool, journal) and `packages_dir` | helper state and key, `/etc/domovoi/helper.toml`, skill sources for writing |
   | skill runner (`domovoi skill-runner`) | `domovoi-skill`, no supplementary groups | nothing of Domovoi's | agent state, helper state and key, both sockets, `/etc/domovoi` except `skills/` |
   | helper (`domovoi helper serve`) | root | `/var/lib/domovoi-helper` (0700: `attest.key`, `state`, `state.lock`) and `/etc/domovoi/helper.toml` | nothing: it executes no arbitrary command, offers no `run_as`, `sudo` or `systemd-run` |

   The agent is kept from the helper's state and key by file permissions alone (a root-owned 0700 directory and 0600 files). The skill runner is kept out by file permissions and by the mount mask in its unit.

2. **Three separate layers decide who may connect to a socket.** None substitutes for another.
   - *Socket mode and group*: `SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660` on both sockets. `0660` is a hard maximum: a `0666` socket admits any account, and no unit directive protects it.
   - *`SO_PEERCRED`*: the service checks the peer uid. It separates users, not processes, so a compromised `domovoi-agent` passes it.
   - *Mount-namespace masking in the skill runner's unit*: the sockets and the state directories are invisible to `domovoi-skill`, even when a group would otherwise allow a connection.

3. **`run_as`.** The only user a skill may name is `domovoi-skill`. The runner refuses any other value, the agent reports `run_as_unsupported` for such a skill, and `domovoi agent selftest` lists the affected skills. There is no per-skill user switching and no `sudo` inside skills: every privileged effect goes through the typed helper.

4. **`domovoi-skill` has no supplementary groups, and the account enforces it.** An empty `SupplementaryGroups=` does not drop an account's own groups, so the control is the account, not the unit. The installer creates it with `useradd --system --user-group --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin` and adds it to no other group, in particular not `docker`, `domovoi-agent`, `systemd-journal`, `sudo` or `adm`. No Domovoi account is in the `docker` group, because that membership is root. Numeric ids are never hard-coded. The runner's start check reads `getgroups()` and fails closed unless it is exactly the primary gid; `domovoi agent selftest` checks the accounts and the Docker-group rule.

5. **The helper's allowlist bounds operations; it is not execution authorization.** Plan hash, `allow_risk`, the ceiling and the rate limiter live in the agent and the hub. A compromised control agent can call the helper directly and is then bounded only by the helper's allowlist and the safety rules the helper enforces itself.

6. **Failure does not fall back.** With the skill runner absent, every skill is unavailable; it never runs as `domovoi-agent`. With the helper absent, helper capabilities are unavailable.

7. **A connection is adopted from fd 0 with std only.** Both unit shapes use `Accept=yes` with `StandardInput=socket`, so the connection is the process's standard input and `LISTEN_FDS` is 0. The process adopts it with `Stdin::as_fd().try_clone_to_owned()` into a `UnixStream`, refuses a non-zero `LISTEN_FDS` and never resolves a path from `LISTEN_*`. This needs no `unsafe` and no dependency. The skill runner starts children with a cleared environment and a non-inherited stdin, so a skill never inherits the connection.

8. **Placement.** The helper is the crate `domovoi-helper`, with edges to `domovoi-core` and `domovoi-config` only ([0033](0033-helper-crate-and-isolation-rules.md)). The skill runner's server side is the `serve` module of `domovoi-runner` ([0034](0034-skill-runner-process.md)).

9. **Rules.** The rules that enforce this record are `HELPER-1`, `HELPER-2`, `IDENT-1`, `ID-2`, `ID-3` and the `UNIT-*` family ([0033](0033-helper-crate-and-isolation-rules.md)), and `RUNNER-1` and `RUNNER-2` ([0034](0034-skill-runner-process.md)). `ID-3` forbids parsing or comparing a systemd instance name, because its format differs between systemd builds.

## Consequences

- Package code cannot reach the helper: it runs as `domovoi-skill`, which cannot connect to the helper socket and cannot see its state.
- A host whose agent does not run as `domovoi-agent` with exactly the expected groups is labelled "privilege boundary not in effect", and an agent that runs as root keeps the labelled local path for skills.
- The checker cannot see whether a host really has the accounts, modes and units described. Only the run-time self-test sees that, and only when it runs ([0031](0031-helper-and-skill-runner-delivery.md)).
- The agent cannot read the helper's state on any host tested at the operating-system level; whether Yama adds protection on hosts that have it was not separated, so custody does not depend on it.

## Alternatives considered

- Run skills as `domovoi-agent` and rely on `SO_PEERCRED`: a package could call the helper directly.
- Per-skill users or `sudo` switching: privileged effects would sit outside the typed helper.
- A wrapper crate for socket adoption (`sd-listen-fds`, `libsystemd`, `systemd`, `sd-notify`, `tokio-listener`): unsound, no safe owned fd, licence or the wrong layer.
- An exception to R7 for one module: needs a separate crate and costs more than the std-only route.
