# 0031 Helper and skill-runner delivery and isolation

- Status: proposed
- Date: 2026-11-22
- Extends [0030](0030-three-identities.md) (the identities and what each may reach) with how the root helper and the skill runner are started and sandboxed on each kind of host. Builds on [0018](0018-detect-the-container-runtime.md) (container-runtime detection), [0028](0028-one-execution-path.md) and [0029](0029-package-relay-boundary.md).
- Sibling: [0032](0032-helper-safety-attestation.md).

This record is **proposed**. The open questions on helper delivery and skill-runner isolation stay open. The decisions below are taken; none states that a platform is supported. No helper, runner or installer exists.

## Evidence used

Five runs on four hosts, all with stand-in programs and probe accounts, in real systemd units. Two runs share one WSL machine, so independent hosts number four.

| Host | OS and systemd | Kernel, arch | Scope and notes |
|---|---|---|---|
| Raspberry Pi 5 | Debian 13, systemd 257.13 | 6.18 (Raspberry Pi), aarch64 | system scope; accounts with the final names; no AppArmor, no Yama; root filesystem ext4 on a USB 5400 rpm hard disk |
| Desktop | Ubuntu 26.04, systemd 259.9 | 7.0, x86_64 | system scope; AppArmor and Yama; ext4 on NVMe |
| Desktop | Ubuntu 24.04.5, systemd 255.4 | 6.8, x86_64 | system scope only; AppArmor and Yama; `/run` mounted `noexec`; ext4 on NVMe; the unit files below ran as written, with `ExecStart=` pointing at a Python stand-in and re-rooted paths |
| WSL 2 | Ubuntu 26.04.1, systemd 259.5 | 6.18 (Microsoft), x86_64 | two runs: the first at user scope, the second at system scope with real disposable accounts |

Evidence labels used in this record: **demonstrated** (seen as stated, on the hosts named, with stand-ins), **not demonstrated**, and **requires conventional Linux validation** (the result depends on a host property that was not measured). "Four hosts" means all four; "five of five" counts runs.

## Context

The planning text for the skill runner (RW-39) proposes a socket-activated `domovoi-skill-runner@.service` (`User=domovoi-skill`, `Accept=yes`) and a socket-activated helper running as root. Comparing the measurements with its unit text shows where the text is wrong, recorded here before the decision:

- **Its mask list does not start as written.** `TemporaryFileSystem=/var/lib:ro /run:ro` combined with `InaccessiblePaths=/var/lib/domovoi-helper` failed with `status=226/NAMESPACE` on all five runs ("Failed to set up mount namespacing: <child>: No such file or directory"), and `systemd-analyze verify` printed nothing. Replacing the child mask with `InaccessiblePaths=-<child>` started and gave `ENOENT`. The corrected form masks the parent and binds back what is needed.
- **`SupplementaryGroups=` empty does not drop groups** (0030 Decision 4). The unit text must not suggest it does.
- **`Accept=yes` is valid on AF_UNIX but systemd provides neither cancellation on client close nor a package in `%i`.** Closing the client left the instance and its child `active (running)` three seconds later on all five runs. The instance name is an opaque string whose format differs between builds (0030 Decision 8, `ID-3`), so nothing parses it.
- **`ProtectSystem=strict` leaves `/home`, `/root` and `/run/user/<uid>` writable** unless `ProtectHome=yes` is set (four of five runs; this explains an earlier unexplained writable `$HOME`).
- **With `/etc/domovoi` at `root:domovoi-agent 0750` the skill account cannot reach `skills_dir`** without the mask and bind (demonstrated on three hosts), so the mask is required for function, not only for secrecy.
- **`RuntimeMaxSec=` is ignored with `Type=oneshot`** (the unit ran to completion in 9.1 to 9.5 s against a 5 s limit on the three non-WSL hosts; WSL 2 never tried it); `Type=simple` stopped units at 5.06 to 5.32 s on all five runs.
- **`/run` may be mounted `noexec`** (Ubuntu 24.04): anything executed from `/run` fails `203/EXEC`.
- The repository has no helper or runner unit and no `LISTEN_FDS` handling; the agent unit has no `User=`.

## Decision

1. **Sockets.** Two socket units, `domovoi-helper.socket` at `/run/domovoi-helper.sock` and `domovoi-skill-runner.socket` at `/run/domovoi-skill-runner.sock`. Mode and group are a first layer (0030 Decision 2); a `0666` socket is not protected by the unit sandbox.

   ```ini
   # domovoi-skill-runner.socket
   [Unit]
   Description=Domovoi skill runner socket

   [Socket]
   ListenStream=/run/domovoi-skill-runner.sock
   SocketUser=root
   SocketGroup=domovoi-agent
   SocketMode=0660
   Accept=yes
   MaxConnections=16
   RemoveOnStop=yes

   [Install]
   WantedBy=sockets.target
   ```

   | Directive | Reason |
   |---|---|
   | `SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660` | first access layer; a client outside the group was refused before reaching the service on five of five runs; `0660` is a hard maximum (a `0666` socket admitted an unrelated account on four hosts) |
   | `Accept=yes` | one instance, one sandbox, one `RuntimeMaxSec` and one cgroup per connection (item 2) |
   | `MaxConnections=16` | follows from one sandbox per execution; loaded as written on systemd 255.4, behaviour at the limit not measured (the systemd default is 64) |
   | `RemoveOnStop=yes` | the node otherwise survives a stop (nine leftover nodes were counted on one host with the default); with the directive the node was gone after stop for six units on Ubuntu 24.04 |

   The helper socket is the same text with `ListenStream=/run/domovoi-helper.sock`, `Description=Domovoi helper socket` and `MaxConnections=8`.

