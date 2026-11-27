# 0031. Helper and skill-runner delivery and isolation

- Status: Accepted
- Date: 2026-11-22

## Context

[0030](0030-three-identities.md) decides which accounts exist and what each may reach. This record decides how the root helper and the skill runner are started and sandboxed, and how a host proves at run time that the sandbox is the one the design needs. The shipped unit files are in `deploy/systemd/`; they are the authority for exact text.

Behaviours of systemd that shaped the units, all measured with stand-in programs in real units on four hosts (Debian 13, Ubuntu 24.04 and 26.04, WSL 2; systemd 255 to 259):

- `TemporaryFileSystem=` on a parent combined with `InaccessiblePaths=` on a child of it fails to start (`226/NAMESPACE`) and `systemd-analyze verify` is silent. Mask the parent and bind back what is needed.
- A plain `InaccessiblePaths=` or `BindReadOnlyPaths=` on an absent path fails the start, which is fail closed. A `-` prefix skips it, which fails open. `TemporaryFileSystem=` on an absent path never fails: it creates an empty directory on the host.
- An empty `SupplementaryGroups=` does not drop an account's groups.
- `ProtectSystem=strict` leaves `/home`, `/root` and `/run/user/<uid>` writable unless `ProtectHome=yes` is set.
- `RuntimeMaxSec=` is ignored with `Type=oneshot`; `Type=simple` honours it.
- With `Accept=yes`, closing the client does not stop the instance, and the instance name is an opaque string whose format differs between builds.
- `/run` may be mounted `noexec`.
- Re-binding one socket into a masked `/run` makes it visible again; a mask beats a group grant.

## Decision

1. **Two socket units** (`domovoi-helper.socket` at `/run/domovoi-helper.sock`, `domovoi-skill-runner.socket` at `/run/domovoi-skill-runner.sock`), each `SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660`, `RemoveOnStop=yes`, with `MaxConnections=8` for the helper and 16 for the runner.

2. **`Accept=yes`.** One instance per connection gives one sandbox, one `RuntimeMaxSec` and one cgroup per execution. The helper follows: it holds `flock` across read, recompute, write and sign, so concurrent instances serialise and no in-memory state outlives a request. The runner implements the cancel:
   - It spawns the skill through `domovoi-runner` in its own process group and watches fd 0 for end of file while the child runs.
   - On end of file it terminates the skill's group, waits a bounded time, kills it, collects the child and exits 0. It enforces the request's timeout with the same group kill.
   - It exits 0 after every defined outcome (reply, refusal, timeout, cancel), because a non-zero exit leaves a `failed` instance. The cgroup kill is the guarantee for grandchildren that left the group.
   - `RuntimeMaxSec=` is the backstop and requires `Type=simple`.