2. **`Accept=yes`, decided.** One instance per connection gives one sandbox, one `RuntimeMaxSec` and one cgroup (`system-<template>.slice/<template>@<instance>.service`) per execution on every host that measured it. The helper follows; for the helper this is a judgement, not evidence: it holds `flock` across read, recompute, write and sign (0032), so concurrent instances serialise and no in-memory state outlives a request. `Accept=no` also worked (the process was inactive before the first connect, `LISTEN_FDS=1`, one process served several clients; four of five runs) but shares one sandbox, lets a crash affect all runs and applies `RuntimeMaxSec` to the service, not to an execution (that cost structure follows from documented behaviour and was not measured). Accepted costs: the runner implements the cancel; a failed instance stays `failed` until reset (five of five runs); `%i` is not a package id.
   - **Runner cancel design.** Demonstrated as a real unit with a Python stand-in on Ubuntu 24.04: the child's process group differed from the runner's, three seconds after the client closed the child was dead and the instance gone, no `failed` unit, the runner logged exit 0. That shows the design works as a unit; it says nothing about the Rust code. Requirements: the runner spawns the skill through `domovoi-runner` in its own process group (`process_group(0)`; a service's main process cannot `setpgid` itself and one that signals its own group kills itself and ends `failed`); watches fd 0 for EOF while the child runs; on EOF sends SIGTERM to the skill's group, waits a bounded time, sends SIGKILL, collects the child and **exits 0**; enforces the per-execution timeout from the request with the same group kill; exits 0 after any normal completion, refusal or cancel, because a refusal is a result frame and a non-zero exit leaves a `failed` instance. The cgroup kill removes grandchildren that left the group with `setsid` (demonstrated on the Raspberry Pi and Ubuntu 26.04); the group kill is the courtesy, the cgroup the guarantee.
   - `RuntimeMaxSec=` is the backstop and requires `Type=simple`. With a client that stays connected, the client saw EOF at the limit and the instance ended `failed (timeout)` (six seconds for a 6 s limit, four of five runs).
   - The connection is adopted from fd 0 with std only (0030 Decision 7). `Accept=no` would need the fd 3 fallback.

3. **Corrected skill-runner unit.** This is the unit file that ran as written on Ubuntu 24.04 (systemd 255.4) with a stand-in program: it started, isolated, cancelled on EOF, timed out at `RuntimeMaxSec` and cleaned up as designed. One host, one run, user `domovoi-skill` as a probe account; it replaces the planning text's directive list.

   ```ini
   # domovoi-skill-runner@.service
   [Unit]
   Description=Domovoi skill runner (connection %i)
   # No ConditionPathExists= for the bind sources: a condition would skip the
   # unit silently. A plain bind of an absent path fails the start, which is
   # the wanted fail-closed behaviour.

   [Service]
   Type=simple
   User=domovoi-skill
   Group=domovoi-skill
   # No SupplementaryGroups= line (an empty value does not drop groups).
   StandardInput=socket
   StandardOutput=journal
   StandardError=journal
   ExecStart=/usr/bin/domovoi skill-runner
   Restart=no

   NoNewPrivileges=yes
   CapabilityBoundingSet=
   AmbientCapabilities=
   RestrictSUIDSGID=yes

   ProtectSystem=strict
   ProtectHome=yes
   PrivateTmp=yes
   PrivateDevices=yes
   ProtectProc=invisible
   ProcSubset=pid
   TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro
   BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages

   RuntimeMaxSec=@RUNTIME_MAX_SEC@
   KillMode=control-group
   TimeoutStopSec=5s

   MemoryMax=@SKILL_MEMORY_MAX@
   TasksMax=@SKILL_TASKS_MAX@
   ```

   `@RUNTIME_MAX_SEC@` is rendered by the installer: the agent's maximum skill timeout plus a margin. `@SKILL_MEMORY_MAX@` and `@SKILL_TASKS_MAX@` have no evidence behind a default: the mechanism was measured (`MemoryMax=64M TasksMax=20` set `memory.max` and `pids.max` without delegation, and a fork loop failed after 19 children, on three hosts; the test unit used `256M` and `64`), the values must be set from a measurement of real skills.

   | Directive | Reason and evidence |
   |---|---|
   | `Type=simple` | `RuntimeMaxSec` works; `oneshot` ignores it (three hosts) |
   | `User=`, `Group=` | the identity of 0030; real accounts on four hosts |
   | no `SupplementaryGroups=` | an empty value does not drop groups (five of five runs) |
   | `StandardInput=socket` | the connection is stdin and `LISTEN_FDS` is 0 (five of five runs) |
   | `StandardOutput=` and `StandardError=journal` | keeps stdout from being the connection; frames flowed both ways on fd 0, `LISTEN_FDS` was unset and the runner's stderr lines reached the journal with `/run` masked (Ubuntu 24.04, stand-in); to be re-measured with the real runner |
   | `ExecStart=/usr/bin/...` | never under `/run` or `/tmp`: `/run` was `noexec` on Ubuntu 24.04 (`203/EXEC`) |
   | `NoNewPrivileges=yes`, `CapabilityBoundingSet=`, `AmbientCapabilities=` | views showed `NoNewPrivs=1`, `CapEff=CapBnd=0`; defence in depth, no attack path was measured through them |
   | `RestrictSUIDSGID=yes`, `PrivateTmp=yes`, `PrivateDevices=yes` | present in the measured units, not isolated individually; a private writable temp area is wanted (`PrivateTmp` gave a writable `/tmp` under `strict`) |
   | `ProtectSystem=strict` | `/etc`, `/var/lib`, `/var/tmp`, `/opt`, `/srv`, `/mnt` gave `EROFS` (five of five runs) |
   | `ProtectHome=yes` | required: `strict` alone leaves `/home`, `/root`, `/run/user/<uid>` writable; with it `/home/<user>` gave `EACCES`, `/home` and `/root` were inaccessible and `/run/user` gone |
   | `ProtectProc=invisible`, `ProcSubset=pid` | other processes' `/proc/<pid>/*` and `/proc/1/environ` gave `ENOENT`, `ptrace` and `process_vm_readv` `EPERM`; information hiding, not custody |
   | `TemporaryFileSystem=` for three parents | empty tmpfs masks: both sockets, agent and helper state, the key, `helper.toml` and `agent.toml` gave `ENOENT` even with the socket group granted ("mask beats group"); also what makes `skills/` reachable under `/etc/domovoi` 0750 |
   | `BindReadOnlyPaths=` for two sources | read-only: create, rename and unlink gave `EROFS` on five of five runs |
   | `RuntimeMaxSec`, `KillMode=control-group`, `TimeoutStopSec` | stopped at 5.06 to 5.32 s; `control-group` is the default, written out so a gate test can assert it |
   | `MemoryMax`, `TasksMax` | mechanism measured, values not |

   Measured isolation matrix of the corrected unit, as the runner account saw it from inside the unit (the `ST-13` report below; one run per host, stand-ins):

   | Probe from inside the unit | Result | Hosts |
   |---|---|---|
   | `connect()` to either socket path | `ENOENT` | five of five runs |
   | `stat`, list, open of agent state, helper state, `attest.key` | `ENOENT` | five of five runs |
   | `listdir /etc/domovoi`, `listdir /var/lib` | `['skills']`, `['domovoi-packages']`; `helper.toml`, `agent.toml` `ENOENT` | five of five runs |
   | create, rename, unlink in `skills/` and in packages | `EROFS` | five of five runs |
   | append to an existing file there | `EACCES` (DAC is checked before the mount flag), never success | three hosts |
   | `/home`, `/root`, `/run/user` | `EACCES` or absent | four of five runs |
   | `/proc/<helper pid>/*`, `/proc/1/environ` | `ENOENT` | five of five runs |
   | `getgroups()`, `CapEff`, `CapBnd`, `NoNewPrivs` | primary gid only; 0; 0; 1 | four real-account runs |
   | `listdir /run` | `['systemd']` holding only `incoming` (a root-owned tmpfs, mode 0600) on 255.4; empty on 259.5; not recorded on 257.13 and 259.9 | requires conventional Linux validation on 257 and 259.9 |

   Rules that follow:
   - **Mask the parent, bind back what is needed, never put a child mask under a masked parent.** `InaccessiblePaths=` for a path under a `TemporaryFileSystem=` parent fails with `226/NAMESPACE` (five of five runs).
   - **A plain `InaccessiblePaths=` or `BindReadOnlyPaths=` on an absent path fails the start (226), which is fail closed; a `-` prefix starts and silently skips the mask, which fails open.** `TemporaryFileSystem=` on an absent path never fails: it starts and creates an empty directory on the host (seen on the three non-WSL hosts; it left a stray `/etc/` directory on the Raspberry Pi). So the installer creates every directory a mask or bind names, and `ST-18` checks that `/etc/domovoi` is not a directory created by a mask. `systemd-analyze verify` is not a check: it was silent for the failing form.
   - **Never bind a `.sock` and never re-expose a socket**: re-binding one socket into the masked `/run` made it visible again (four of five runs).
   - **No `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` under `/run` or `/tmp`.**
   - `BindReadOnlyPaths=` is applied after `TemporaryFileSystem=`; directive order in the file carries no meaning. The install order does (item 6).
   - A local drop-in that removes the mask line makes skills stop working (fail closed, measured on Ubuntu 24.04: an unmasked runner and a runner with a clearing drop-in both failed `ST-13`); it does not leak. `selftest` reports it as an outage.
   - The form `InaccessiblePaths=<root>` plus `BindReadOnlyPaths=<root>/%i` failed with 226 at user scope on 259.5 (WSL 2); at system scope on 255.4, 257.13 and 259.9 it started and exposed nothing (`EACCES` for the directory and the own package). Scope and version are confounded: no system-scope run on 259.5 and no user-scope run elsewhere exists. Under no measurement did it give a one-package view. This is a user-scope result and is not cited as a version difference.
   - Hardening candidates (`ProtectKernelTunables=yes`, `ProtectKernelModules=yes`, `ProtectKernelLogs=yes`, `ProtectControlGroups=yes`, `ProtectClock=yes`, `LockPersonality=yes`, `RestrictRealtime=yes`, `RestrictNamespaces=yes`, `SystemCallArchitectures=native`, `UMask=0077`, `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6`) started on Ubuntu 24.04 with the expected effect, once, with a stand-in: `socket(AF_NETLINK)` and `socket(AF_PACKET)` gave `EAFNOSUPPORT`, `AF_INET` worked, `unshare(CLONE_NEWUSER)` gave `EPERM`. No real skill workload ran under them, and `UMask=` was not isolated. They ship only after each is measured under fixture skills.

4. **Per-package narrowing is deferred.** `TemporaryFileSystem=<root>:ro` plus `BindReadOnlyPaths=<root>/%i` in a template started separately gave a view of exactly one package, a sibling was `ENOENT`, writes were `EROFS`, and an instance for an absent package failed (four hosts). But `%i` cannot carry a package under `Accept=yes` (it is the counter-and-pid string), and starting a unit per package is a privileged action the helper must not offer generically. v1 exposes the owner-placed skills and the installed packages read-only as one tree.

5. **Helper unit.** The baseline is measured, the per-group directive set is not.

   ```ini
   # domovoi-helper@.service
   [Unit]
   Description=Domovoi helper (connection %i)

   [Service]
   Type=simple
   # Root: no User= line on purpose.
   StandardInput=socket
   StandardOutput=journal
   StandardError=journal
   ExecStart=/usr/bin/domovoi helper serve
   Restart=no

   NoNewPrivileges=yes
   ProtectSystem=strict
   ReadWritePaths=/var/lib/domovoi-helper
   ProtectHome=yes
   PrivateTmp=yes
   PrivateDevices=yes
   CapabilityBoundingSet=CAP_NET_ADMIN
   AmbientCapabilities=

   RuntimeMaxSec=60s
   KillMode=control-group
   TimeoutStopSec=5s
   ```

   | Directive | Reason and evidence |
   |---|---|
   | `Type=simple`, no `User=` | root helper; one request per connection, the state lock held across it |
   | `ExecStart=/usr/bin/...` | never under `/run` (`noexec` on Ubuntu 24.04) |
   | `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/domovoi-helper`, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes` | the measured shape of the root stand-in: `/etc` and `/var/lib` `EROFS`, key read OK, an atomic state write inside `ReadWritePaths` left no temp file |
   | `CapabilityBoundingSet=CAP_NET_ADMIN` | `nft` needs it; gave `CapEff=CapBnd=0x1000`, `ptrace` of another root process `EPERM` (Raspberry Pi and Ubuntu 26.04); on the Pi a root process under exactly that set restarted a disposable unit through `busctl` and read its state, so the bounding set does not block the D-Bus path. The read-only groups and `safety.attest` need none of it |
   | `RuntimeMaxSec=60s` | loaded as written (`RuntimeMaxUSec=1min`) on Ubuntu 24.04; behaviour at the limit not measured |

   On Ubuntu 24.04 with this unit literally, an agent client was allowed and a skill client got `EACCES` at connect. The helper reads `/etc/domovoi/helper.toml` (0600 root); `/etc` stays readable under `strict`.

   Obligations from other decisions: `After=nftables.service` and a test of `ReloadPropagatedFrom` for containment; the helper never runs `daemon-reload` and refuses a unit with `NeedDaemonReload=yes` (`unit.restart`); it reads unit identity from D-Bus plus the files it can read, with no `CAP_DAC_READ_SEARCH`; the leak self-test at install and on a change of kernel, nft or layout needs the right to create a namespace and links, which the helper's privilege statement must record.

   Not designed: the set per operation group, and whether read-only groups drop `CAP_NET_ADMIN` through one unit per group or an in-process drop. `RestrictAddressFamilies=AF_UNIX AF_NETLINK` started on Ubuntu 24.04 but was not exercised with `busctl` or `nft` (the stand-in does neither); `RestrictNamespaces=yes` as a helper candidate conflicts with the leak self-test and must not be adopted for the helper until that is settled. No helper operation that starts a unit or touches a container ships without its own measured set.

6. **Accounts, modes and installer.** Required modes:

   | Item | Owner and mode | Verified by |
   |---|---|---|
   | user `domovoi-agent` | system user, supplementary groups exactly `systemd-journal`; shell `nologin`; home `/var/lib/domovoi-agent`, not created by `useradd` | `ST-02`, `ST-04` |
   | user `domovoi-skill` | created as in 0030 Decision 4; no supplementary group | `ST-03` to `ST-05` |
   | numeric ids | never hard-coded | `ST-09` |
   | `/etc/domovoi` | `root:domovoi-agent 0750` (decided; the alternative of 0755, or moving `skills_dir` out, stays documented and untested) | `ST-10` |
   | `/etc/domovoi/skills` | `root:root 0755`; files 0644, scripts 0755; nothing writable except by root | `ST-10` |
   | `/etc/domovoi/helper.toml` | `root:root 0600` | `ST-10` |
   | `/etc/domovoi/agent.toml` | `root:root 0644`; reach limited by the 0750 parent | `ST-10` |
   | `/var/lib/domovoi-agent` | `domovoi-agent:domovoi-agent 0700` | `ST-10` |
   | `/var/lib/domovoi-helper` | `root:root 0700`, created by `helper init`, which never accepts an existing directory with another mode | helper start check, `ST-11` |
   | `attest.key`, `state`, `state.lock` | `root:root 0600`, regular files, not symlinks | helper start check, `ST-11` |
   | `/var/lib/domovoi-packages` | `domovoi-agent:domovoi-agent 0755`; files 0644, scripts 0755 | `ST-10` |
   | socket nodes | `root:domovoi-agent 0660`, created by the socket units | `ST-12` |

   Install order: (1) accounts, confirming with `getent` that `domovoi-skill` is a member of no other group; (2) directories with the modes above, never relying on `TemporaryFileSystem=` to create one; (3) `domovoi helper init`; (4) unit files root-owned 0644 with every `ExecStart=` under `/usr/bin` and no drop-ins; (5) `daemon-reload` and enable the sockets; (6) `domovoi agent selftest --installer` as root (a failed check leaves the sockets stopped and names it); (7) only then the agent.
   - **A wrong directory, key or state mode is refused, never repaired at start.** The helper start check and `ST-10` and `ST-11` open each path with `O_NOFOLLOW` and report every offending path with found mode and owner and expected mode and owner (`helper_state_permissions`, a token distinct from `state_corrupt`; `path_mode_wrong`). Repair is a separate local root command that prints the plan, changes nothing without confirmation at the terminal, refuses a symlink or wrong type, changes only the listed paths and re-checks afterwards. It is not reachable from the agent, the hub, a chat command or a policy, and not part of start or of the installer's normal run. The reason: the stand-in's initialisation accepted a pre-existing 0755 directory, after which agent and skill could list file names and `stat` them.
   - State writes: temp file, `fsync`, `rename`, `fsync` of the directory, under `flock`. Demonstrated on four hosts: no temp file left, an old descriptor keeps the old content, `flock` conflicts per open file description, is released on `SIGKILL`, and a second root process got `EWOULDBLOCK` while the lock was held. Median state write: 1.2 to 1.5 ms on ext4 on NVMe, 67 ms (p90 212, maximum 722) on ext4 on the USB hard disk, 0.01 to 0.03 ms on tmpfs. `ST-20` warns above a median of about 100 ms and never blocks. Power-loss durability, `ENOSPC` and network or overlay filesystems are not demonstrated.

7. **Platform tiers replace any version floor.** The maintainer decided against a hard systemd floor: everything is allowed, the project publishes which platforms are supported, untested and unaccounted for, and the gate is a runtime isolation check, not a version number. The reason is measurement: Ubuntu 24.04 (systemd 255.4) passed every check that reads effective isolation, below the 257 floor first proposed.

   | Tier | Qualifies | Skills | Helper read groups | Helper write groups (`unit.restart`, containment) |
   |---|---|---|---|---|
   | Supported | on the published list after a real-installer pass with the production binaries, and the gate passes | allowed | allowed | allowed once their own gates pass |
   | Untested | systemd host not on the list whose gate passes (`ST-01` to `ST-07`, `ST-09` to `ST-16`, `ST-18`; the helper start checks) | allowed with a persistent "untested platform" notice and a recorded acknowledgement | allowed | refused unless the owner opts in per host (a switch in the helper configuration, off by default, recorded in audit): their safety rests on kernel and nft behaviour that is version-specific |
   | Blocked | any check fails, or no systemd, or a container without a measured design | refused (`skill_runner_status` is `unsupported` or `error` with a reason; never a fallback to the agent uid) | allowed only if the helper's own checks pass | refused |

   Nothing is Supported yet. The three hosts measured only with stand-ins (Debian 13 aarch64, Ubuntu 26.04, Ubuntu 24.04.5) are "measured candidates" and gate as Untested. Ubuntu 24.04's stock kernel and nft leak on interval-set replace, so its containment writes stay Untested and prefix targets are refused there. `ST-08` records the systemd version and compares nothing. The gate is only as good as its checks: three wordings failed on a real host although isolation held, and were corrected to avoid a false Blocked verdict (the table below); it cannot see an SELinux denial; a drop-in is caught only when `ST-07` and `ST-13` run afterwards. The exact wording of the notice and acknowledgement, who may grant the opt-in, and where the published list lives are still to be settled.

   **Self-test and start checks.** Start checks run inside the helper and the runner at every start and fail closed; selftest checks run from the agent (`domovoi agent selftest`) or as root (`--installer`). Results: `pass`, `fail` (fails closed), `warn` (reported, not blocking), `info`, and `nd` (not run on this host; never a `pass` and never satisfies a gate that requires the check). Reason strings are proposals and are distinct from the wire tokens of 0032.

   | Id | Run by | Check | On failure |
   |---|---|---|---|
   | `ST-01` | agent | effective uid is that of `domovoi-agent`, not 0 | `privilege_boundary_not_in_effect` |
   | `ST-02` | agent | `getgroups()` is exactly `domovoi-agent` and `systemd-journal`; no `docker`, no gid 0 | `agent_groups_unexpected` |
   | `ST-03` | root | `domovoi-skill` exists, uid not 0 and not the agent's, shell `nologin`, member of no group | `skill_account_invalid` |
   | `ST-04` | root | neither Domovoi account is a member of `docker` | `docker_group_member` |
   | `ST-05` | runner, at start | `getgroups()` is exactly the primary gid of `domovoi-skill` | `runner_groups_extra` |
   | `ST-06` | runner, at start | `CapEff` and `CapBnd` zero, `NoNewPrivs` 1 | `runner_privileges` |
   | `ST-07` | root | `systemctl show` of both sockets, the agent unit and both templates, queried through an instance name (`domovoi-skill-runner@selftest.service`, `domovoi-helper@selftest.service`): `LoadState=loaded`, `FragmentPath` the installed path, `DropInPaths` empty, `NeedDaemonReload=no`, `SocketMode=0660`, runner `Type=simple`, `RuntimeMaxUSec` equal to the rendered value after parsing the duration text | `unit_modified`; `unit_not_inspectable` when `systemctl show` exits non-zero or `FragmentPath` is empty (never a skip or a pass) |
   | `ST-08` | root | record the systemd version string and build; no comparison | none (`info`) |
   | `ST-09` | root | no numeric uid or gid in a shipped unit or config; no `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` path under `/run` or `/tmp` | `unit_hardcoded_id`; `unit_exec_under_run` |
   | `ST-10` | root | stat of every path in the mode table; walk of `skills/` and `domovoi-packages` for write bits | `path_mode_wrong` per offending path; never repaired by the check |
   | `ST-11` | root | `fstat` of the helper directory, `attest.key`, `state`, `state.lock` opened with `O_NOFOLLOW`: 0700, 0600, 0600, 0600, uid 0, regular files | `helper_state_permissions` per path |
   | `ST-12` | agent or root | both socket nodes are sockets, `root:domovoi-agent`, 0660 | `socket_mode_wrong` (0666 is a fail) |
   | `ST-13` | root | start one probe instance of the runner unit and read its isolation report (the matrix above, with the tolerant forms below) | `runner_not_isolated{what}` |
   | `ST-14` | agent | connect to both sockets | `error{reason}` of the status |
   | `ST-15` | agent | open `attest.key`, list the helper directory, open `state.lock`, read `/proc/<helper pid>/environ`: `EACCES` for all | `agent_reaches_helper_state` |
   | `ST-16` | agent | `connect` to the Docker socket: `EACCES` or absent | `agent_reaches_docker` |
   | `ST-17` | root | container installs: no read-write `docker.sock` bind, no `--privileged`, no bind of `/` or of the helper directory into the agent container | `container_escape_surface` |
   | `ST-18` | root | `/etc/domovoi` is not an empty 0755 directory created by a mask | `host_directory_created_by_mask` |
   | `ST-19` | root | record LSM list, Yama `ptrace_scope`, `apparmor_restrict_unprivileged_userns` and `..._unconfined`, the AppArmor label of the probe instance, mount options of `/run` and `/tmp` | none (`info`) |
   | `ST-20` | root | ten write cycles (write, `fsync`, `rename`, `fsync` of the directory) on the state filesystem | `state_write_slow` (`warn`, median above about 100 ms) |

   `ST-13` runs inside the real unit: a directive in the file proves nothing, only the effective view answers. `EACCES` where `ENOENT` is expected is a failed mask (it is what an account with a stray group sees without the mask), and the check must tell the two apart; on Ubuntu 24.04 the runner without the masks and the runner with a clearing drop-in both failed with `runner_not_isolated` and passed again after the drop-in was removed. `/etc/shadow` giving `EACCES` is informational only (it appears without the unit too).

   Corrected wording. Implemented as first written, these three checks would have classed a sound host as Blocked (checked on Ubuntu 24.04 against raw output); the corrected forms fail on structure, not by accepting any error code:

   | Check | First wording | Observed | Corrected wording |
   |---|---|---|---|
   | `ST-07` | `systemctl show` of the template names for `RuntimeMaxSec` | the bare template name is rejected ("neither a valid invocation ID nor unit name") on 255.4, 257.13, 259.5 and 259.9; the loaded property is `RuntimeMaxUSec`, printed as `6s`, `1min` or `infinity` | query `<template>@selftest.service`, read `RuntimeMaxUSec`, compare the parsed duration with the rendered value; non-zero exit or empty `FragmentPath` is `unit_not_inspectable`. A drop-in on the runner template gave `unit_modified` with `DropInPaths` listing it, also before any `daemon-reload` (Ubuntu 24.04); the reload-pending case for a running unit was not tested |
   | `ST-13`, `/run` | `listdir('/run')` is empty | `['systemd']` on 255.4, holding only `incoming` (root tmpfs, mode 0600); `[]` on 259.5 | `listdir('/run')` is a subset of `['systemd']`; `listdir('/run/systemd')` a subset of `['incoming']`; if `incoming` exists it is a directory, uid 0, `mode & 0o077 == 0`; anything else, in particular a socket, fails. Checked against the raw listings of 255.4 and 259.5 only |
   | `ST-13`, writes | create, append, rename, unlink all give `EROFS` | create, rename, unlink `EROFS`; append to a file the account does not own `EACCES` on three hosts | create, rename, unlink exactly `EROFS`; append never succeeds and gives `EROFS` or `EACCES`; `/proc/self/mountinfo` must show `ro` on both binds and the three masks. The append result alone cannot tell a read-only mount from DAC. No writable-bind negative was run |

8. **Containers and hosts without systemd.** No skill execution through the runner and no helper. An existing agent on such a host keeps today's behaviour, labelled "privilege boundary not in effect", with no unattended execution. A 0660 socket bind-mounted into a container is matched by numeric gid, which is host-specific, and a mounted `docker.sock` is root (item 9). Whether `deploy/compose` and `deploy/unraid` fit that label was not examined; a supervisor-based design for non-systemd hosts needs its own measurement.

9. **Docker.** No Domovoi account is in the `docker` group (`ST-04`); the agent unit and any agent container get no `docker.sock` mount (`ST-16`, `ST-17`). Demonstrated on the three hosts with Docker (rootful daemon, socket `root:docker 0660`): an account holding only the docker gid listed all containers and ran a disposable container as root, and from inside read the 32-byte key of a 0700 root directory it could not list on the host; a read-only bind of the helper directory alone was enough, and AppArmor's `docker-default` profile, enforcing, did not stop it. On the Raspberry Pi the container also read lines of `/etc/shadow` and created a root-owned file in `/etc` (with the root bind). Not demonstrated: a socket bind-mounted into a container (reachability by uid 0), `lxd` membership, `connect` to the Docker socket from the helper unit under `ProtectSystem=strict`, a Docker-reaching directive set. Any Docker write for the helper needs its own narrow first-party design and ADR; `container.restart` stays blocked for unattended use until it exists.

10. **Sandbox mechanism and profiles.** systemd builds the runner's sandbox from the unit; no user namespaces at launch (with `apparmor_restrict_unprivileged_userns=1`, as on both Ubuntu hosts, `unshare -Ur` failed for an unprivileged account with `write failed /proc/self/uid_map: Operation not permitted`, while systemd's own mount-namespace sandbox was unaffected; it worked on the Raspberry Pi, which has no AppArmor); Landlock stays an idea. No AppArmor or SELinux profile ships at launch; the design must hold by file permissions and namespaces alone, which it did on a host with neither LSM nor Yama (Raspberry Pi). On the two AppArmor hosts every probe unit had the label `unconfined` and no denial was attributable to a probe. Yama is a bonus and is reported (`ST-19`), not required: with `ptrace_scope=1` a same-uid sibling got `EPERM` on attach and `EACCES` on `/proc/<pid>/mem`, but could read `environ` and `maps`; documentation does not claim same-uid isolation where Yama is off. SELinux was not measured.

## What the checker enforces

Nothing in this record is enforced yet; only R3 and R4 apply to the new processes. Proposed, for the implementation to add with gate tests (changes to `xtask/src/arch/rules.rs` need an ADR):

- `UNIT-1`: a parser over `deploy/systemd/` that fails when the runner unit has an `InaccessiblePaths=` entry under a `TemporaryFileSystem=` parent.
- `UNIT-2`: the runner unit has no non-empty `SupplementaryGroups=` and no `Group=` other than `domovoi-skill`; `docker` appears in no `Group=` or `SupplementaryGroups=`.
- `UNIT-3`: the runner unit contains each required directive of Decision 3 with its value.
- `UNIT-4`: a unit with `RuntimeMaxSec=` has `Type=simple`.
- `UNIT-5`: both socket units have `SocketMode=0660`, `SocketUser=root`, `SocketGroup=domovoi-agent`, and `ListenStream` is one of the two literals.
- `UNIT-6`: no `BindPaths=` or `BindReadOnlyPaths=` entry ends in `.sock`; no `/run` child is bound into the runner.
- `UNIT-7`: every path in a plain `InaccessiblePaths=`, `BindReadOnlyPaths=`, `TemporaryFileSystem=` or `ReadWritePaths=` is in the installer's created-set (one list in one file) or is `/var/lib` or `/run`.
- `UNIT-8`: the agent unit has `User=domovoi-agent`; no unit or compose file binds `docker.sock` read-write into the agent.
- `UNIT-9`: no `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` of a shipped unit has a path under `/run` or `/tmp`.
- `INSTALL-1`: the installer creates each directory of Decision 6 before the line that enables a socket.
- `SELFTEST-1`: each `ST` id and each start-check reason string has a named test.
- Gate tests `helper_start_never_chmods_an_existing_directory` and `helper_repair_never_runs_at_start`.

None replaces the run-time checks: a file can say anything and only the effective view is the answer.

## What the checker cannot see

- Whether a given systemd honours a directive as the measured builds did; four builds agreed except the confounded user-scope row, and nothing older than 255.4 was measured.
- Whether a distribution drop-in, a unit override or the host's mount layout changes the effective view. Only the run-time checks (`ST-07`, `ST-13`) see that, and only when they run.
- Whether `domovoi-skill` has supplementary groups on a real host (0030).
- A missing mask that the runner start check does not probe; the isolation probe of `ST-13` covers it.
- Anything about containers, SELinux hosts and non-systemd hosts.

## Unproven

- Everything about the production helper, runner, installer and units; every result used stand-ins or probe scripts. The cheapest next step is a real-installer pass with the production binaries on the measured hosts.
- Systemd 252 and 249, systemd 258, SELinux, a non-merged `/usr` host, other filesystems for `flock` and atomic rename, power-loss durability and `ENOSPC`.
- The individual effect of `PrivateTmp`, `PrivateDevices`, `RestrictSUIDSGID` and `AmbientCapabilities=`; seccomp, `RestrictAddressFamilies=` and the hardening candidates under a real workload; `UMask=`.
- The `/run/systemd` listing under the mask on 257.13 and 259.9; the tolerant `ST-13` wording was checked against the raw listings of 255.4 and 259.5 only, so a different entry elsewhere could still produce a false Blocked verdict.
- The reload-pending case of `ST-07` for a loaded running unit; `StandardOutput=journal` with `StandardInput=socket` under the `/run` mask with the real runner and the fd 0 adoption code.
- The helper directive set per operation group, Docker access from the helper, and netlink and `nft` as a sandboxed root unit.
- The behaviour at `MaxConnections`, and the repair command (designed, not built).

## Open questions

| # | Question | Status |
|---|---|---|
| B1 | `Accept=` | **Decided**: `Accept=yes` (Decision 2). |
| B2 | Which platforms ship at launch | **Decided** as tiers (Decision 7). Still open: the wording of the untested-platform notice and acknowledgement, who may grant the per-host write-group opt-in, where the published list lives, how a host that fails only a `warn` is listed. |
| B3 | Minimum systemd version | **Decided**: none; the runtime gate replaces it (Decision 7). |
| B4 | Per-package narrowing | **Decided**: deferred (Decision 4). |
| B5 | Helper unit directive set | **Open**: the baseline is measured; the per-group set needs measurement with real accounts, and the `RestrictNamespaces=` conflict with the leak self-test needs settling (Decision 5). |

## What acceptance still needs

Before this ADR is accepted:

- a real-installer pass with the production binaries, the corrected units and real accounts on the measured hosts; a platform is promoted to Supported only after it;
- the checks `ST-01` to `ST-20` implemented, with the three corrected wordings, and run in the pass;
- B5 settled and the helper's directive set measured per operation group;
- the remaining unproven items above run or explicitly accepted as residual;
- the published platform list and the notice wording settled.

## Follow-up edits

None of these is made by this change.

- The architecture-decisions plan, RW-39, skill-runner unit: replace the directive list with Decision 3 (delete the `InaccessiblePaths=/var/lib/domovoi-helper` bullet and the `SupplementaryGroups=` sentence; add `ProtectHome=yes`, `ProtectProc=invisible`, `ProcSubset=pid`, `Type=simple`); say that the runner implements the EOF cancel; mark the per-package `%i` example as not available under `Accept=yes`; state that no `ExecStart=` may sit under `/run`; restate "Platforms" as the tier model.
- The installer text: confirm "privilege boundary not in effect" covers container and non-systemd installs.
- `deploy/` and `install.sh` change with the implementation.

## Alternatives considered

- **A transient unit per execution (`systemd-run`).** Rejected: it needs privilege and puts a command-execution path behind the socket.
- **`Accept=no`.** Rejected for the runner: shared sandbox, a crash affects all runs, per-run limits only in-process.
- **A hard systemd floor (257).** Withdrawn: a host below it passed every effective-isolation check.
- **`/etc/domovoi` at 0755, or `skills_dir` moved out of `/etc/domovoi`.** Not chosen; both stay documented and untested.
- **`chmod` and continue at start on a wrong mode.** Rejected: it hides a possibly compromised directory.
- **Copy the planning text's directive list unchanged.** Rejected: it contains a combination that fails to start.
- **Bind a helper socket into the runner's namespace.** Rejected: it would make the helper reachable by package code.

## Revision (2026-11-22)

Brought up to date with the measurements on four hosts and four systemd builds and the maintainer's decisions. Changed: the evidence is stated inline with hosts, scope and labels; `Accept=yes` decided with the runner cancel design and `Type=simple`; the corrected runner and helper units and both socket units are included in full with a reason per directive, replacing the form that fails with `226/NAMESPACE`; the `ProtectHome=yes`, `ProtectProc` and `TemporaryFileSystem=` findings added and the unexplained `$HOME` item resolved; the fail-closed rule amended (a plain mask fails closed, `-` fails open, `TemporaryFileSystem=` creates the directory); the user-scope mask-order result is no longer cited as a version difference; mode refusal and owner-confirmed repair added; the platform matrix and the systemd floor replaced by the tier model; the self-test checks and their three corrected wordings listed; per-package narrowing, sandbox mechanism and profiles recorded as decided; helper obligations from the unit-restart and containment decisions added; proposed rules `UNIT-1` to `UNIT-9`, `INSTALL-1`, `SELFTEST-1` listed; Unproven rewritten; "What acceptance still needs" added. Status stays proposed.