3. **The skill-runner unit** (`domovoi-skill-runner@.service`) runs as `User=domovoi-skill`, `Group=domovoi-skill`, with no `SupplementaryGroups=` line, `StandardInput=socket`, `NoNewPrivileges=yes`, an empty `CapabilityBoundingSet=`, `RestrictSUIDSGID=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes`, `ProtectProc=invisible`, `ProcSubset=all` (so `/proc/meminfo`, `uptime`, `loadavg` and `mounts` exist; `ProtectProc=invisible` still hides other users' processes), `TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro` and `BindReadOnlyPaths=` for the skills and packages directories. The effect is that the runner sees only the skills and packages trees, read-only: both sockets, the agent and helper state, the key and `helper.toml` are `ENOENT`, even where a group would allow a connection. The rules that follow are enforced by `UNIT-*` ([0033](0033-helper-crate-and-isolation-rules.md), [0034](0034-skill-runner-process.md)):
   - Mask the parent, bind back what is needed, never put a child mask under a masked parent.
   - Never bind a `.sock` and never re-expose a socket.
   - No `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` under `/run`, `/tmp` or `/dev/shm`.
   - The installer creates every directory a mask or bind names, before any socket starts.
   - Per-package narrowing is not offered: under `Accept=yes` the instance name cannot carry a package, and starting a unit per package would be a privileged action the helper must not offer generically. The runner sees the owner's skills and the installed packages as one read-only tree.
   - `MemoryMax=`, `TasksMax=` and further hardening directives are not shipped until measured under real skills.

4. **The helper unit** (`domovoi-helper@.service`) runs as root with `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/domovoi-helper`, `ProtectHome=yes`, `PrivateTmp=yes`, `RuntimeMaxSec=60s` and the device and capability set of [0036](0036-helper-read-operations.md), and is ordered after `nftables.service`. The long-lived helper never creates a namespace ([0038](0038-containment-boundary.md)). The helper never runs `daemon-reload` and refuses a unit with `NeedDaemonReload=yes`.

5. **Accounts and modes.** `/etc/domovoi` is `root:domovoi-agent 0750`; `/etc/domovoi/skills` is root-owned and not writable by anyone else; `/etc/domovoi/helper.toml` is `root:root 0600`; `/var/lib/domovoi-agent` is `domovoi-agent 0700`; `/var/lib/domovoi-helper` and its `attest.key`, `state` and `state.lock` are `root` 0700 and 0600 regular files; `/var/lib/domovoi-packages` is `domovoi-agent`; the sockets are `root:domovoi-agent 0660`. The installer creates the accounts (confirming with `getent` that `domovoi-skill` is in no other group), then the directories, then runs `domovoi helper init`, installs the units with no drop-ins and every `ExecStart=` under a fixed binary path, enables the sockets, runs `domovoi agent selftest --installer`, and only then starts the agent.
   - **A wrong directory, key or state mode is refused, never repaired at start.** The helper start check and the self-test open each path with `O_NOFOLLOW` and report every offending path (`helper_state_permissions`, `path_mode_wrong`). Repair is the separate owner-confirmed local command `domovoi helper repair`, which prints the plan, changes nothing without confirmation at the terminal and is reachable from neither the agent, the hub, chat nor a policy.
   - State writes are temp file, `fsync`, `rename`, `fsync` of the directory, under `flock`.

6. **Platform tiers replace a version floor.** The gate is a run-time isolation check, not a systemd version.

   | Tier | Qualifies | Skills | Helper read groups | Helper write groups |
   | --- | --- | --- | --- | --- |
   | Supported | on the published list after a real-installer pass, and the gate passes | allowed | allowed | allowed once their own gates pass |
   | Untested | a systemd host not on the list whose gate passes | allowed after a recorded acknowledgement and with a persistent "untested platform" notice | allowed | refused unless the owner opts in per host in `helper.toml` (off by default, audited) |
   | Blocked | any check fails, no systemd, or a container | refused (`skill_runner_status` is `unsupported` or `error`; never a fallback to the agent uid) | only if the helper's own checks pass | refused |

   The published list of supported platforms is a constant that is empty until a platform has passed a real-installer pass.

7. **Self-test and start checks** (`ST-01` to `ST-20`). Start checks run inside the helper and the runner at every start and fail closed. Self-test checks run from the agent (`domovoi agent selftest`) or as root (`--installer`). Results are `pass`, `fail`, `warn`, `info` and `nd` (not run on this host: never a pass and never satisfies a gate that requires the check). The checks cover the agent's uid and groups, the skill account, the Docker-group rule, the runner's groups and privileges, the units (`systemctl show` through an instance name: `DropInPaths` empty, `SocketMode=0660`, `Type=simple`, `RuntimeMaxUSec` equal to the rendered value), the path and mode table, the helper state, the socket nodes, the runner's effective isolation view from inside the real unit (`ST-13`), the agent's inability to reach the helper state or the Docker socket, the container install surface and the state write cost. `ST-13` is the one that proves isolation: a directive in a file proves nothing, only the effective view answers, and it tells `ENOENT` from `EACCES` (the latter is a failed mask). A local drop-in that removes the mask makes skills stop working, never leak. Every check id has a named test (`SELFTEST-1`).

8. **Containers and hosts without systemd** get no skill runner and no helper. An agent there keeps the local path, labelled "privilege boundary not in effect", with no unattended execution. A container agent reports `in_container`; the hub labels the host `container_agent` and refuses a capability that needs the helper or runner with `agent_in_container`. Reads through the `/host` mounts and the read-only Docker socket proxy are unchanged.

9. **Docker.** No Domovoi account is in the `docker` group and neither the agent unit nor an agent container gets a read-write `docker.sock`: membership or a mounted socket is root. Any Docker write for the helper needs its own narrow first-party design and ADR; `container.restart` stays blocked for unattended use until it exists.

10. **Sandbox mechanism.** systemd builds the runner's sandbox from the unit; no user namespaces are created at launch. No AppArmor or SELinux profile ships: the design holds by file permissions and namespaces alone. Yama is reported, not required.

## Consequences

- A host proves its isolation at run time. A distribution drop-in, a changed mount layout or a missing mask shows up as a failed check, and skills stop rather than leak.
- The checker reads shipped files; it cannot see a deployed host. Only `ST-07` and `ST-13` see that, and only when they run.
- The design was measured on systemd 255 to 259 and on Debian and Ubuntu. Systemd 252 and older, SELinux hosts, other filesystems for `flock` and atomic rename, power-loss durability and non-systemd hosts were not measured.
- Nothing is Supported until a real-installer pass has run on that platform.

## Alternatives considered

- A transient unit per execution (`systemd-run`): needs privilege and puts a command-execution path behind the socket.
- `Accept=no`: a shared sandbox, a crash affects every run and per-run limits exist only in the process.
- A hard systemd version floor: a host below a candidate floor of systemd 257 passed every effective-isolation check.
- `/etc/domovoi` at 0755 or `skills_dir` outside it: not chosen.
- `chmod` and continue at start on a wrong mode: hides a possibly compromised directory.
- Binding a helper socket into the runner's namespace: makes the helper reachable by package code.
