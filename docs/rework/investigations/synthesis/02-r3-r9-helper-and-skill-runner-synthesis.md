# Synthesis 02. R-3 and R-9: helper delivery and skill-runner isolation

Status: synthesis of evidence plus recommendations. **R-3 and R-9 stay open.** Nothing here accepts a design, declares a platform or a minimum systemd version supported, or edits `99` or any ADR. Facts and recommendations are kept apart: sections 1 to 4 and 10 state what was measured and what follows from it; sections 5 to 9 are proposals; section 6 is speculation marked as such. Section 9 records the owner's decisions of synthesis 00 sections 5a and 5b as decisions (with the consequences this document draws from them) and keeps every other item as a recommendation. This revision adds the fourth host report (U) and the owner's decisions; section 12 lists exactly what changed and why, and section 11 lists the inconsistencies found.

R-10 (the attestation) has its own synthesis. It appears here only where isolation depends on it: custody of `attest.key` and of the helper state.

## 1. Scope, sources and the five runs

### 1.1 Sources

| Id | Report | What it measured |
|---|---|---|
| J | `J-r3-r9-helper-and-skill-runner.md` | first pass at user scope (stand-in uid 1000 for everything), then a system-scope pass (J §9) with `User=loginuser` as the stand-in for `domovoi-skill` and a disposable group |
| M | `M-r3-r9-r10-real-user-pass.md` | system scope, two real disposable users (agent uid 999, skill uid 997), probe-prefixed paths, stand-in root helper (the R-10 harness code behind a socket) |
| N | `N-pi-r3-r9-r10-systemd257.md` | system and user scope, real names `domovoi-agent` and `domovoi-skill` and real design paths, stand-in helper, Docker section |
| S | `S-host-c-r3-r9-r10-ubuntu-kernel7.md` | system scope only, probe-prefixed accounts and re-rooted paths, AppArmor and Yama active, Docker section, harness run |
| U | `U-host-t-r3-r9-ubuntu2404-systemd255.md` | system scope only, probe-prefixed accounts and re-rooted paths, AppArmor (with the userns restriction) and Yama active, Docker section; **the first run of the corrected unit files of section 5 literally, and of the selftest checks ST-01 to ST-20** (Python stand-ins); no toolchain, so no R-10 harness |

Probes: `probes/r3r9`, `probes/realuser`, `probes/pi3`, `probes/host-c3`, `probes/host-t3`. Evidence labels below follow the reports: J and M use "demonstrated on WSL2 (user scope / system scope / real users, system scope)"; N uses P, P-user, ND, CL; S uses C, ND, CL; U uses T, ND, CL. Cells below cite `U§n` for a section of U.

### 1.2 The five runs and four hosts

J and M ran on the same WSL2 machine (same systemd build, same kernel). "N of 5" in the tables counts the five runs J, M, N, S and U (the five environments), not five independent hosts: independent hosts are four (WSL2, Pi, host-c, host-t), of which WSL2 is a research environment and three are real machines. Where a result needs host independence, this document says "4 hosts" and the host count is stated wherever independence matters. The Pi's and WSL's earlier counts ("3 hosts", "4 of 4") are superseded by this revision.

| Item | J (WSL2, first and second pass) | M (WSL2, real users) | N (Pi) | S (host-c) | U (host-t) |
|---|---|---|---|---|---|
| OS | Ubuntu 26.04.1 on WSL2 (J §1; M §1 gives the release) | Ubuntu 26.04.1 LTS on WSL2 | Debian 13 (trixie) | Ubuntu 26.04.1 LTS | Ubuntu 24.04.5 LTS (U§1) |
| systemd | 259.5-0ubuntu3.4 (J §1) | 259.5-0ubuntu3.4 (M §1) | 257.13-1~deb13u1 (N §1) | 259.9-0ubuntu0.1 (S §1) | 255.4-1ubuntu8.18 (U§1) |
| kernel | 6.18.58.1-microsoft-standard-WSL2 | same | 6.18.60+rpt-rpi-2712 | 7.0.0-43-generic | 6.8.0-149-generic |
| arch | x86_64 (not stated in J) | x86_64 (not stated in M) | aarch64 | x86_64 | x86_64 |
| LSM | not recorded in J | not recorded in M | `capability` only | lockdown, capability, landlock, yama, apparmor, bpf, ima, evm | lockdown, capability, landlock, yama, apparmor (U§1) |
| Yama | not recorded in J | `ptrace_scope=1`, not changed (M §1) | absent | `ptrace_scope=1` | `ptrace_scope=1`, not changed |
| AppArmor userns restriction | not recorded | not recorded | none (no AppArmor) | `apparmor_restrict_unprivileged_userns=1` and `..._unconfined=1`, read only (S §1) | `apparmor_restrict_unprivileged_userns=1` and `..._unconfined=0` (S: 1), read only (U§1) |
| `/run` | not recorded | not recorded | tmpfs (N §1) | tmpfs (S §1) | tmpfs `rw,nosuid,nodev,noexec` (U§1) |
| Root filesystem | `$HOME` ext4, probe dir tmpfs (J §1) | not recorded | ext4 on USB 5400 rpm HDD | ext4 on NVMe | ext4 on NVMe (write cache write back); `/tmp` and `/var/tmp` on the root filesystem |
| Scope | user scope (J §1 to §8), system scope (J §9) | system | system, plus user scope for several rows (N §3.1) | system only (S §0 header) | system only; nothing at user scope (U§2) |
| User types | `loginuser` uid 1000 as stand-in for agent, skill and data owner (user scope); `loginuser` as skill stand-in plus a disposable group (system scope) | two real disposable system users with primary group only, plus an extra group for wrong-group tests | real `domovoi-agent` (groups agent plus `systemd-journal`) and `domovoi-skill` (primary only), login user, `nobody` | probe-prefixed agent (agent group plus a second group) and skill (primary only), login user `host-c` (in `docker`), `nobody` | probe-prefixed agent (agent group plus a second group standing for `systemd-journal`) and skill (primary only), login user uid 1000 (in `docker`, `lxd`, `sudo`; passwordless `sudo -n`), `nobody` |
| Names and paths | `$HOME` fixtures at user scope, real design paths at system scope | `domovoi-probe-ru-*` names, design shape | real names, real design paths | `domovoi-probe-c3-*` names, `/srv/domovoi-probe-c3/...` re-rooted paths | `domovoi-probe-t3-*` names, `/srv/domovoi-probe-t3/...` re-rooted paths (U§2) |
| Helper | none (dummy 0600 key file) | root stand-in `ru_attest serve` | root stand-in | root stand-in | root stand-ins in Python (`helper_full.py`, `runner_full.py`, `selftest.py`); the units of 5.1 to 5.3 ran literally, with `ExecStart=/usr/bin/python3 /srv/...` in place of `/usr/bin/domovoi` (U§3.3) |
| Docker | absent (J §3 H8) | not tested | rootful, gid 985, empty group | rootful, gid 983, login user is a member | rootful, gid 1001, the login user is a member (U§3.6) |

Cells marked "not recorded" mean the report does not state the item; they are not measurements of absence. M's design paths are the shape of RW-39 but carry the `-ru-` prefix; S and U re-root them; only N used the literal design paths (N §3.6, M §1 deviation, S §2 naming table, U §2). U's host was not blank: a live agent and other services were running and were neither read nor touched; a second research session ran there at the same time (U§1). Consequence for results that depend on a literal string: none of the reports ran the literal RW-39 text with the literal `/var/lib` and `/run` parents under a real `domovoi-skill` except N (N §3.2) and the parent-mask behaviour is path-independent (M §1).

No run used the production helper, the production skill runner, the installer or any unit shipped from `deploy/`: none exists (J §7, M §6, N §3.5, S §6, U §3.4 and §3.8). Every "helper" and "runner" result is about a stand-in. U is the first run to use the unit text of section 5 of this document as written (re-rooted paths, a Python interpreter as `ExecStart=`), and the first to run a Python re-implementation of the checks ST-01 to ST-20; neither is the planned Rust code (U§3.4).

## 2. Consolidated findings

Notation: `Y` = observed as stated; `n/m` = not measured; `part` = measured in a reduced form. Counts are over the five runs J, M, N, S, U ("N of 5"); "4 hosts" is stated where it matters. Report sections are cited as `J§`, `M§`, `N§`, `S§`, `U§`.

### 2.1 Socket activation, `Accept=`, mode, group, `SO_PEERCRED`

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-ACT-1 `Accept=no` socket activation: inactive before first connect, active after; `LISTEN_FDS=1`, `LISTEN_FDNAMES`, `LISTEN_PID` own pid; one process serves several clients | Y (J§3 H2, user scope) | n/m | Y (N§3.1 H2, system and user) | Y (S§3.1 H2) | Y (U§3.1 H2) | 4 of 5 (M did not run it) | universal (4 hosts) |
| F-ACT-2 `Accept=yes` is valid on AF_UNIX; one instance per connection; instance name format differs between builds (2.1.1); with `StandardInput=socket` the connection is stdin and `LISTEN_FDS=0`; `systemd-analyze verify` clean | Y (J§3 H3, §5, §9) | Y (M§3 row d) | Y (N§3.1 H3) | Y (S§3.1 H3) | Y (U§3.1 H3) | 5 of 5 | universal for the model; **the instance name format differs between builds, build and host not separated** (2.1.1) |
| F-ACT-3 `Accept=yes` on INET gives pid=0 uid=-1 gid=-1 as peer cred | Y (J§3 H4, H5) | n/m | Y | Y | Y (U§3.1 H4) | 4 of 5 | universal; irrelevant to AF_UNIX design |
| F-ACT-4 `SocketMode=0660 SocketUser=root SocketGroup=<g>`: root and group members connect; others get `EACCES` and never reach the service | Y (J§9) | Y (M§3 row d) | Y (N§3.1 H5) | Y (S§3.1 H5) | Y (U§3.1 H2) | 5 of 5 | universal |
| F-ACT-5 a `0666` socket admits `nobody`/skill user | Y (J§9) | part (contrast socket in M§3 row b: skill socket connects EACCES, mode not 0666) | Y (N§3.1 H5) | Y (S§3.1 H3b) | Y (U§3.1 H3b) | 4 of 5 explicit | universal (4 hosts) |
| F-ACT-6 `SO_PEERCRED` gives the kernel uid/gid of every peer; `unshare -U` and (where allowed) `-Ur` do not change it | Y (J§3 H5, user scope, same-uid peer) | n/m for ns; creds per instance Y (M§3 row d) | Y (N§3.1 H5, both unshare forms) | `-U` Y; `-Ur` fails as unprivileged under AppArmor (S§3.1 H5, §3.3) | `-U` Y; `-Ur` fails as an unprivileged account under AppArmor, root works (U§3.1 H5) | 5 of 5 for creds; `-Ur` blocked on both AppArmor hosts (S, U), worked on N | universal for creds; `-Ur` host-dependent (AppArmor) |
| F-ACT-7 helper-side uid check on the real socket path: agent allowed, skill (given the group, no mask) denied by uid; with the mask the socket is invisible | n/m | part (instance peer cred read, M§6) | Y (N§3.5) | Y (S§3.4) | Y (U§3.5) | 3 of 5 (N, S, U: the stand-ins with a uid check; M read instance creds only) | universal as far as measured; note the check is in a stand-in |
| F-ACT-8 each `Accept=yes` instance gets its own cgroup `system-<template>.slice/<template>@<instance>.service` | n/m | n/m | Y (N§3.7 C3) | Y (S§3.1 Accept matrix) | Y (U§3.1 Accept matrix) | 3 of 5 | universal as far as measured; the instance part of the name varies (2.1.1) |

#### 2.1.1 The instance name format differs between builds

Instance names seen in the raw output (fact; searched in `probes/realuser`, `probes/r3r9`, `probes/pi3`, `probes/host-c3`, `probes/host-t3`): 255.4 (U, host-t) `0-416061-995` and 257.13 (N, Pi) `0-112936-999` have the form `<counter>-<pid>-<uid>`; 259.5 (J and M, WSL) `0-11-1869_5384-999` (`probes/realuser`) and `0-10-1249_8414-0` (`probes/r3r9`), and 259.9 (S, host-c) `0-8198-576198_560481-995` (`probes/host-c3`), have the form `<counter>-<n>-<pid>_<n>-<uid>` (the numeric fields are not interpreted here). The earlier text of this document gave the second form for every host and gave the first in F-ACT-8; U's observation (U§3.1 H3, §4 item 2) is the older form. The split follows the build, but each build ran on exactly one host, so build and host are not separated (the same one-to-one confound as S10b, 2.4.1); this document does not call it a version fact. What stays true on every build: the name is opaque, it is not a package id (F-NAR-3), and it carries a counter.

Recommendation: nothing in Domovoi may parse, construct or compare an instance name (the helper and the runner identify a connection by `SO_PEERCRED` and the unit cgroup only). Gate test proposed in 7.2: `no_code_parses_systemd_instance_names`.

### 2.2 Cancellation, timeouts, process groups, lingering instances

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-CAN-1 closing the client does not end an `Accept=yes` instance or its child (still `active (running)` 3 s later) | Y (J§4 S9, §9) | Y (M§3 row d, 3.5 s) | Y (N§3.1 S9) | Y (S§3.1 S9) | Y (U§3.1 S9) | 5 of 5 | universal |
| F-CAN-2 a runner that watches stdin for EOF and kills its process group leaves nothing | Y (J§4 S9, §9) | Y (M§3 row d, also under the corrected masks) | Y | Y | Y (U§3.1 S9; the design of 5.4 as a real unit, U§3.3) | 5 of 5 | universal |
| F-CAN-3 the instance that kills its own group ends `failed` and lingers until `reset-failed`; a first version's `setpgid` of the main process failed `EPERM` (main process already leads its group) | part (failing instance lingers, J§5) | Y (M§4 F-7) | Y (N§3.1 S9) | Y (S§3.1 S9, `/bin/false` instance) | Y (U§3.1 S9; U§3.3: a runner that exits 0 leaves no `failed` unit) | 5 of 5 for lingering `failed`; the `setpgid` detail 1 of 5 | lingering universal; `setpgid` note single-source |
| F-CAN-4 `RuntimeMaxSec=5s` with `Type=simple` stops the unit at 5.06 to 5.32 s, `Result=timeout`, SIGTERM; cgroup kill removes `sleep` children; N and S also a `setsid` grandchild | Y (5.17 s user, 5.18 s system; J§4 S8, §9) | Y (5.06 s, client still connected; M§3 row d) | Y (5.28 s, grandchild dead; N§3.1 S8) | Y (5.32 s; S§3.1 S8) | Y (5.11 s, U§3.1 S8) | 5 of 5 | universal |
| F-CAN-5 `RuntimeMaxSec=` with `Type=oneshot` is ignored with a warning and the unit runs to completion (9.1 to 9.5 s) | n/m (J used `simple`) | n/m | Y | Y | Y (9.16 s; `systemd-analyze verify` prints the warning, U§3.1 S8) | 3 of 5 (N, S, U: the three non-WSL hosts; WSL 259.5 never tried it) | universal as far as measured, but unmeasured on 259.5; do not call it version-dependent |
| F-CAN-6 with `RuntimeMaxSec` and a client that stays connected, the client sees EOF at the limit and the instance ends `failed (timeout)` | n/m | part (M§3: stopped at 5.06 s with client connected) | Y (4.3 s) | Y (4.2 s) | Y (6.1 s with the corrected unit, `RuntimeMaxSec=6s`, U§3.3) | 4 of 5 | universal |

### 2.3 Sandbox directives and mask behaviour

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-MSK-1 `TemporaryFileSystem=<dir>:ro` makes the dir an empty tmpfs (marker `ENOENT`, write `EROFS`); mode seen 0755 | Y (J§4 S2, §9) | Y (M§3 row b) | Y (N§3.1 S2) | Y (S§3.1 S2) | Y (U§3.1 S2) | 5 of 5 | universal |
| F-MSK-2 parent masks `TemporaryFileSystem=/var/lib:ro /run:ro` make every child path `ENOENT`, both 0660 sockets invisible even with the group granted ("mask beats group") | Y (J§9) | Y (M§3 row c) | Y (N§3.1 S5, §3.2) | Y (S§3.1 S5, §3.2) | Y (U§3.1 S5, §3.3) | 5 of 5 | universal |
| F-MSK-3 `InaccessiblePaths=<existing>` gives `EACCES` (stat shows mode 0000) | Y (J§4 S3, S4, §9) | Y (M§3 row b, hardening-only run still DAC) | Y | Y | Y (U§3.1 S3, S4) | 5 of 5 | universal |
| F-MSK-4 RW-39 form (parent `TemporaryFileSystem=` plus child `InaccessiblePaths=` as a second layer) fails `226/NAMESPACE`, "Failed to set up mount namespacing: <child>: No such file or directory"; `systemd-analyze verify` prints nothing | Y (J§9, system, stand-in uid) | Y (M§3 row b, real users) | Y (N§3.2, literal RW-39 text, real accounts, as a runner instance: first connection gets an empty reply, instance `failed`, socket stays `active`) | Y (S§3.2) | Y (U§3.2: the text as written; also as a runner instance behind the socket) | 5 of 5 | universal |
| F-MSK-5 same with `InaccessiblePaths=-<child>`: starts, child `ENOENT` | Y (J§9) | Y (M§3 row b) | Y | Y | Y (U§3.2) | 5 of 5 | universal |
| F-MSK-6 corrected form (parent masks alone, binds back): starts; key, helper and agent state, both sockets `ENOENT`; skills and packages readable, writes `EROFS`; `CapBnd=0`, `NoNewPrivs=1`; `groups=[primary]` | Y (J§9, stand-in) | Y (M§3 row b) | Y (N§3.2) | Y (S§3.2) | Y (U§3.2, and U§3.3: the unit of 5.2 literally) | 5 of 5 | universal |
| F-MSK-7 a plain `InaccessiblePaths=` or `BindReadOnlyPaths=` on an absent path: `226/NAMESPACE` (fail closed) | Y (J§4, §9) | n/m | Y (N§3.3 A1) | Y (S§3.2 absent table) | Y (U§3.2 absent-path table) | 4 of 5 | universal (4 hosts) |
| F-MSK-8 `-` prefix on an absent path: starts, mask silently skipped (fails open for that path) | Y | Y (M§3 row b, child under parent mask) | Y | Y | Y (U§3.2) | 5 of 5 | universal |
| F-MSK-9 `ReadOnlyPaths=` on an absent path: starts | n/m | n/m | Y | Y | Y (U§3.2) | 3 of 5 | universal as far as measured |
| F-MSK-10 `TemporaryFileSystem=<absent>:ro` starts and creates the mount point as an empty directory on the host (also under `ProtectSystem=strict`); N's stray `/etc/domovoi-probe-pi3-mp` | n/m | n/m | Y (N§2, §3.3 A1, A3) | Y (S§3.2: inside the fixture tree and at a fresh path) | Y (U§3.2: an empty root directory on the host, inside the fixture tree and at a fresh path) | 3 of 5 (the three non-WSL hosts) | universal as far as measured; never tested on 259.5 |
| F-MSK-11 `TemporaryFileSystem=/var/lib:ro /run:ro` combined with `InaccessiblePaths=<child under it>` fails `226/NAMESPACE` | Y (J§9) | Y (M§3 row b) | Y (N§3.2) | Y (S§3.2) | Y (U§3.2) | 5 of 5 | universal |
| F-MSK-12 re-binding one socket into a masked `/run` (`BindReadOnlyPaths=-/run/...sock`) makes it visible again (`EACCES` without the group) | Y (J§9) | n/m | Y (N§3.1 S5) | Y (S§3.1 S5) | Y (U§3.1 S5) | 4 of 5 | universal (4 hosts) |
| F-MSK-13 a world-connectable (0666) socket is not protected by the directives | Y (J§9) | n/m | Y | Y | Y (U§3.1 S5, H3b) | 4 of 5 | universal (4 hosts) |
| F-MSK-14 read-only bind: create, append, rename, delete, symlink, hardlink, chmod `EROFS`; hardlink to `/tmp` `EXDEV` | Y (J§4 S6, §9) | part (writes `EROFS`/`EACCES`; M§3 row b) | Y (N§3.1 S6) | part (create only; append to an agent-owned file is `EACCES` by DAC; S§3.1 S6) | Y: create, rename, unlink `EROFS`; **append to a file the skill does not own is `EACCES`** (U§3.1 S6, §3.3) | 5 of 5 (full matrix on 2); the append `EACCES` is also seen on S and in M's real-user pass (`probes/realuser/results/t_b.txt`, `open(w, existing)`) | universal; the `EACCES` for append matters for ST-13 (5.9) |
| F-MSK-15 `ProtectProc=invisible ProcSubset=pid` hides other pids from the filesystem (`/proc/<pid>/*`, `/proc/1/environ` `ENOENT`); `ptrace` and `process_vm_readv` still `EPERM`, not `ESRCH` | Y (J§9 for `/proc/1/environ`) | Y (M§3 row a, F-5) | Y (N§3.1 S7) | Y (S§3.1 S7) | Y (U§3.1 S7) | 5 of 5 | universal |
| F-MSK-16 `PrivateTmp`, `PrivateDevices`, `RestrictSUIDSGID`, `AmbientCapabilities=` present in measured units, never isolated | present | present (M§6) | present (N§5) | not stated | present in the 5.2 and 5.3 units that ran (U§3.3); not isolated | none isolated | not measured individually |
| F-MSK-17 `RestrictAddressFamilies=`, seccomp, `ProtectKernel*`, `MemoryDenyWriteExecute`, `SystemCallFilter=` | n/m | n/m | n/m | n/m | part: the 5.2 and 5.3 hardening candidates (`RestrictAddressFamilies=`, `ProtectKernel*`, `ProtectClock`, `LockPersonality`, `RestrictRealtime`, `RestrictNamespaces`, `SystemCallArchitectures`) start and show the stated effect, with stand-in programs only (U§3.3); seccomp filters, `MemoryDenyWriteExecute`, `SystemCallFilter=` n/m | 1 of 5 (U only; start and effect, no workload) | single-source; `UMask=` unverified (probe defect, U§2) |

### 2.4 The RW-39 and corrected unit forms, per-package narrowing (S10)

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-NAR-1 `TemporaryFileSystem=<pkgs>:ro` plus `BindReadOnlyPaths=<pkgs>/%i`, template started per package: `pkgA` sees only `pkgA`, sibling `ENOENT`, write `EROFS`, mirror for `pkgB`; a non-existent instance fails | Y (J§4 S10, §9; user and system) | n/m | Y (N§3.1 S10; user and system) | Y (S§3.1 S10) | Y (U§3.1 S10) | 4 of 5 (M n/m; 4 hosts) | universal (4 of 5 runs, 4 hosts) |
| F-NAR-2 `InaccessiblePaths=<pkgs>` plus `BindReadOnlyPaths=<pkgs>/%i` | **fails `226/NAMESPACE` for every instance, at user scope on 259.5** (J§4 S10). The system-scope S10 of J§9 does not repeat this form. | n/m | starts, succeeds, **nothing visible** (`EACCES` incl. the own package), both directive orders (N§3.1 S10; system and user scope on 257) | starts, nothing visible, both orders (S§3.1 S10; system scope) | starts, nothing visible: `EACCES` for the directory and the own package, mount source `/systemd/inaccessible/dir`, both orders (U§3.1 S10b) | 4 of 5 (M n/m); the outcomes differ, 2.4.1 | **confounded: see 2.4.1**; three system-scope hosts agree |
| F-NAR-3 `%i` under `Accept=yes` is the counter-pid-uid string, not a package id; narrowing used a separately started template instance | Y (J§5) | n/m | Y (N§3.1 S10 note, §5) | Y | Y (U§3.1 S10 note) | 4 of 5 (M n/m; 4 hosts) | universal |

#### 2.4.1 The S10b confound, stated plainly

The claim in the Pi report (N§4 item 1) is "259.5 fails with 226, 257 starts and exposes nothing". The host-c report (S§3.1 S10, §4 item 5) points out that J's 226 was measured at user scope while N and S measured system scope.

What is established:
- At user scope on systemd 259.5 in WSL2, the form `InaccessiblePaths=<root>` plus `BindReadOnlyPaths=<root>/%i` failed every instance with `226/NAMESPACE` (J§4 S10; user scope is stated in J§0 and J§4).
- At system scope on 255.4 (U§3.1 S10b), on 257.13 (N) and on 259.9 (S), the same form starts and exposes nothing (on 255: `EACCES` for the directory and the own package, mount source `/systemd/inaccessible/dir`, both directive orders).
- N's S10 row carries the labels P and P-user, and its text says "same at user scope" right after the template-instance result; it is ambiguous whether the `InaccessiblePaths=` plus bind form itself was also run at user scope on 257. If it was, and it did not fail there, that would already separate scope from version; the report does not say. This is a gap in N, not a finding.

What is not established:
- That 259.5 at system scope fails. J§9 did not run the form at system scope; M did not run it; no run measured 259.5 at system scope.
- That 255, 257 or 259.9 at user scope succeed (U ran nothing at user scope).
- That the difference is a version difference. 255.4, 257.13 and 259.9 agree at system scope (three hosts, three builds); 259.5 is the odd one only at a different scope. A third system-scope host agreeing does not separate scope from version, because the 259.5 datum is still the only user-scope one. The cheapest discriminator: run the form once at system scope on the WSL host (259.5) and once at user scope on the Pi or host-c.

Consequence for the design: the form is broken or useless in every observation (either `226` or nothing visible). Under no measurement does it deliver "narrowed view of one package". The recommendation (section 5) does not use it, so the confound does not block anything, but ADR text must not cite the `226` as a systemd-version fact.

### 2.5 Accounts, groups, `SupplementaryGroups=`

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-GRP-1 `SupplementaryGroups=` empty does not drop groups the account has; the unit inherits them. With a stray group: `[stray, primary]`; a granted group is added on top | Y (system, `User=loginuser`: `[4,24,27,30,46,100,1000]`, J§9 S1; user scope: unmapped gids shown as 65534, J§4 S1) | Y (`[984, 985]`, M§3 row c) | Y (`[983, 1002]`, N§3.1 S1) | Y (`[979, 980]`, S§3.1 S1) | Y (`[983, 984]` with a stray group, `[983, 984, 985]` with a granted one, U§3.1 S1) | 5 of 5 | universal |
| F-GRP-2 a clean account (primary group only) gives `getgroups()` of the primary gid only, in units with and without the empty directive | n/a (stand-in) | Y | Y (`[983]`) | Y (`[980]`) | Y (`[984]`, U§3.1 S1) | 4 of 4 real-account runs (M, N, S, U) | universal |
| F-GRP-3 the agent unit without a directive gets the account's groups (agent plus `systemd-journal` on the Pi: `[984, 999]`) | n/a | n/a | Y | Y (`[979, 981]`) | Y (`[983, 985]`, U§3.1 S1) | 3 (N, S, U) | universal as far as measured |
| F-GRP-4 a skill unit given the agent group and no mask passes the socket group layer; with the corrected masks the same unit sees neither socket nor `agent.toml` | n/a | Y (M§3 row c: also `agent.toml` 0640 group readable without mask) | Y | Y | Y (U§3.2: with `SupplementaryGroups=agent` the view is identical; U§3.5: given the group and no mask, the skill passes the group layer) | 4 of 4 real-account runs | universal |
| F-GRP-5 user-namespace unit group handling at user scope: supplementary gids appear as `65534` but the kernel still holds them | Y (J§4 S1) | n/a | Y (17 gids, N§3.1 S1) | n/m (no user scope) | n/m (system scope only) | 2 | host-independent but user-scope only |

### 2.6 `ProtectSystem`, `ProtectHome`, the `$HOME` anomaly

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-PRT-1 `ProtectSystem=strict` makes `/etc`, `/var/lib`, `/var/tmp`, `/opt`, `/srv`, `/mnt` `EROFS` | Y (J§9) | part (hardening-only run, M§3 row b) | Y | Y | Y (U§3.7) | 5 of 5 | universal |
| F-PRT-2 with `ProtectSystem=strict` and default `ProtectHome=no`, `/home`, `/root` and `/run/user/<uid>` remain writable (separate read-write mounts) | user scope Y, unexplained (J§4 S7); system scope not probed (J§9 last paragraph) | n/m | Y, explained from mountinfo (N§3.4; system and user) | Y (S§3.6) | Y (U§3.7) | 4 of 5 | universal (4 hosts); the J anomaly is explained, not a WSL oddity |
| F-PRT-3 `ProtectHome=yes`: `/home/<user>` `EACCES`, `/home` and `/root` inaccessible, `/run/user` gone; `ProtectHome=read-only`: `EROFS` | Y (J§4 S7, user scope) | n/m | Y | Y | Y (U§3.7) | 4 of 5 | universal |
| F-PRT-4 `ProtectSystem=full` leaves `/var/lib` writable by DAC only | n/m | n/m | Y | Y (`/home` writable) | Y (U§3.7: `/home` writable) | 3 (N, S, U) | universal as far as measured |
| F-PRT-5 `PrivateTmp=yes` gives a writable private `/tmp` and `/var/tmp` even under `strict`; `ProtectHome=yes` hides `/run/user`, so scripts stored under `/home` or `/run/user/<uid>` cannot be started | n/m | n/m | Y (N§3.4) | n/m | Y (U§3.3, §3.7: private writable `/tmp` and `/var/tmp`; `/run/user` gone under `ProtectHome=yes`) | 2 of 5 (N, U) | two hosts |

### 2.7 Custody, state files, modes, `/etc/domovoi`

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-CUS-1 a root-owned 0700 directory with 0600 key, state and lock refuses two real unprivileged uids (and uid 1000/65534 earlier) for list, read, write, copy, create, rename, link, chmod, unlink, flock | Y (J§9, stand-in uids) | Y (M§3 rows a, f, two real uids) | Y (N§3.1 H6, §3.5) | Y (S§3.1 H6, §3.4) | Y (U§3.1 H6) | 5 of 5 | universal |
| F-CUS-2 `/proc/<helperpid>/{environ,mem,maps,fd,exe,cwd,root/..}` refused to non-root real users; `ptrace`, `strace -p`, `kill(pid,0)`, `process_vm_readv` `EPERM`; `status` and `cmdline` readable | n/m | Y (M§3 row a) | Y (N§3.5, incl. gdb and strace) | Y (S§3.4, no gdb installed) | Y (U§3.5, with `gdb`: both users get "Could not attach", root attaches) | 4 of 5 (M, N, S, U) | universal (4 hosts) |
| F-CUS-3 the cross-uid `ptrace` denial is plain DAC; Yama is not needed for it, but on the only two hosts with Yama (M on WSL, S) it was not separated from it. The Pi has no Yama and also refused | n/a | not separated (M§4 F-5) | Y refused without Yama (N§3.5) | Y; Yama `ptrace_scope=1` also separates same-uid siblings (S§3.4) | Y: Yama 1 and AppArmor active, same refusals (U§3.5) | 4 | universal for the cross-uid case; independence from Yama is established by the Pi only, because on the three Yama hosts (M, S, U) the two were not separated |
| F-CUS-4 directory, key, state and socket modes unchanged by `restart` and by `SIGKILL` plus start | n/a | Y (M§3 row f) | n/m | n/m | n/m | 1 | single-source |
| F-CUS-5 F-4: a helper directory that exists with mode 0755 before `init` is accepted by the harness; agent and skill can then list file names and `stat` them (files stay 0600) | n/a | Y (M§4 F-4; superseded run kept) | n/m | n/m | part: the ST-11 check logic refuses a pre-existing 0755 directory (U§3.4); the harness was not run | 1 (harness behaviour); check logic also seen on U | harness behaviour, single-source; motivates a start check |
| F-CUS-6 F-6: a lock file the process cannot open is reported as `state_corrupt: lock` | n/a | Y (M§4 F-6) | n/m | n/m | n/m | 1 | harness behaviour, single-source |
| F-CUS-7 F-8: with the layout's permissions an agent user cannot touch the state file at all; the generation-repeat scenario needs write access to the directory, which in this layout also exposes the key | n/a | Y (M§3 row f, M§4 F-8) | n/m | n/m | n/m | 1 | single-source |
| F-CUS-8 F-2: with `/etc/domovoi` at `root:domovoi-agent 0750` (N and M's mode) a `domovoi-skill` process cannot traverse into `/etc/domovoi/skills`; only `TemporaryFileSystem=<etc>:ro` plus `BindReadOnlyPaths=<etc>/skills` makes it readable | n/a | Y (M§3 row b, F-2) | Y (N§3.2 observation 1) | not run (S used a fixture `etc/domovoi` mode; its §6 lists the different-mode case as unproven) | Y (U§3.4 ST-13 negatives: without the mask `listdir etc/domovoi` is `EACCES`; the corrected unit reads `skills/`) | 3 of 5 (M, N, U) | universal under the stated mode; the mode is a design input (see 9, D-1, decided) |
| F-CUS-9 under DAC alone the packages tree (0755) is readable by `domovoi-skill`; `/etc/domovoi` is not | n/a | Y (M§3 row b) | Y (N§3.2) | n/m | part: readable through the bind (U§3.3); a DAC-alone read is not reported | 2 (M, N) | universal as far as measured |

### 2.8 Docker (D-24 evidence)

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-DOC-1 no Docker daemon in WSL | Y (J§3 H8) | n/m | n/a | n/a | n/a | n/a | environment |
| F-DOC-2 `/var/run/docker.sock` `srw-rw---- root docker`; `docker.socket` `SocketMode=0660 SocketGroup=docker` | n/a | n/a | Y (gid 985, group empty) | Y (gid 983, login user is a member, who also has `sudo`) | Y (gid 1001; the login user is a member and is also in `lxd`, U§3.6) | 3 of 3 Docker hosts | universal on Docker hosts |
| F-DOC-3 rootful daemon, no userns remap; security options seccomp and cgroupns (N), plus apparmor `default` (S); no podman, no rootless socket | n/a | n/a | Y | Y | Y (apparmor `default`, seccomp `builtin`, cgroupns; no podman, no rootless socket, U§3.6) | 3 of 3 | universal as far as measured |
| F-DOC-4 a probe account holding only the docker gid (not a member) lists all containers; without the gid refused | n/a | n/a | Y | Y | Y (U§3.6) | 3 of 3 | universal |
| F-DOC-5 root-equivalence: that account ran a disposable container as root, from inside read the 32-byte `attest.key` of a 0700 root directory it cannot list on the host; N additionally read `/etc/shadow` lines and created a root-owned file in `/etc` (with `-v /:/host`); a read-only bind of only the helper directory was enough; S used only a read-only bind of the fixture helper directory | n/a | n/a | Y | Y (AppArmor `docker-default` did not stop reading the bind-mounted file) | Y (read the key through a read-only bind of the helper directory; AppArmor `docker-default` in enforce did not stop it, U§3.6) | 3 of 3 | universal on Docker hosts |
| F-DOC-6 a docker.sock bind-mounted into a container is reachable by container uid 0 and any uid holding gid 985 | n/a | n/a | Y | n/m | n/m (not shown) | 1 | single-source |
| F-DOC-7 a helper-style 0660 socket bind-mounted into a container is matched by numeric gid (host-specific); skill's numeric ids refused, agent's accepted, `--group-add` accepted | n/a | n/a | Y | n/m | n/m (not shown) | 1 | single-source |
| F-DOC-8 a socket-activated (inherited fd) helper inside a container; per-endpoint Docker API narrowing; container escape; remapped userns | n/a | n/a | ND | ND | ND | 0 | not measured |

### 2.9 AppArmor, Yama, LSM

| Finding | Evidence | Reproduced | Class |
|---|---|---|---|
| F-LSM-1 every probe unit and runner instance had AppArmor label `unconfined`; no `apparmor="DENIED"` attributable to a probe unit or socket service (kernel log read per stage) | S§3.3; U§3.5 (kernel log for the whole work: two DENIED lines, both from `unshare`, none from a probe unit or socket service; 149 profiles, 53 enforcing) | 2 of 5 (S, U: the only two AppArmor hosts) | two AppArmor hosts (S, U), same result |
| F-LSM-2 `apparmor_restrict_unprivileged_userns=1` stops `unshare -Ur` for unprivileged accounts (`write failed /proc/self/uid_map: Operation not permitted`, `capable sys_admin` DENIED in the `unprivileged_userns` profile); root works; systemd's own mount-namespace sandbox is not a user namespace and is unaffected | S§3.3, §4 item 1; U§3.1 H5 (the same two denials; `os.unshare(CLONE_NEWUSER)` itself succeeds for the skill uid inside a unit, because the profile applies afterwards, and `RestrictNamespaces=yes` makes it `EPERM`, U§3.3); contrast N§3.1 H5 (worked on the Pi, no LSM) | 2 of 5 blocked (S, U); worked on N, which has no LSM | host-dependent: blocked on both AppArmor hosts (S, U), works on the Pi |
| F-LSM-3 Docker container processes run under `docker-default`; it did not stop reading a root-owned bind-mounted file | S§3.3, §3.5; U§3.6 (`docker-default` in enforce, same result) | 2 of 5 (S, U: the two Docker hosts with AppArmor) | two Docker hosts with AppArmor |
| F-LSM-4 a unit with `AppArmorProfile=` attached to the skill runner | not run | 0 of 5 | not measured |
| F-LSM-5 Yama `ptrace_scope=1`: a same-uid sibling attaching to a non-descendant gets `EPERM`; the sibling can read `/proc/B/environ` and `/proc/B/maps` but `/proc/B/mem` is `EACCES` | S§3.4; U§3.5 (same result, also inside a sandboxed skill unit with `ProtectProc=invisible`) | 2 of 5 (S, U; N has no Yama; WSL did not run the row) | two hosts; the Pi has no Yama, WSL had it at 1 but did not run this row |
| F-LSM-6 SELinux | not run anywhere | 0 of 5 | not measured |
| F-LSM-7 the userns sysctl pair differs between hosts (`apparmor_restrict_unprivileged_unconfined` 0 on U, 1 on S); no probe result depended on it | U§1 | 2 of 5 (S, U) | recorded fact |

### 2.10 Storage cost, locking, cgroups

| Finding | J | M | N | S | U | Reproduced | Class |
|---|---|---|---|---|---|---|---|
| F-FS-1 write temp, `fsync`, `rename`, `fsync(dir)`: no temp left, an old fd keeps the old content | Y (J§3 H6, tmpfs probe dir, ext4 `$HOME`) | n/m | Y (200 cycles, ext4) | Y (500 cycles) | Y (500 cycles, U§3.7) | 4 of 5 (M n/m; 4 hosts) | universal |
| F-FS-2 cost of one state write (median): tmpfs 0.01 to 0.027 ms; ext4 NVMe 1.21 to 1.48 ms (p99 2.1 to 7.7 ms, max 6.2 to 11.1 ms); ext4 on a USB 5400 rpm HDD 67 ms (p90 212 ms, max 722 ms) | n/a | n/a | Y | Y | Y (ext4 NVMe median 1.455 ms and 1.484 ms; ST-20 median 1.431 ms; tmpfs 0.027 and 0.020 ms, U§3.7, §3.4) | 3 of 5 (N, S, U; 3 hosts) | host-dependent by 45 to 55 times between NVMe and the USB HDD; payload size (2 B vs 385 B) irrelevant at this scale (S§3.9, U§3.7) |
| F-FS-3 `flock` conflicts per open file description (same and other process, ext4 and tmpfs); released on SIGKILL; an `fcntl` lock does not conflict with it | Y (J§3 H6, J§9) | n/m | Y (N§3.9 F3) | Y (S§3.9) | Y (U§3.1 H6, §3.7) | 4 of 5 (M n/m; 4 hosts) | universal |
| F-FS-4 power loss, `ENOSPC`, rename-to-dirfsync crash window, network and overlay filesystems | n/m | n/m | n/m | n/m | n/m | 0 of 5 | not measured |
| F-CG-1 `DynamicUser=yes`: no stable uid (65376), `StateDirectory=` kept as a symlink under `/var/lib/private` after stop, implied `ProtectSystem=strict`; cannot replace `domovoi-skill`; same refusals as `domovoi-skill` for key, state, sockets | n/m | n/m | Y (N§3.7 D1) | not run (S§2) | not run (U§2) | 1 of 5 (N only) | single-source |
| F-CG-2 without `Delegate=` the unit's cgroup is root-owned (mkdir and `subtree_control` `EACCES`); with `Delegate=yes` it is chowned; controllers can be enabled in a sub-cgroup, only after the process left the delegated cgroup (no-internal-process rule; moving back `EBUSY`); a child takes `memory.max`, `pids.max` and dies by `cgroup.kill` | n/m | n/m | Y (N§3.7 C1) | Y (S§3.7; matches) | Y (U§3.7) | 3 of 5 (N, S, U; 3 hosts) | universal (3 of 3 hosts) |
| F-CG-3 `MemoryMax=64M TasksMax=20` set `memory.max` and `pids.max` without delegation; a fork loop failed after 19 children | n/m | n/m | Y (N§3.7 C2) | Y | Y (U§3.7) | 3 of 5 (N, S, U; 3 hosts) | universal (3 of 3 hosts) |

### 2.11 Universal, host-dependent, confounded

- **Universal on every host that measured it** (all rows with 4 or 5 reproductions in 2.1 to 2.7): `Accept=yes` on AF_UNIX and its instance model (not its name format, 2.1.1); mode/group on sockets; kernel peer credentials; no EOF cancellation by systemd and the runner-side fix; `RuntimeMaxSec` with `Type=simple`; the empty-tmpfs masks and "mask beats group"; the 226 failure of the RW-39 parent plus child form and its `-` repair; absent-path behaviour of plain and `-` masks; `SupplementaryGroups=` empty not dropping account groups; read-only binds; `ProtectSystem=strict` leaving `/home`, `/root`, `/run/user/<uid>` writable; root-owned 0700/0600 custody against non-root uids; `/proc` and `ptrace` refusal across uids.
- **Measured once, on U, with the units of section 5 literally**: the runner template of 5.2 and the helper template of 5.3 with their sockets start, isolate, cancel on EOF, time out at `RuntimeMaxSec` and clean up as designed; journal output and frames on fd 0 under the `/run` mask work; `RemoveOnStop=yes` removes the socket node; the 5.2 and 5.3 hardening candidates start (U§3.3). Stand-in programs, one host.
- **Universal as far as measured but never measured on 259.5**: `RuntimeMaxSec` ignored for `Type=oneshot`; `TemporaryFileSystem=` creating a host mount point on an absent path. Both were found on the Pi, host-c and host-t (the three non-WSL hosts). The ADR text must say "not measured on 259.5", not "version-dependent".
- **Host-dependent**: `unshare -Ur` for unprivileged accounts (blocked by AppArmor on host-c and host-t, works on the Pi); Yama same-uid separation (present on host-c, host-t and WSL, absent on the Pi); state write cost (1.2 to 1.5 ms NVMe, 67 ms USB HDD); the `docker` group membership (empty on the Pi, populated with the login user on host-c and host-t); numeric uid and gid values (999/984 and 997/983 on the Pi, 995/994 on host-c and host-t, 999/997 on WSL); the instance name format (differs between builds, not separated from the host, 2.1.1); the content of a `/run` mask (`/run/systemd/incoming`: present on 255, empty on 259.5, unrecorded on 257 and 259.9, 5.9); `/run` mounted `noexec` (host-t; not recorded elsewhere).
- **Confounded**: S10b (2.4.1). Also confounded: M's and J§9's results were run with probe-prefixed names (M) or a stand-in uid (J), so only N used the literal design names and paths.
- **Single-source**: F-CUS-4 to F-CUS-7 (M only; the ST-11 check logic of F-CUS-5 was also run on U), F-DOC-6 and F-DOC-7 (N only), F-CG-1 (N only), F-MSK-17 (U only, start and effect), `PrivateTmp`/`ProtectHome` side effects in N§3.4 (now N and U). F-LSM-1 to F-LSM-3 and F-LSM-5 are no longer single-source: two AppArmor hosts.

## 3. Analysis

### 3.1 ADR claims against the evidence

Verdict key: holds (measured and consistent on all runs that measured it), partly (measured but narrower than stated), fails, untested.

| Claim | Verdict | Evidence |
|---|---|---|
| 0030 D1 three identities table (agent owns `state_dir` 0700; runner has no supplementary groups, reaches nothing of Domovoi's; helper root with 0700 directory) | partly | the OS boundary holds for two real uids on four hosts (F-CUS-1, F-CUS-2, F-MSK-6); the identities were measured with a stand-in helper, with no installer and with account names that differ from design except on the Pi (N§3.6). The agent's `systemd-journal` membership is measured only for the unit-without-directive group list (F-GRP-3). |
| 0030 D2 layer 1: mode `0660` and group first layer; a `0666` socket not protected | holds | F-ACT-4, F-ACT-5, F-MSK-13 |
| 0030 D2 layer 2: `SO_PEERCRED` reports real uid; a user namespace cannot present another uid | holds | F-ACT-6; `-Ur` could not run on host-c, but `-U` and the Pi's `-Ur` cover it |
| 0030 D2 layer 2 "it is not the isolation" | holds | S§3.4 and N§3.5: a skill unit given the group without a mask passes the group layer and is stopped only by the uid check; the compromised agent uid passes both (by design, 0030 D5) |
| 0030 D2 layer 3: mask makes sockets and state invisible even when a group allows connection | holds | F-MSK-2 (5 of 5), F-GRP-4 (4 of 4 real-account runs) |
| 0030 D3 `run_as` only `domovoi-skill` | untested | no runner; no `run_as_unsupported` reason exists |
| 0030 D4 `SupplementaryGroups=` empty does not drop groups; the requirement is on the account | holds, and now with real accounts | F-GRP-1 (5 of 5), F-GRP-2 (4 real-account runs) |
| 0030 D4 start-up check (`getgroups` has any group besides the primary) fails closed | partly | the check logic, run from a script in real units, separates clean from polluted (M§3 row c; N§5; S§5). Never run inside a Rust runner or `selftest`. |
| 0030 D5 helper allowlist is not execution authorization; a compromised agent can call the helper | untested by measurement | follows from group membership: the agent uid connects (F-ACT-4, F-ACT-7); the allowlist does not exist |
| 0030 D6 no fallback to the agent uid | untested | design rule |
| 0030 Unproven "root-owned state against another uid" | now holds for two real uids | F-CUS-1; power loss still open (F-FS-4) |
| 0031 Context: RW-39 mask list does not start | holds | F-MSK-4 (5 of 5; the literal text on the Pi, the text as written on U) |
| 0031 Context: `-` prefix fails open for that path; plain fails closed; `verify` silent | holds | F-MSK-5, F-MSK-7, F-MSK-8; `verify` printed nothing for six forms on the Pi and for the failing forms on host-c (N§3.3 A2, S§3.2) |
| 0031 Context: `Accept=yes` does not give cancellation or a package in `%i` | holds | F-CAN-1, F-NAR-3 |
| 0031 D3 per-package narrowing is infeasible through `%i` under `Accept=yes` | holds (as stated: "through the instance name") | F-NAR-3. The `TemporaryFileSystem` plus bind template works when the instance is started by a dispatcher (F-NAR-1), so the alternative design is feasible and the 0031 deferral is a choice, not a limit |
| 0031 D4 table rows | holds for every measured row; additions below | see F-MSK, F-PRT |
| 0031 D4 `ProtectHome=yes` row labelled user scope only | strengthened | F-PRT-3 (4 of 5); and the "unexplained writable `$HOME`" row is explained (F-PRT-2) |
| 0031 D4 "mask order matters: `InaccessiblePaths=<root>` plus bind fails" | partly | F-NAR-2: confounded; on two system-scope hosts it does not fail but exposes nothing, which is equally unusable |
| 0031 D4 "installer creates every directory a plain mask names" | partly | true for `InaccessiblePaths=` and `BindReadOnlyPaths=` (F-MSK-7); for `TemporaryFileSystem=` the failure never happens and a host directory is created silently (F-MSK-10), so an installer that skips a directory is not caught by a `TemporaryFileSystem=`-only unit |
| 0031 D4 "never re-expose a socket" | holds | F-MSK-12 |
| 0031 D5 helper directive set not designed | unchanged | one root unit variant measured (N§3.1 H7, S§3.1 H7): reads its key, `CapEff=CapBnd=CAP_NET_ADMIN`, `/etc`, `/var/lib` `EROFS`, ptrace of another root process `EPERM` |
| 0031 D6 root 0700/0600 behave as designed; `flock` blocks a second root process | holds | F-CUS-1, F-FS-3 |
| 0031 D6 atomic write leaves no temp file | holds | F-FS-1; power loss untested |
| 0031 D7 platform matrix | partly | four systemd hosts now measured (section 3.4) |
| 0031 D8 Docker: agent gets no general Docker socket write; helper Docker access needs its own design | holds, with evidence | F-DOC-4, F-DOC-5 show why: docker-group membership or a bind-mounted docker socket is root on the host |
| 0031 D9 version dependence unproven | narrowed | four builds (255.4, 257.13, 259.5, 259.9) agree except S10b (confounded), the instance name format (2.1.1), the `/run` mask listing (5.9) and rows unmeasured on 259.5; the next older versions (Debian 12 has 252, Ubuntu 22.04 has 249 per N§5 and S§5) are still unmeasured |
| 0031 B1 `Accept=yes` with runner-side EOF and `RuntimeMaxSec` backstop | partly | the mechanism works on 5 of 5 runs, and on U as a real unit with a stand-in runner (U§3.3); the cost items hold (lingering `failed`: 5 of 5; `%i` not a package); the `setpgid` pitfall and the `Type=oneshot` pitfall are new (F-CAN-3, F-CAN-5) |
| RW-39 "`SupplementaryGroups=` empty" | fails (as a control) | F-GRP-1 |
| RW-39 `InaccessiblePaths=/var/lib/domovoi-helper` as a second layer under `/var/lib` mask | fails | F-MSK-4, F-MSK-11 |
| RW-39 `BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages` "applied after the temporary filesystems" | holds | F-MSK-6 |
| RW-39 "on EOF the runner kills the process group" | holds only as a runner duty | F-CAN-1, F-CAN-2 |
| RW-39 `RuntimeMaxSec` as backstop | holds with `Type=simple`; ignored with `Type=oneshot` | F-CAN-4, F-CAN-5 |
| RW-39 per-package `%i` example | fails under `Accept=yes` | F-NAR-3 |
| RW-39 "`ProtectSystem=strict` alone makes `/etc` read-only but still readable" | holds | F-PRT-1 and `/etc/shadow` etc. by DAC; `/etc` listings readable |
| RW-39 packages dir `domovoi-agent`-owned 0755, read-only to skill | holds | F-CUS-9, F-MSK-14 |
| RW-39 a compromised skill cannot reach `/etc/domovoi` files other than `skills/` | holds under the corrected unit; under DAC alone with `/etc/domovoi` 0750 it cannot reach `skills/` either | F-CUS-8 |
| D-29 skill identity with no access to helper socket or agent-private state | holds at the OS level for the measured shapes | F-CUS-1, F-MSK-6, F-GRP-4 |
| D-29 privileged operations go only through one root helper | untested (no helper) | |
| D-31 `run_as` refusal path | untested | |
| D-24 no general Docker socket write for the agent | consistent with evidence | F-DOC-5, F-DOC-6 |
| 04 §8.3 "Attestation key ... masked in the agent and skill-runner units" | partly | masking in the **skill-runner** unit is measured (F-MSK-6). Masking in the **agent** unit is wrong as a mechanism: the agent must reach the helper socket, and M§3 row (b) "corrected form for the agent user" shows the mask removes its own sockets. The agent is protected from the key by DAC (0700 root), not by a mask. This text needs correction (section 8). |
| 5.1 to 5.3 unit text of this document, run as written (re-rooted paths) | holds on 255.4 | the runner and helper units, with their sockets, start, isolate, cancel on EOF, time out and clean up as designed (U§3.3); one host, stand-in programs |
| 5.9 ST-13 point 3, "`listdir('/run')` is empty" | **fails as written on 255.4**; holds on 259.5 | 255 always leaves `/run/systemd/incoming` (root tmpfs, mode 0600) in the mask (U§3.3); 259.5 recorded `[]` (`probes/realuser/results/t_b.txt:170`); 257 and 259.9 unrecorded. Checked against raw output in this revision |
| 5.9 ST-13 point 4, "create, append, rename and unlink give `EROFS`" | **fails as written for append** on every host that tested an unowned file | append to a file the skill does not own is `EACCES` (DAC before the mount flag): U§3.3, S§3.1 S6, and M's real-user pass (`t_b.txt`, `open(w, existing)`) |
| 5.9 ST-07, `systemctl show` of the template names | **fails as written** (also in R-8) | the bare template name is rejected ("neither a valid invocation ID nor unit name") on 259.5, 257.13, 259.9 (R-8 raw output: `probes/r8`, `probes/pi8`, `probes/host-c8`) and 255.4 (U§3.3); the property is `RuntimeMaxUSec`, not `RuntimeMaxSec` |
| a noexec `/run` and the units | holds, with one constraint | direct exec of a script under `/run` fails `203/EXEC` (U§3.3); the units exec `/usr/bin` and root-filesystem trees and are unaffected; nothing may be executed from `/run` |
| 0031 D4 `RemoveOnStop=yes` (derived in 5.1) | holds | the node is removed for six units; the default leaves it (U§3.3) |

### 3.2 What each layer contributes

"Load-bearing" means that with the layer removed and all others present, a measured attack path opens. "Redundant" means every attack path it blocks is also blocked by another layer on the measured hosts. Attack paths: skill user reaching helper (sockets, state, key) or agent state.

| Layer | What the evidence shows it does | Load-bearing or redundant |
|---|---|---|
| Socket mode `0660` and group `domovoi-agent` | refuses the skill user and `nobody` at `connect` (F-ACT-4) | **Load-bearing against a clean skill account** if no mask is present. With a polluted skill account (group added) it opens (M§3 row c: sockets connect). Redundant when the mask is present. |
| `SO_PEERCRED` uid check | skill given the group and no mask passes the group layer and is denied by uid (F-ACT-7) | Load-bearing in exactly one case: skill account polluted with the agent group and the mask missing. Also the only layer that separates a second human member of the agent group from the agent. It does nothing against the agent uid (by design). It was measured in a stand-in, not a production helper. |
| Mount mask (parent `TemporaryFileSystem=`) | makes sockets, state, key and `helper.toml`, `agent.toml` invisible even with the group granted (F-MSK-2, F-GRP-4) | **Load-bearing for the polluted-group case**, and for F-2: the only way the skill reads `/etc/domovoi/skills` under a 0750 `/etc/domovoi` (F-CUS-8). Redundant for secrecy of 0700/0600 files against a clean account. |
| DAC (root-owned 0700 directory, 0600 files, agent 0700 state) | alone refuses the skill user and agent for helper state and key (F-CUS-1); alone refuses the skill for agent state | **Load-bearing for key and helper state custody in every case**, because it also covers the agent uid, which the mask is not applied to. The mask is not a custody mechanism for the agent. |
| Uid separation (`domovoi-skill` is not `domovoi-agent`) | the premise of everything above | Load-bearing. |
| No supplementary groups on `domovoi-skill` | removes the group-based paths (F-GRP-1) | Load-bearing as an account requirement; the unit directive does nothing (F-GRP-1). |
| `ProtectProc=invisible ProcSubset=pid` | hides the pid from the filesystem only; syscalls still `EPERM` (F-MSK-15) | Redundant for custody (cross-uid `/proc` and ptrace are refused by DAC); useful against information leakage (command lines, other processes). Not a security boundary on its own (M§4 F-5). |
| `NoNewPrivileges`, empty `CapabilityBoundingSet=` | `NoNewPrivs=1`, `CapEff=CapBnd=0` | Defence in depth; no measured attack path opens without them because all measured attacks are filesystem or socket paths. Not tested by an attack (no setuid binary probe). |
| Yama | separates same-uid siblings (S§3.4, U§3.5) | Not load-bearing for the helper (different uid). Relevant between two executions of the same `domovoi-skill` uid. |
| AppArmor | no profile applied to any probe unit on either AppArmor host (F-LSM-1) | Not contributing on the measured hosts; blocks only our own unprivileged userns tools (F-LSM-2). |
| `ProtectHome=yes` | closes `/home`, `/root`, `/run/user` writable under `strict` (F-PRT-2, F-PRT-3) | Load-bearing for the "no writes outside private areas" property; DAC still applies to the skill user, so what is exposed is only what the skill account may write. |
| Read-only binds | package and skill sources cannot be changed even by their owner uid (F-MSK-14, J§9) | Load-bearing against the case where the skill account owns what it executes; otherwise DAC already refuses (agent-owned 0755 files). |
| `RuntimeMaxSec` plus runner group kill | bounds a run (F-CAN-4); the runner's EOF cancel is the primary; cgroup kill removes `setsid` grandchildren (N, S) | Load-bearing for the time bound; the cgroup kill is what makes escaped grandchildren die. |

### 3.3 Threat model against protections demonstrated

Protections here are those demonstrated with a stand-in helper; "none" means no measurement addresses it, not that it is exposed.

| Adversary | Can do (design) | Protections demonstrated | Not demonstrated or open |
|---|---|---|---|
| Compromised agent (uid `domovoi-agent`) | connect to helper and runner sockets; read its own state | cannot read helper key, state or lock, cannot list the helper directory, cannot use `/proc`, `ptrace`, `process_vm_readv` on the helper (F-CUS-1, F-CUS-2; 4 hosts); cannot forge an attestation (M§3 row e, R-10 synthesis); cannot modify helper state (M§3 row f) | what the production helper allowlist and safety rules constrain (nothing exists); that the helper rejects a hostile frame; the agent can call the helper directly by design (0030 D5) |
| Compromised skill (uid `domovoi-skill`) | whatever the account may do: outbound network, world-readable files | cannot connect to either socket (DAC, mask: 5 of 5), cannot read agent state, helper state, key, `/etc/domovoi` except the exposed `skills/` (4 of 4 real-account runs); sources read-only; cannot cancel the unit's time limit | outbound network (`RestrictAddressFamilies=` was shown to start and to change the socket families on one host with a stand-in, U§3.3; no egress filtering and no real skill workload measured); a skill reaching the hub port (RW-39 note); other skills' and packages' content (static view exposes all, F-NAR deferred); seccomp, kernel attack surface |
| Same-uid process (another skill execution, or another process of a user) | read its sibling's `/proc/<pid>/environ` and `maps` | Yama `ptrace_scope=1` refuses attach to non-descendants and `/proc/B/mem` is `EACCES` (S§3.4, one host); `PrivateTmp` per instance (not isolated as a result) | per-instance isolation beyond Yama and `PrivateTmp`: `environ` readable; per-package narrowing would reduce it; Pi has no Yama |
| Docker-group member | root on the host: read the helper key by a read-only bind of the helper directory (F-DOC-5) | none; Docker group members bypass every layer here | design stance only (section 5.9): ensure no Domovoi account is in `docker`; the login user on a Docker host normally is |
| Rolled-back state (restore of an old helper `state` file) | repeat a generation for different content (M§3 row f, P2 scenario) | the layout's permissions make this need write access to the 0700 directory, which also exposes the key (M§4 F-8) | the hub rule rejecting a repeat with different identity (0032 C6, R-10 synthesis) |
| A distribution drop-in or a host override that changes the unit | silently weakens the sandbox | none | no check exists; `verify` is silent on the failing forms (F-MSK-4) |
| A root-owned host directory missing at runner start | the runner starts with a hole (dash-prefixed) or fails (plain) | plain masks fail closed with `226` (F-MSK-7); parent `TemporaryFileSystem=` never fails and creates a host directory (F-MSK-10) | the installer and `selftest` that would catch it do not exist |
| An unprivileged local user not in `domovoi-agent` | connect to sockets | refused at connect, never reaches the service (5 of 5) | n/a |

### 3.4 Platform matrix updated

Support claims are not made. "Status in the evidence" says what was measured; "Tier (proposal)" is the classification under the tier model of D-3 and D-4 in section 9, which implements the owner's decision of synthesis 00 section 5b, and is a recommendation, not a support statement. Until the production helper, runner and installer are measured, no host is release-tested, so no host is "Supported" in the sense of 5b; hosts measured with stand-ins are labelled **measured candidates** here (the word U uses is "Supported-candidate").

| Platform | Evidence | Status in the evidence | Tier (proposal) |
|---|---|---|---|
| systemd host, Ubuntu 26.04, 259.9, kernel 7.0, AppArmor and Yama | S: all mechanisms at system scope with real probe users; AppArmor did not interfere with systemd sandbox directives (F-LSM-1) | measured, stand-in helper | measured candidate |
| systemd host, Debian 13, 257.13, aarch64, no LSM, no Yama | N: same mechanisms, literal design names and paths | measured, stand-in helper | measured candidate (write groups: gated by the containment and unit tests of 04 and 01) |
| systemd host, Ubuntu 24.04.5, 255.4, kernel 6.8, x86_64, AppArmor with the userns restriction, Yama 1, `/run` `noexec` | U: all mechanisms at system scope with real probe users; **the corrected units of section 5 and the selftest checks ST-01 to ST-20 run literally with stand-ins**; three ST wordings fail as written (5.9) | measured, stand-in helper and runner; below the 257 floor this document recommended before the tier decision; the isolation holds | measured candidate for skills and helper read groups; **helper write groups Untested** (the nft side is outside this document: synthesis 04 and report W found that on kernel 6.8 with nft 1.0.9 an interval-set replace is not packet-atomic, W §3.9 and §3.10) |
| WSL2, 259.5 | J and M | research environment; two runs, one host | research environment, not a deployment target |
| systemd 252 (Debian 12), 249 (Ubuntu 22.04) | none | not measured (N§5, S§5) | Untested (admitted if the gate passes) |
| SELinux distributions (Fedora, RHEL family) | none | not measured | Untested if the gate passes; the gate cannot see an SELinux denial that is not an `EACCES` at the probe |
| Container with socket mounts | N: bind-mounting a 0660 socket into a container: matched by numeric gid, skill ids refused (F-DOC-7); a docker.sock mounted gives root (F-DOC-6) | partial: bind-mounted path only; no inherited-fd helper in a container; no skill runner in a container; no systemd in a container | Blocked for skills and write groups (no measured design) |
| Hosts without systemd (Unraid and others) | none (J§3 H9, N, S, U: not testable) | not measured | Blocked |
| Rootless Docker/Podman, user-namespace remapped daemons | none (no rootless socket on any Docker host; remap not configured) | not measured | Blocked until measured |
| Existing root agent hosts (compose, Unraid XML) | `deploy/compose/agent.yml` uses a docker socket proxy (read-only mount of the socket); not examined for the label | out of the evidence | legacy agent, "privilege boundary not in effect" (5.12) |

## 4. Conclusions

### 4.1 Supported by the evidence

1. **The OS-level boundary between two non-root accounts and a root-owned 0700/0600 area holds** on four hosts and five runs, against read, write, copy, rename, link, `chmod`, `flock`, `/proc` and `ptrace` (F-CUS-1, F-CUS-2). It was measured with a stand-in root helper, not the production helper.
2. **Socket mode `0660` with `SocketGroup=` blocks a clean account**, and the mask blocks a polluted one; the mask beats the group (F-ACT-4, F-MSK-2, F-GRP-4; 5 of 5, 4 of 4 real-account runs for the last).
3. **The RW-39 unit text fails to start** (`226/NAMESPACE`) on every run, including the literal text with real accounts on the Pi. The corrected form of 0031 (parent masks alone, binds back) gives the intended view on every run.
4. **`SupplementaryGroups=` empty is not a control.** The control is the account's group list, checked by `getgroups()` (F-GRP-1, F-GRP-2).
5. **systemd does not cancel on EOF, and lingering `failed` instances are normal.** A runner-implemented EOF cancel with a separate process group works; `RuntimeMaxSec` with `Type=simple` is the backstop and kills grandchildren through the cgroup (F-CAN-1 to F-CAN-6).
6. **`ProtectHome=yes` is required** for the runner unit: `ProtectSystem=strict` alone leaves `/home`, `/root` and `/run/user/<uid>` writable (F-PRT-2, 4 hosts). The earlier "unexplained `$HOME`" observation is explained.
7. **A plain mask on an absent path fails closed; a `-` mask fails open; `TemporaryFileSystem=` of an absent path silently creates a host directory; `systemd-analyze verify` catches none of it** (F-MSK-7, F-MSK-8, F-MSK-10, 2.3).
8. **F-2 is real**: with `/etc/domovoi` at 0750 `root:domovoi-agent` the skill account cannot reach `skills_dir` without the mask and bind (F-CUS-8; M, N and U). This makes the mask a functional requirement, not only a secrecy measure.
9. **Docker group membership and a mounted docker.sock are root on the host** on all three Docker hosts, with only a read-only bind of the helper directory needed (F-DOC-5), and `docker-default` AppArmor did not prevent it (F-LSM-3).
10. **AppArmor with the default Ubuntu profile set did not interfere** with any systemd sandbox directive or socket service on two AppArmor hosts, Ubuntu 26.04 (S) and Ubuntu 24.04 (U) (F-LSM-1), but it blocks unprivileged `unshare -Ur` on both, so any rootless-namespace sandbox for the runner would need a profile (F-LSM-2).
11. **Per-package narrowing is feasible mechanically** (`TemporaryFileSystem=<pkgs>:ro` plus `BindReadOnlyPaths=<pkgs>/%i` with a dispatcher-started instance) on four runs on four hosts (F-NAR-1); it is not available through the `Accept=yes` instance name.
12. **The corrected unit files of section 5 work as written on systemd 255.4** (one host, stand-in programs): `systemd-analyze verify` is silent, every unit starts, an instance isolates the skill (ST-05, ST-06, ST-13 pass), EOF cancellation per 5.4 leaves nothing and no `failed` unit, `RuntimeMaxSec=6s` stops a runner that ignores EOF at 6.1 s and kills the `setsid` grandchild, journal output and fd 0 frames work under the `/run` mask, `RemoveOnStop=yes` removes the node, and the hardening candidates of 5.2 and 5.3 start and change the socket families and namespaces as expected (U§3.3). Of the items 5.1 to 5.3 had marked unmeasured, journal output and fd 0 frames under the `/run` mask, `RemoveOnStop=` and the start and effect of the hardening candidates are now measured once. Still unmeasured: the behaviour of `MaxConnections=` at its limit, `UMask=`, the helper variant's real operations (`busctl` over `AF_UNIX` and nft over `AF_NETLINK` under `RestrictAddressFamilies=`), a real skill workload, and any production binary.
13. **Three wordings of the checks in 5.9 fail on real hosts although the isolation holds**: ST-13 point 3 (255 leaves `/run/systemd/incoming`), ST-13 point 4 for append (`EACCES`, three hosts), and ST-07's query (a bare template name is rejected on four builds). Implemented literally they would classify a sound host as Blocked (a false negative). Section 5.9 carries tolerant forms that still fail on real drift; the drift cases (unmasked unit, unit with a drop-in, runner without `NoNewPrivileges`, polluted account, wrong helper directory mode, a 0666 socket) were run as negatives on U (U§3.4).
14. **A `noexec` `/run` constrains where binaries and generated scripts may live, not the units**: direct exec from `/run` fails `203/EXEC`; `/usr/bin` and root-filesystem trees work, also inside the masked runner (U§3.3).

### 4.2 Not supported, or still open

- That any unit shape is safe in production: no unit shipped, no installer, no production helper or runner, no in-process `SO_PEERCRED` check in a production helper, no inherited-socket adoption (M§6, N§3.5, S§6, U§3.4).
- That `Accept=yes` is the right choice versus `Accept=no`: both work mechanically (F-ACT-1, F-ACT-2); the evidence shows cost differences only (3.1, B1).
- Any claim about systemd below 255 or SELinux hosts, containers with systemd, non-systemd hosts, Unraid, rootless runtimes.
- That the S10b difference is a version difference (2.4.1).
- Power-loss durability, `ENOSPC`, crash windows, other filesystems (F-FS-4).
- Seccomp filters, `MemoryDenyWriteExecute`, `AppArmorProfile=`, `PrivateDevices`/`PrivateTmp`/`RestrictSUIDSGID`/`AmbientCapabilities=` individually (F-MSK-16, F-MSK-17, F-LSM-4); `RestrictAddressFamilies=` and the other 5.2 candidates only for start and effect, once, with stand-ins and no real workload; `UMask=` (U's umask probe measured its own setting and is ignored, U§2).
- Helper directive set per operation group (0031 B5); netlink and nft operations as a root unit with a real directive set.
- That "independent of Yama" holds for the cross-uid ptrace denial on a Yama host: shown on the Pi (no Yama), not separated on M, S or U (F-CUS-3). It is supported by the Pi only.
- `DynamicUser=` on S and U (neither ran it); the reload-pending case of ST-07 for a running loaded unit (U tested only an unloaded instance, which systemd reads from disk on demand).
- `skills_dir` with a different `/etc/domovoi` mode or a group-readable tree (N§6, S§6): the alternatives to F-2's mask are untested.

## 5. Recommendations as concrete requirements

Everything in this section is a proposal for the implementer and for the final ADR text. "Must" is used when a measurement shows the alternative fails or opens a path; "should" is a judgement. Directives marked **(unmeasured)** have no result in any of the five reports. They are listed so the implementer measures them with the test plan of section 7 before shipping them; they are not part of the evidence.

### 5.1 Skill-runner socket unit (`domovoi-skill-runner.socket`)

```ini
[Unit]
Description=Domovoi skill runner socket

[Socket]
ListenStream=/run/domovoi-skill-runner.sock
# Mode, owner and group are the first layer (F-ACT-4). 0660 is a hard maximum:
# a 0666 socket is not protected by any unit directive (F-MSK-13).
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
# One instance per connection; see 5.5 for the choice and its costs.
Accept=yes
# Loaded as written on systemd 255; behaviour at the limit is unmeasured. The
# systemd default is 64.
MaxConnections=16
# Stopped socket units leave the node behind (S section 2); this removes it
# (measured, U section 3.3).
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
```

Reasons: `SocketUser`, `SocketGroup`, `SocketMode` (F-ACT-4: refused clients never reach the service, 5 of 5). `Accept=yes` (F-ACT-2: valid on AF_UNIX, 5 of 5). `MaxConnections=16` follows from "one sandbox and one `RuntimeMaxSec` per execution"; U loaded it as written (U§3.3), the behaviour at the limit is unmeasured. `RemoveOnStop=yes` was derived from S section 2 (nine leftover nodes with the default) and is now measured: the node is gone after stop for six units and left behind by the default (U§3.3).

### 5.2 Skill-runner service template (`domovoi-skill-runner@.service`)

```ini
[Unit]
Description=Domovoi skill runner (connection %i)
# Do not add ConditionPathExists= for the bind sources: a condition would skip
# the unit silently. A plain bind of an absent path fails the start with
# 226/NAMESPACE, which is the wanted fail-closed behaviour (F-MSK-7).

[Service]
# Type=simple is required: RuntimeMaxSec is ignored with Type=oneshot (F-CAN-5).
Type=simple
User=domovoi-skill
Group=domovoi-skill
# No SupplementaryGroups= line. An empty value does not remove the account's
# groups (F-GRP-1) and would suggest that it does. The account carries the
# control (5.7) and the start check verifies it (ST-05).

# The connection is the instance's stdin and carries the frames in both
# directions. With StandardInput=socket, LISTEN_FDS=0 (F-ACT-2).
StandardInput=socket
# Measured on 255 under the /run mask: frames flow both ways on fd 0 and stderr
# lines reach the journal (U section 3.3).
StandardOutput=journal
StandardError=journal
# /run may be mounted noexec (host-t): direct exec of anything under /run fails
# 203/EXEC. The binary lives under /usr/bin; nothing is exec'd from /run (UNIT-9).
ExecStart=/usr/bin/domovoi skill-runner
Restart=no

# Privilege.
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=
RestrictSUIDSGID=yes

# Filesystem view.
ProtectSystem=strict
# Required: strict alone leaves /home, /root and /run/user/<uid> writable (F-PRT-2).
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectProc=invisible
ProcSubset=pid
# Parent masks ALONE. No InaccessiblePaths= for any path under these parents
# (F-MSK-4, F-MSK-11). Everything below them becomes ENOENT: both sockets, the
# agent and helper state, the key, helper.toml and agent.toml (F-MSK-6).
# /etc/domovoi must already exist: if absent, this directive creates it on the
# host as an empty directory (F-MSK-10). The installer creates it.
TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro
# Plain (no dash) binds: an absent source fails the start. Never bind a .sock
# here (F-MSK-12).
BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages

# Time bound. The runner's own group kill on EOF and its per-execution timeout
# are the primary limits; this is the backstop. The cgroup kill also removes
# setsid grandchildren (F-CAN-4).
RuntimeMaxSec=@RUNTIME_MAX_SEC@
KillMode=control-group
TimeoutStopSec=5s

# Mechanism measured (F-CG-3); the values are unmeasured.
MemoryMax=@SKILL_MEMORY_MAX@
TasksMax=@SKILL_TASKS_MAX@
```

`@RUNTIME_MAX_SEC@` is rendered by the installer: the agent's maximum skill timeout plus a margin (RW-39). `@SKILL_MEMORY_MAX@` and `@SKILL_TASKS_MAX@` have no evidence behind a default and must be set from a measurement of real skills.

| Directive | Reason and evidence |
|---|---|
| `Type=simple` | `RuntimeMaxSec` works (F-CAN-4); `oneshot` ignores it (F-CAN-5, N, S and U) |
| `User=`, `Group=` | the identity of 0030; real users in M, N, S |
| no `SupplementaryGroups=` | an empty value does not drop groups (F-GRP-1, 5 of 5) |
| `StandardInput=socket` | measured with `Accept=yes` (F-ACT-2) |
| `StandardOutput=` and `StandardError=journal` | keeps stdout from being the connection. Measured on 255 with a stand-in: frames flow both ways on fd 0, `LISTEN_FDS` is unset, the runner's stderr lines reach the journal with `/run` masked (U§3.3). To be re-measured with the real runner (`runner_writes_frames_to_inherited_socket_and_logs_reach_journal_under_run_mask` in 7.1) |
| `NoNewPrivileges=yes`, `CapabilityBoundingSet=`, `AmbientCapabilities=` | views show `NoNewPrivs=1`, `CapEff=CapBnd=0` (F-MSK-6); defence in depth, no attack path was measured through them |
| `RestrictSUIDSGID=yes` | present in measured units, not isolated (F-MSK-16) |
| `ProtectSystem=strict` | `/etc` and `/var/lib` `EROFS` (F-PRT-1) |
| `ProtectHome=yes` | required (F-PRT-2, F-PRT-3) |
| `PrivateTmp=yes`, `PrivateDevices=yes` | present in measured units, not isolated; a private writable temp area is wanted (N section 3.4) |
| `ProtectProc=invisible`, `ProcSubset=pid` | other processes' `/proc/<pid>/*` `ENOENT` (F-MSK-15); information hiding, not custody |
| `TemporaryFileSystem=` for three paths | F-MSK-1, F-MSK-2, F-MSK-6; also what makes `skills/` reachable under `/etc/domovoi` 0750 (F-CUS-8) |
| `BindReadOnlyPaths=` for two sources | F-MSK-6, F-MSK-14 |
| `RuntimeMaxSec`, `KillMode=control-group`, `TimeoutStopSec` | F-CAN-4; `control-group` is the default, written out so a gate test can assert it |
| `MemoryMax`, `TasksMax` | mechanism F-CG-3; U loaded `MemoryMax=256M` and `TasksMax=64` as rendered (values are probe values, not defaults) |

The order of directives inside the file carries no meaning: systemd applies `BindReadOnlyPaths=` after `TemporaryFileSystem=` (F-MSK-6, RW-39). The ordering that matters is the installer's (5.8).

The runner under the `/run` mask writes its logs through inherited stderr, not through a path under `/run`. M section 3 row (d) showed the inherited connection works under the mask; U measured journal output through `StandardError=journal` under the mask and frames on fd 0 with `StandardOutput=journal` (two `runner-log:` lines per probe in the journal, U§3.3). That was a Python stand-in on one host; the test `runner_writes_frames_to_inherited_socket_and_logs_reach_journal_under_run_mask` in 7.1 stays, to be run with the real runner.

**Additional hardening.** On 255.4 (U§3.3) all of the directives below start, the ST-13 rows 1 to 6 are unchanged, `socket(AF_NETLINK)` and `socket(AF_PACKET)` give `EAFNOSUPPORT` (without them: netlink allowed, packet `EPERM` by capability), `AF_INET` works and `unshare(CLONE_NEWUSER)` is `EPERM` (without them: allowed). That is start and effect, once, with a stand-in. Not measured: a real skill workload under them, and `UMask=` (U's umask probe measured its own setting, U§2). Add them only after each directive is measured on the matrix of 7.5 and shown not to break the fixture skills of 7.3:

```ini
# candidates for the runner service (start and effect measured once on 255; no workload)
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
LockPersonality=yes
RestrictRealtime=yes
RestrictNamespaces=yes
SystemCallArchitectures=native
UMask=0077
# Skills need outbound network by design; netlink and the rest can go.
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
```

`RestrictNamespaces=yes` is consistent with S section 3.3 and U§3.1 H5: the design uses no user namespace, so the AppArmor userns restriction cannot reach the runner.

### 5.3 Helper socket and service

Only a baseline is given. The directive set per operation group is not designed (0031 B5), and only the rows marked measured below have a result.

```ini
# domovoi-helper.socket
[Unit]
Description=Domovoi helper socket

[Socket]
ListenStream=/run/domovoi-helper.sock
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
Accept=yes
MaxConnections=8
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
```

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
# Under /usr/bin, never under /run (noexec on host-t).
ExecStart=/usr/bin/domovoi helper serve
Restart=no

# Measured shape of the root stand-in (N section 3.1 H7, S section 3.1 H7,
# and this unit literally on 255, U section 3.3).
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/domovoi-helper
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
# nft needs CAP_NET_ADMIN (the value measured in H7). The read-only groups and
# safety.attest need none of it; this baseline shares one unit.
CapabilityBoundingSet=CAP_NET_ADMIN
AmbientCapabilities=

# One request per connection; the lock is held across it (0032 D9).
RuntimeMaxSec=60s
KillMode=control-group
TimeoutStopSec=5s
```

- `CapabilityBoundingSet=CAP_NET_ADMIN` gave `CapEff=CapBnd=0x1000`, `ptrace` of another root process `EPERM`, `/etc` and `/var/lib` `EROFS`, key read OK (N section 3.1 H7, S section 3.1 H7; U§3.3 with the unit above literally: agent client allowed, skill client `EACCES` at connect, an atomic state write inside `ReadWritePaths` leaves no temp file). Dropping the capability for groups that do not need it needs one unit per group or an in-process drop; neither is measured.
- `Accept=yes` for the helper is a judgement, not evidence: the helper holds `flock` across read, recompute, write and sign (0032 D9), so concurrent instances serialise and no in-memory state outlives a request. The mechanical evidence on `Accept=` (F-ACT-1, F-ACT-2) transfers. `Accept=no` also works (F-ACT-1) and keeps a long-lived root process. See D-2.
- `RuntimeMaxSec=60s` and `MaxConnections=8` were loaded as written on 255 (U§3.3, `RuntimeMaxUSec=1min`); the behaviour at the limit is unmeasured.
- The helper reads `/etc/domovoi/helper.toml` (0600 root); `/etc` stays readable under `ProtectSystem=strict` (F-PRT-1).
- Helper hardening candidates (U§3.3: the variant with `RestrictAddressFamilies=AF_UNIX AF_NETLINK` and the other candidates below starts on 255 and gives the same report; that `busctl` over `AF_UNIX` and nft over `AF_NETLINK` work under it is not measured, because the stand-in does neither): `RestrictAddressFamilies=AF_UNIX AF_NETLINK`, `ProtectKernelTunables=yes`, `ProtectKernelModules=yes`, `ProtectControlGroups=yes`, `LockPersonality=yes`, `RestrictNamespaces=yes`, `SystemCallArchitectures=native`.
- No helper operation that starts a unit or touches a container ships without its own measured directive set (0031 B5).

### 5.4 Runner-side EOF cancellation and process-group design

The measured behaviours are F-CAN-1 to F-CAN-6. U ran items 1 to 3 as a real unit with a Python stand-in: the child's process group differed from the runner's, three seconds after the client closed the child was dead and the instance gone, no `failed` unit, the runner logged "exit 0" (U§3.3). That is the design working as a unit; it says nothing about the Rust code. Requirements for `domovoi skill-runner`:

1. After reading and validating one request, the runner spawns the skill through `domovoi-runner` (the only place allowed to spawn, R3 and R4) with the skill in **its own process group** (`process_group(0)` in the spawn code). A runner that calls `setpgid` on its own main process fails `EPERM`, because a service's main process already leads its group; one that signals its own group kills itself and ends `failed` (M F-7, F-CAN-3).
2. While the child runs, the runner watches the connection (fd 0) for EOF or error. On EOF it sends SIGTERM to the skill's group, waits a bounded time, sends SIGKILL, collects the child and **exits 0**. systemd does not do this (F-CAN-1).
3. The runner enforces the per-execution timeout from the request with the same group kill (RW-39). `RuntimeMaxSec` is the backstop and is reached after the runner's own limit.
4. After any normal completion, refusal (`run_as_unsupported`, `impl_hash` mismatch) or cancel the runner exits 0; a refusal is a result frame, not an exit status. A non-zero exit means a defect or crash and leaves a `failed` instance (F-CAN-3) that the agent's `skill_runner_status` should surface and `reset-failed` clears.
5. A skill that leaves its group with `setsid` is still removed when the instance stops, because the cgroup is killed (N section 3.1 S8, S section 3.1 S8: grandchild dead). The cgroup is the guarantee; the group kill is the courtesy.
6. The runner uses fd 0 and never resolves a path from `LISTEN_*`. How the process adopts fd 0 without `unsafe` is evaluated in `05-socket-adoption-crate-evaluation.md`, which recommends std only (`Stdin::as_fd().try_clone_to_owned()` into a `UnixStream`, refusing a non-zero `LISTEN_FDS`, no crate); that recommendation awaits the owner's confirmation (D-6). The unit shape it relies on (`Accept=yes`, `StandardInput=socket`, `LISTEN_FDS` unset or 0) was measured in U§3.3; the adoption code itself has not been run in a unit.

### 5.5 `Accept=` choice

Decided by the owner (synthesis 00 section 5a): `Accept=yes` for the skill runner; the helper follows (5.3). Accepted costs, from the evidence: (a) the runner implements the cancel and the process-group design of 5.4; (b) failed instances linger until reset (F-CAN-3, 5 of 5); (c) the instance name is an opaque counter, pid and uid string whose format differs between builds (2.1.1), so `%i` is not a package id (F-NAR-3), narrowing through it is not available and nothing may parse the name. The reason: one instance per execution gives one sandbox, one `RuntimeMaxSec` and one cgroup (F-ACT-8) per execution on every systemd host that measured it. With `Accept=no`, children share one sandbox, a crash affects all runs and `RuntimeMaxSec` applies to the service, not to an execution (0031 B1; that cost structure follows from documented behaviour and was not measured). Both modes function (F-ACT-1, F-ACT-2). `Accept=no` would also need the fd 3 fallback of the adoption note (5.4 item 6). Decision D-2 in section 9.

### 5.6 Per-package narrowing alternative (not recommended for launch)

Measured: `TemporaryFileSystem=<pkgs>:ro` plus `BindReadOnlyPaths=<pkgs>/%i` in a template started per package gives a view of exactly one package, a sibling is `ENOENT`, writes are `EROFS`, an absent package fails closed (F-NAR-1). Unusable: `InaccessiblePaths=<pkgs>` in place of the tmpfs (F-NAR-2). Unavailable: the package in the instance name of an `Accept=yes` connection (F-NAR-3).

Shape if the owner wants it (built on F-NAR-1; not tested as a whole):

```ini
# domovoi-skill-package@.service: started by a dispatcher, not socket-activated.
# %i is validated by the dispatcher against ^[a-z0-9][a-z0-9-]{0,62}$ and by the
# agent's package index. An instance for a package that is not installed fails
# closed with 226, because the bind source is absent.
TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro
BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages/%i
```

What makes it costly: something must start the instance and give the agent a way to reach it (a per-instance socket or a connection handoff), and starting a unit is a privileged action the helper must not offer generically (RW-39). Hence the recommendation of the static view (0031 B4).

### 5.7 Account, directory and mode requirements

The installer creates the items below in the order of 5.8. "Verified by" names the check that re-asserts the item on every start and in `selftest` (5.9); the installer does not rely on its own creation having worked.

| Item | Owner and mode | Evidence | Verified by |
|---|---|---|---|
| user `domovoi-agent` | system user, primary group `domovoi-agent`, supplementary groups exactly `systemd-journal`; shell `nologin`; home `/var/lib/domovoi-agent`, not created by `useradd` | N section 3.6; F-GRP-3 | ST-02, ST-04 |
| user `domovoi-skill` | `useradd --system --user-group --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin domovoi-skill`; **no supplementary group of any kind; never added to `docker`, `domovoi-agent`, `systemd-journal`, `sudo`, `adm` or any other group** | M section 1 (the command); F-GRP-1, F-GRP-2, F-GRP-4 | ST-03, ST-04, ST-05 |
| numeric ids | never hard-coded; `useradd --system` allocates them dynamically (999/984 and 997/983 on the Pi, 995/994 on host-c and host-t) | N section 3.6; F-DOC-7 | ST-09 |
| `/etc/domovoi` | `root:domovoi-agent 0750` (decided, D-1) | M, N and U used this; F-CUS-8 | ST-10 |
| `/etc/domovoi/skills` | `root:root 0755`; files 0644, scripts 0755; nothing writable except by root | the bind mount point keeps the directory's own mode, and it must be traversable through the tmpfs parent (F-CUS-8) | ST-10 |
| `/etc/domovoi/helper.toml` | `root:root 0600` | `04` section 8.3 | ST-10 |
| `/etc/domovoi/agent.toml` | `root:root 0644` per `04` section 8.3; reach is limited by the 0750 parent | `04` section 8.3 | ST-10 |
| `/var/lib/domovoi-agent` | `domovoi-agent:domovoi-agent 0700` | M section 3 row (b) | ST-10 |
| `/var/lib/domovoi-helper` | `root:root 0700`, created by `helper init`, which never accepts an existing directory with another mode | F-CUS-1; F-CUS-5 | helper start check, ST-11 |
| `attest.key`, `state`, `state.lock` | `root:root 0600`, regular files, not symlinks | F-CUS-1, F-CUS-4 | helper start check, ST-11 |
| `/var/lib/domovoi-packages` | `domovoi-agent:domovoi-agent 0755`; files 0644, scripts 0755 | F-CUS-9; RW-39 | ST-10 |
| socket nodes | `root:domovoi-agent 0660`, created by the socket units | F-ACT-4 | ST-12 |

`/etc/domovoi` stays 0750 and the mask carries `skills/`: decided by the owner (D-1, synthesis 00 section 5a). The consequence the implementer must accept is that the runner cannot work without the mask, and that `selftest` must report a missing mask as an outage (ST-13, measured to do so on U with an unmasked runner and with a drop-in that clears the mask, U§3.4). If a local drop-in removes the `TemporaryFileSystem=` line, skills stop working (fail closed); they do not leak. Binaries and generated scripts live under `/usr/bin`, the skill trees or the private `/tmp`, never under `/run`, which may be mounted `noexec` (U§3.3).

### 5.8 Installer ordering

1. Create the groups and accounts (5.7); confirm with `getent` that `domovoi-skill` is a member of no group other than its own.
2. Create `/etc/domovoi`, `/etc/domovoi/skills`, `/var/lib/domovoi-agent` and `/var/lib/domovoi-packages` with the modes of 5.7. Do not rely on `TemporaryFileSystem=` to create any of them: it creates an empty host directory silently (F-MSK-10) and `systemd-analyze verify` says nothing (F-MSK-4).
3. Run `domovoi helper init`: it creates `/var/lib/domovoi-helper` 0700 with key and state and refuses to overwrite. No runner directive names the helper directory, so its absence cannot break the runner, but the helper must not start without it.
4. Install the unit files (two sockets, two service templates, the agent unit with `User=domovoi-agent`) root-owned 0644, with every `ExecStart=` under `/usr/bin` (never under `/run`). Install no drop-ins.
5. `systemctl daemon-reload`, then enable and start the sockets.
6. Run `domovoi agent selftest --installer` as root (ST-01 to ST-20, including the isolation probe). A failed check leaves the sockets stopped and names the check.
7. Only then start the agent as `domovoi-agent`.

The installer, like the helper start, never repairs a wrong mode or owner it finds on an existing path: it stops, reports each path with the found and the expected mode and owner, and leaves the repair to the explicit, owner-confirmed command of D-11.

### 5.9 `domovoi agent selftest` and start checks, in full

Two kinds. **Start checks** run inside the helper and the runner at every start and fail closed: all work is refused and a reason reaches `helper_status` or `skill_runner_status`. **Selftest checks** run from the agent (`selftest`) or as root (`selftest --installer`). A check the agent cannot perform, because it is denied access by design, is done by root or by the isolation probe inside the runner.

Results: `pass`, `fail` (fails closed), `warn` (reported, not blocking), `info`, and `nd` (not run on this host or layout; it is never a `pass` and never satisfies a gate that requires the check). The reason strings are proposals and are not the nine tokens of 0032 (those belong to the R-10 synthesis).

| Id | Run by | Check | Expected | On failure |
|---|---|---|---|---|
| ST-01 | agent | effective uid | uid of `domovoi-agent`, not 0 | `fail`: `privilege_boundary_not_in_effect`; hosts where the agent runs as root keep the label of RW-39 |
| ST-02 | agent | `getgroups()` | exactly {`domovoi-agent`, `systemd-journal`}; no `docker`, no gid 0 | `fail`: `agent_groups_unexpected` with the gids (F-GRP-3, F-DOC-4) |
| ST-03 | root | `getent passwd domovoi-skill`; scan of `getent group` for its membership | exists; uid not 0 and not the agent's; shell `nologin`; listed as a member of no group | `fail`: `skill_account_invalid` |
| ST-04 | root | `getent group docker` | neither Domovoi account is a member (the login user of a Docker host normally is, and is not a Domovoi account: S, U§3.6) | `fail`: `docker_group_member` (F-DOC-4, F-DOC-5) |
| ST-05 | runner, at start | `getgroups()` | exactly [primary gid of `domovoi-skill`] | `fail`: `runner_groups_extra` (F-GRP-1, F-GRP-2) |
| ST-06 | runner, at start | `/proc/self/status` | `CapEff` and `CapBnd` zero, `NoNewPrivs` 1 | `fail`: `runner_privileges` |
| ST-07 | root | `systemctl show` with the properties `LoadState`, `FragmentPath`, `DropInPaths`, `NeedDaemonReload`, `Accept`, `SocketMode`, `SocketUser`, `SocketGroup`, `Type`, `User`, `RuntimeMaxUSec` for the two sockets, the agent unit and the two templates, **queried through an instance name** (`domovoi-skill-runner@selftest.service`, `domovoi-helper@selftest.service`; systemd rejects the bare template name and nothing parses the instance name) | shipped values; `LoadState=loaded`; `FragmentPath` the installed path; `DropInPaths` empty; `NeedDaemonReload=no`; `SocketMode=0660`; runner `Type=simple`; `RuntimeMaxUSec` equal to the rendered value after parsing the duration text (`6s`, `1min` and `infinity` were seen) | `fail`: `unit_modified` for a differing value or any drop-in; `fail`: `unit_not_inspectable` when `systemctl show` exits non-zero or `FragmentPath` is empty (never a skip or a pass). `systemd-analyze verify` is not a check (F-MSK-4). Corrected wording and cases: 5.9.1 |
| ST-08 | root | record the systemd version string and build; no comparison with a version | `info` | none. There is no version floor (D-3, D-4): the tier follows from the checks that read effective isolation (ST-13 above all), and an unsupported directive shows up as a failing unit start or a failing check |
| ST-09 | root | numeric uid or gid in any shipped unit or config; any `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` path under `/run` or `/tmp` (proposed addition; `/run` may be `noexec`) | none | `fail`: `unit_hardcoded_id`; `fail`: `unit_exec_under_run` |
| ST-10 | root | stat of every path in 5.7 (type, owner, group, mode); walk of `skills/` and `domovoi-packages` for write bits | the table of 5.7 | `fail`: `path_mode_wrong`, reported for every offending path with found mode and owner and expected mode and owner; never repaired by the check (D-11) |
| ST-11 | root | `fstat` of the helper directory, `attest.key`, `state`, `state.lock`, opened with `O_NOFOLLOW` | 0700, 0600, 0600, 0600; uid 0; regular files | `fail`: `helper_state_permissions`, reported per path with found and expected mode and owner. A wrong existing mode is refused, never repaired at start (F-CUS-5, D-11); the repair is the explicit owner-confirmed command |
| ST-12 | agent or root | stat of both socket nodes | socket type, `root:domovoi-agent`, 0660 | `fail`: `socket_mode_wrong` (0666 is a fail, F-MSK-13) |
| ST-13 | root | start one probe instance of the runner unit in `probe` mode and read its isolation report (list below, with the tolerant forms of 5.9.1) | list below | `fail`: `runner_not_isolated{what}` |
| ST-14 | agent | connect to both sockets | succeeds | `error{reason}` of the RW-42 status |
| ST-15 | agent | open `attest.key`; list `/var/lib/domovoi-helper`; open `state.lock`; read `/proc/<helperpid>/environ` | `EACCES` for all | `fail`: `agent_reaches_helper_state` (F-CUS-1, F-CUS-2) |
| ST-16 | agent | `connect` to the Docker socket | `EACCES` or absent | `fail`: `agent_reaches_docker` (F-DOC-4) |
| ST-17 | root | container installs: the agent container has no read-write `docker.sock` bind, no `--privileged`, no bind of `/` or of the helper directory | none | `fail`: `container_escape_surface` (F-DOC-5, F-DOC-6) |
| ST-18 | root | `/etc/domovoi` is not an empty 0755 directory created by a mask | mode and owner as 5.7 | `fail`: `host_directory_created_by_mask` (F-MSK-10) |
| ST-19 | root | record `/sys/kernel/security/lsm`, Yama `ptrace_scope`, `apparmor_restrict_unprivileged_userns` and `..._unconfined`, the AppArmor label of the probe instance, the mount options of `/run` and `/tmp` | recorded | `info` (F-LSM-1, F-LSM-2) |
| ST-20 | root | ten write cycles (write, `fsync`, `rename`, `fsync` of the directory) on the state filesystem | median below a threshold chosen in D-10 | `warn`: `state_write_slow` (F-FS-2: 1.2 ms to 67 ms; 1.43 ms on U) |

The isolation report of ST-13 runs as the runner inside the real unit. A directive that is in the file proves nothing; only the effective view answers R-9 (J section 0):

1. `connect()` to both socket paths gives `ENOENT` (the mask), not `EACCES`.
2. `stat`, `listdir` and `open` of `/var/lib/domovoi-agent`, `/var/lib/domovoi-helper` and `attest.key` give `ENOENT`.
3. `listdir('/etc/domovoi')` is `['skills']`, `listdir('/var/lib')` is `['domovoi-packages']`; `helper.toml` and `agent.toml` give `ENOENT`. `/run` holds at most the root-only staging directory: `listdir('/run')` is a subset of `['systemd']`, `listdir('/run/systemd')` is a subset of `['incoming']`, and if `/run/systemd/incoming` exists it is a directory owned by uid 0 with `mode & 0o077 == 0`. Any other entry fails (the first wording, "`listdir('/run')` is empty", failed on 255).
4. Create, rename and unlink in `/etc/domovoi/skills` and `/var/lib/domovoi-packages` give exactly `EROFS`; opening an existing file for writing (append) never succeeds and gives `EROFS` or, when the skill does not own the file, `EACCES` (the first wording, "give `EROFS`", failed for append on three hosts). The read-only property itself is carried by the create, rename and unlink results together with point 7.
5. `/home` and `/root` give `EACCES` or are absent; `/run/user` is absent.
6. `/proc/<helperpid>/` (the pid supplied by the root caller) and `/proc/1/environ` give `ENOENT`.
7. Group list, `CapEff`, `CapBnd`, `NoNewPrivs`, uid, gid, AppArmor label (informational) and the mount points of the three masks from `/proc/self/mountinfo`, with the `ro` option required on both bind mounts and on the three masks.
8. `/etc/shadow` gives `EACCES` (informational only: the same result appears without the unit, J section 4 S7).

`EACCES` where `ENOENT` is expected is a failed mask; it is the view of a polluted-group account without the mask (M section 3 row (c)), and the check must tell the two apart. U ran this distinction as negatives: the runner without the masks and the corrected runner with a drop-in that clears them both fail `runner_not_isolated`, with the note that tells `EACCES` from `ENOENT`, and pass again after the drop-in is removed (U§3.4).

#### 5.9.1 Corrected wording of ST-07 and of ST-13 points 3 and 4, with the negative cases

The first wording of these three expectations failed on a real host although the isolation held (U§3.3 and §3.4; checked against `probes/host-t3/results/sys-g_corrected.txt`, `sys-h_run_diag.txt` and `sys-i_drift.txt`). Implemented literally, a sound host would have been classified Blocked, a false negative. The corrected forms fail on structure (unexpected entries, a missing read-only flag, an uninspectable unit), not by accepting any error code. T = run on U with the Python stand-in; D = designed here, not run.

| Check | Wording that failed | Observed | Corrected wording | Negative cases |
|---|---|---|---|---|
| ST-07 | `systemctl show` of the template names with the property `RuntimeMaxSec` | the bare template name is rejected ("neither a valid invocation ID nor unit name") on 255.4, 257.13, 259.5 and 259.9 (`probes/r8`, `probes/pi8`, `probes/host-c8`, U§3.3); the loaded property is `RuntimeMaxUSec`, printed as `6s`, `1min`, `infinity`; an unloaded instance is read from disk on demand | query `<template>@selftest.service` and read `RuntimeMaxUSec`; compare the parsed duration with the rendered value; a non-zero exit of `systemctl show` or an empty `FragmentPath` is `unit_not_inspectable`, never a skip or a pass | T: a drop-in on the runner template gives `unit_modified` with `DropInPaths` listing it, also before any `daemon-reload`; the check passes again after the drop-in is removed. D: `unit_not_inspectable` path; a drop-in that changes `RuntimeMaxSec`; the reload-pending case (`NeedDaemonReload=yes`) for a loaded running unit, which U did not test |
| ST-13 point 3 | `listdir('/run')` is empty | 255.4: `['systemd']`, and `/run/systemd` holds only `incoming`, a root tmpfs `ro,nosuid,nodev,noexec` of mode 0600, for the mask alone and for each directive combination tried (`sys-h_run_diag.txt`); 259.5: `[]` (`probes/realuser/results/t_b.txt:170`); 257.13 and 259.9 not recorded | `listdir('/run')` is a subset of `['systemd']`; `listdir('/run/systemd')` is a subset of `['incoming']`; if `incoming` exists: a directory, uid 0, `mode & 0o077 == 0` (the parent is 0755, so the skill can `stat` it). An empty `/run` passes. Anything else, in particular a socket, fails | T: the runner without the masks and the runner with a clearing drop-in both fail `runner_not_isolated` overall (U§3.4; the saved `what=` list is cut by the probe script before the `/run` item, so the specific `/run` reason is not in the committed output). D: a re-bound `.sock` appearing as an extra entry (the exposure itself is measured, F-MSK-12); an `incoming` that is not root-owned or is group- or world-accessible |
| ST-13 point 4 | create, append, rename and unlink all give `EROFS` | create, rename, unlink `EROFS`; append to a root-owned skills file and to an agent-owned package file `EACCES` (DAC is checked before the mount flag); the same on S and in M's real-user pass | create, rename, unlink exactly `EROFS`; append never succeeds and gives `EROFS` or `EACCES`; point 7 requires `ro` on both binds. The append result alone cannot tell a read-only mount from DAC, which is why the property rests on create, rename, unlink and the mount options together | D (no writable-bind negative was run): a writable bind lets `create` succeed and lacks `ro` in `mountinfo`, so it fails twice; an append that succeeds fails |

The other points of the original wording (1, 2, 5, 6, 8) passed as written on U. The tolerant `/run` form has been checked against the raw listings of 255.4 and 259.5 only: the `/run` listing under the mask was never recorded on 257.13 and 259.9, so a different `/run/systemd` entry there could still produce a false Blocked (section 10 lists the measurement). How these checks gate platform tiers is in D-3 and D-4.

### 5.10 Fail-closed rules and tokens (isolation side)

The attestation tokens are in the R-10 synthesis; these cover isolation.

- **Runner**: a failure of ST-05, ST-06 or the ST-13 report makes `skill_runner_status` `unsupported{reason}` or `error{reason}`; every skill is unavailable. It never falls back to running as `domovoi-agent` (0030 D6).
- **Helper start**: before reading the key the helper `fstat`s its directory and files, opened with `O_NOFOLLOW`: type, owner, mode. A wrong value is `helper_state_permissions`, a token distinct from `state_corrupt`, because an unopenable lock reported as `state_corrupt: lock` hides a permission fault (M F-6). The helper refuses to start and does not `chmod` at start. The refusal report names every offending path with found mode and owner and expected mode and owner. Self-healing is offered as a separate, explicit, owner-confirmed root command, never as part of start (D-11, decided by the owner). These tokens must be reconciled with 0032 C2 and the R-10 synthesis before the implementation freezes.
- **`unsupported`** is also the result for a unit property or feature the host does not honour; on a host that passes every other check it leads to the tier of D-3 and D-4, not to a version refusal.

### 5.11 AppArmor, Yama and LSM stance

- The design must not depend on AppArmor, SELinux or Yama. Every custody result holds by DAC and namespaces alone on the Pi, which has neither LSM nor Yama (F-CUS-1 to F-CUS-3). On the two hosts with AppArmor and Yama (S, U) the directive results were the same and no probe unit was confined (F-LSM-1).
- Yama is a bonus: it separates executions of one `domovoi-skill` uid (F-LSM-5). `selftest` reports it as `info` (ST-19) and does not require it. The documentation must not claim same-uid isolation on hosts where it is off.
- The runner must not use user namespaces (`unshare`, `bwrap` without root): on Ubuntu 24.04 and later `apparmor_restrict_unprivileged_userns=1` blocks them for an unprivileged account (F-LSM-2; measured on 24.04.5 and 26.04). A future design that needs them ships an AppArmor profile; that is decisions D-5 and D-9.
- No `AppArmorProfile=` or `SELinuxContext=` ships at launch (F-LSM-4, F-LSM-6 unmeasured). A profile added by an owner's drop-in is outside any support statement; ST-07 reports a drop-in as `unit_modified`.

### 5.12 Containers and non-systemd hosts: what ships at launch

This follows 0031 B2. It rests partly on absence of evidence.

- The helper and the skill runner ship for systemd hosts only. The measured hosts are Debian 13 with systemd 257.13, Ubuntu 26.04 with 259.9 and Ubuntu 24.04.5 with 255.4; WSL2 with 259.5 is a research environment. There is no systemd floor: the owner decided (synthesis 00 sections 5a and 5b) on platform tiers gated by the runtime isolation check (D-3, D-4). 255.4 is the lowest build measured on a real host, and it passed with stand-ins.
- Container installs (compose, Unraid template) and hosts without systemd keep today's agent, labelled "privilege boundary not in effect", with no skill execution through the runner and no helper (RW-39, `04` section 7.3). The evidence shows why socket mounts do not substitute: a 0660 socket bind-mounted into a container is matched by numeric gid, which is host-specific (F-DOC-7), and a mounted `docker.sock` is root (F-DOC-6). `deploy/compose/agent.yml` already routes Docker through a socket proxy with a read-only socket mount; that proxy was not measured and is not endorsed here.
- A supervisor-based design for Unraid and other non-systemd hosts needs its own measurement (J section 3 H9).

### 5.13 Docker stance (D-24)

- No Domovoi account is in the `docker` group; ST-04 fails if one is (F-DOC-4, F-DOC-5). The login user of a Docker host normally is a member and may also be in `lxd` (S, U§3.6); that is not a Domovoi account, and `lxd` membership was not tested.
- The agent unit and any agent container get no `docker.sock` mount and no read-write bind of it (ST-16, ST-17; F-DOC-6).
- What the helper may do with Docker: only typed, enumerable operations by container id through a first-party narrow client, which D-24 requires to have its own ADR. The helper is root and can reach the socket by DAC. The evidence says any path that lets a caller choose an API path, a bind mount, an image or a user is root on the host (F-DOC-5). So no Docker operation ships before that ADR, and `container.restart` stays blocked for unattended use (RW-39, RW-12).
- Whether `connect` to the Docker socket works from the helper unit under `ProtectSystem=strict` was not measured; neither was a Docker-reaching directive set (F-DOC-8).

### 5.14 `arch-check` and gate rules to add

Each needs an ADR when it changes `xtask/src/arch/rules.rs` (0030 D7). They extend `HELPER-1`, `HELPER-2`, `RUNNER-1`, `RUNNER-2`, `IDENT-1` (0030) and `UNIT-1` (0031).

| Rule | Checks | Backs |
|---|---|---|
| UNIT-1 (extended) | a parser over `deploy/systemd/`: the runner unit has no `InaccessiblePaths=` entry under a `TemporaryFileSystem=` parent | F-MSK-4, F-MSK-11 |
| UNIT-2 | the runner unit has no non-empty `SupplementaryGroups=` and no `Group=` other than `domovoi-skill`; the string `docker` appears in no `Group=` or `SupplementaryGroups=` | F-GRP-1, F-DOC-4 |
| UNIT-3 | the runner unit contains each required directive of 5.2 with its value | F-PRT-2, F-MSK-6 |
| UNIT-4 | a unit with `RuntimeMaxSec=` has `Type=simple` | F-CAN-5 |
| UNIT-5 | both socket units: `SocketMode=0660`, `SocketUser=root`, `SocketGroup=domovoi-agent`, `ListenStream` is one of the two literals | F-ACT-4, F-MSK-13 |
| UNIT-6 | no `BindPaths=` or `BindReadOnlyPaths=` entry ends in `.sock`; no `/run` child is bound into the runner | F-MSK-12 |
| UNIT-7 | every path in a plain `InaccessiblePaths=`, `BindReadOnlyPaths=`, `TemporaryFileSystem=` or `ReadWritePaths=` is in the installer's created-set (one list in one file) or is `/var/lib` or `/run` | F-MSK-7, F-MSK-10 |
| UNIT-8 | the agent unit has `User=domovoi-agent`; no unit file or compose file binds `docker.sock` read-write into the agent | D-29, F-DOC-6 |
| UNIT-9 | no `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` of a shipped unit has a path under `/run` or `/tmp` | direct exec under a `noexec` `/run` fails `203/EXEC` (U§3.3) |
| ID-2 | the account names and the group-membership check live in one module; no numeric uid or gid literal next to them | F-DOC-7 |
| ID-3 | no code parses, builds or compares a systemd instance name | the name format differs between builds (2.1.1) |
| SELFTEST-1 | each ST id and each start-check reason string has a named test (7.2) | all |
| INSTALL-1 | the installer creates each directory of 5.7 before the line that enables a socket | F-MSK-10 |

None replaces the run-time checks: a file can say anything and only the effective view is the answer (0031 "What the checker cannot see").

## 6. Improvement ideas beyond the evidence

Every item here is an idea. None has a measurement behind it; each says what it might buy, what it costs and how to find out.

| Id | Idea | Expected benefit | Cost | How to test |
|---|---|---|---|---|
| I-1 | Re-run the isolation probe (ST-13) periodically and after every `daemon-reload` or unit change, and report a hash of each unit's fragment and drop-ins to the hub next to `skill_runner_status` | catches drift (a drop-in that removes the mask) between installs; the checker cannot see it (0031) | one probe instance per interval; a new status field and hub display | privileged test: add a drop-in that clears `TemporaryFileSystem=`, expect the next probe to fail within one interval; measured once on U with the stand-in checks: a drop-in was caught by ST-07 (also before any reload) and by ST-13 (U§3.4) |
| I-2 | Allowlist view for the runner: `TemporaryFileSystem=/:ro` plus `BindReadOnlyPaths=` of only `/usr`, `/lib`, `/lib64`, `/bin`, `/etc/ssl`, `/etc/resolv.conf`, `/etc/passwd` and the two skill trees, instead of today's denylist of three masks | closes every unlisted readable path (`/srv`, `/mnt`, `/opt`, `/var/log`, any data directory), which F-MSK-6 does not hide and RW-39 accepts as world-readable | skills that need other paths break; the allowlist must follow the distribution layout; interpreters (python, bash) pull in libraries | run the fixture skills of 7.3 under the allowlist view on Debian and Ubuntu; compare `listdir('/')` and reachable world-readable files with the current form |
| I-3 | `DynamicUser=yes` for the runner instance instead of a static `domovoi-skill` | removes the account-pollution class (F-GRP-1: a dynamic user has no group list) and gives each execution its own uid, so same-uid sibling reads (S section 3.4: `/proc/B/environ` readable) disappear without Yama | contradicts the literal `domovoi-skill` of D-31 and `run_as`; `DynamicUser=` implies `ProtectSystem=strict` and a private state area kept after stop (F-CG-1); no stable owner for packages (they stay 0755 agent-owned, readable by all) | measure the isolation matrix and the ST-13 list with `DynamicUser=yes` on the three hosts; check the instance's uid differs per connection; confirm `skills/` and packages remain readable |
| I-4 | Landlock applied by the runner to itself, as a second, systemd-independent view restriction | the view survives a unit that drifts; needs no user namespace and no AppArmor rule (S and U list `landlock` among active LSMs) | a new dependency (R11) or `unsafe` syscalls (R7); kernel 5.13 or later; behaviour on WSL2 and the Pi unmeasured | probe the three hosts: install a ruleset limiting the process to the two trees and its connection fd, repeat the ST-13 list with the unit masks removed |
| I-5 | Per-execution cgroup via `Delegate=yes`: the runner creates a child cgroup per skill, sets `memory.max` and `pids.max` on it and ends it with `cgroup.kill` | a clean kill of the skill's whole tree without signalling a group, so the runner exits 0 (answers F-CAN-3 and the `setpgid` problem); per-execution limits independent of the unit (F-CG-2: works after the process leaves the delegated cgroup) | `Delegate=yes` gives the skill account write access to its own cgroup tree; the no-internal-process rule makes setup fiddly (`EBUSY` when moving back, F-CG-2); a bug could leave a cgroup | privileged test: run a fork bomb and a `setsid` grandchild under the child cgroup, assert both die and the runner exits 0 |
| I-6 | Egress control for the runner through `IPAddressDeny=` and `IPAddressAllow=` (cgroup BPF) | a compromised skill cannot reach the hub port, the helper's neighbours or the LAN, which F-MSK-6 leaves open | needs the BPF cgroup feature; skills legitimately need outbound; the hub address is a configuration value; interacts with the nft work of R-2 | probe from a runner instance: connect to the hub port, a LAN address and a public address; assert the first two fail and the third follows policy |
| I-7 | A minimal peer check in the helper beyond uid: `SO_PEERPIDFD` plus the peer's `/proc/<pid>/cgroup`, accepting only the agent unit's cgroup | refuses any process of the agent uid that is not the agent service (for example a hand-started process) | Linux 6.5 or later for the pidfd option; adds a `/proc` read in the helper; cgroup path is host-specific | privileged test: connect from a `setpriv --reuid=agent` shell and from the agent unit; assert only the unit is accepted. Low value while the agent account has `nologin` and the mask is in place |
| I-8 | Gate on the `systemd-analyze security --offline` exposure score of each shipped unit (J section 3 H7 recorded 5.7 MEDIUM for one probe unit; U recorded 5.1 MEDIUM for the runner template and 6.0 MEDIUM for the helper template, offline, on 255, U§3.3) | a regression alarm when a directive is removed | the score is not a verdict (J says so); thresholds drift with systemd versions | record the score per unit per systemd version in CI and fail on a rise of more than a set amount; do not treat the absolute value as a requirement |
| I-9 | One generator renders all five units from a single table (paths, modes, directive list), and the arch-check parser reads the same table | the UNIT rules and the installer cannot disagree; the created-set of UNIT-7 is derived | a generator to maintain; units are no longer hand-readable files in the repository | a test that the rendered files equal the committed `deploy/systemd/*` files |
| I-10 | `selftest` reports measured state-write latency (ST-20) to the hub as a host fact | surfaces slow-storage hosts (1.2 ms versus 67 ms median, a 55-fold spread) before a state-write path is on the critical path | one more field | covered by ST-20 on the Pi |
| I-11 | A per-host `domovoi-probe` helper command that prints the full evidence table of this synthesis for the host (versions, LSMs, Yama, mounts, group lists, the ST-13 list) | the validation matrix of 7.5 runs by one command on any distribution, and owners can attach the output to an issue | one more subcommand; must be read-only and never create files | run on each matrix host; compare against the table in section 1 |

## 7. Test plan

Naming follows `AGENTS.md`: tests are named after behaviour. "Tier U" runs in ordinary CI without root. "Tier P" needs root and a real systemd as PID 1, creates only `domovoi-probe-ci-*` users, groups, units and paths, removes them by a teardown trap and never touches the real names (the pattern of M section 2 and S section 2). Tier P is not run by `cargo xtask ci` by default; section 7.4 says how CI can run it.

### 7.1 Tests mapped to findings

| Test | Tier | Finding | Asserts |
|---|---|---|---|
| `startup_check_accepts_primary_group_only` | U | F-GRP-2 | the runner start check passes when the identity port returns only the primary gid |
| `startup_check_rejects_extra_group` | U | F-GRP-1 | with one extra gid the check fails closed with `runner_groups_extra` |
| `startup_check_rejects_nonzero_capabilities` | U | F-MSK-6 | a status text with `CapEff` not zero is `runner_privileges` |
| `startup_check_requires_no_new_privs` | U | F-MSK-6 | `NoNewPrivs: 0` fails |
| `helper_start_refuses_directory_mode_0755` | U (tempdir) | F-CUS-5 | an existing 0755 helper directory is refused, not chmodded |
| `helper_start_refuses_key_with_group_or_world_bits` | U | F-CUS-5 | key mode 0640 or 0644 refused |
| `helper_start_refuses_symlinked_key` | U | F-CUS-1 | a symlink at `attest.key` is refused (`O_NOFOLLOW`) |
| `helper_reports_permission_fault_not_corruption_for_unopenable_lock` | U | F-CUS-6 | an unopenable lock gives `helper_state_permissions`, not `state_corrupt` |
| `helper_init_creates_directory_0700_and_refuses_existing` | U | F-CUS-5 | `helper init` creates 0700, never overwrites |
| `runner_puts_skill_in_its_own_process_group` | U (Linux) | F-CAN-3 | the child's pgid differs from the runner's |
| `runner_kills_skill_group_on_eof_and_exits_zero` | U (Linux, socketpair) | F-CAN-2, F-CAN-3 | closing the peer ends the skill and its child; the runner exits 0 |
| `runner_does_not_signal_its_own_group` | U (Linux) | F-CAN-3 | the runner survives the kill and exits cleanly |
| `runner_enforces_request_timeout_before_runtime_max` | U (fake clock) | F-CAN-4 | the group is killed at the request timeout, earlier than the unit backstop |
| `runner_refuses_run_as_other_user` | U | D-31 | `run_as` other than `domovoi-skill` gives a refusal frame and exit 0 |
| `runner_refuses_impl_hash_mismatch` | U | RW-39 validation | mismatch is refused without executing |
| `unit_parser_flags_rw39_inaccessible_child_under_masked_parent` | U | F-MSK-4, F-MSK-11 | the literal RW-39 directive list fails UNIT-1 |
| `unit_parser_flags_dash_prefixed_mask_of_installer_path` | U | F-MSK-8 | `-` on a path in the created-set fails UNIT-7 |
| `unit_parser_flags_runtime_max_with_oneshot` | U | F-CAN-5 | UNIT-4 |
| `unit_parser_flags_missing_protect_home` | U | F-PRT-2 | UNIT-3 |
| `unit_parser_flags_nonempty_supplementary_groups_and_docker` | U | F-GRP-1, F-DOC-4 | UNIT-2 |
| `unit_parser_flags_wide_socket_mode` | U | F-MSK-13 | UNIT-5 with 0666 |
| `unit_parser_flags_socket_bind_into_runner` | U | F-MSK-12 | UNIT-6 |
| `unit_parser_flags_agent_unit_without_user` | U | D-29 | UNIT-8 |
| `shipped_units_pass_every_unit_rule` | U | all UNIT rules | the files under `deploy/systemd/` pass |
| `installer_creates_mask_and_bind_paths_before_enabling_sockets` | U (script parse) | F-MSK-7, F-MSK-10 | INSTALL-1 |
| `selftest_reports_every_check_with_its_id` | U | 5.9 | ST-01 to ST-20 appear in the report, each with a reason string, with fakes for the host ports |
| `isolation_report_distinguishes_eacces_from_enoent` | U | M section 3 row (c) | an `EACCES` where `ENOENT` is expected is `runner_not_isolated` |
| `isolation_report_accepts_systemd_incoming_and_empty_run_and_rejects_any_other_entry` | U | 5.9.1, U§3.3 | `/run` empty or `['systemd']` with `incoming` root-owned and not group- or world-accessible passes; a socket or any other entry fails |
| `isolation_report_accepts_eacces_for_append_to_unowned_file_but_not_success` | U | 5.9.1, F-MSK-14 | append gives `EROFS` or `EACCES` and passes; a successful append fails |
| `isolation_report_requires_ro_on_both_binds_and_erofs_for_create_rename_unlink` | U | 5.9.1 | a writable bind (create succeeds, no `ro` in mountinfo) fails |
| `selftest_reads_unit_properties_through_an_instance_name_and_runtime_max_usec` | U | 5.9.1, U§3.3 | the query names an instance, reads `RuntimeMaxUSec` and compares parsed durations (`6s`, `1min`, `infinity`) |
| `selftest_treats_a_failing_systemctl_show_as_unit_not_inspectable` | U | 5.9.1 | a non-zero exit or an empty `FragmentPath` is a `fail`, not a skip |
| `selftest_flags_exec_start_under_run` | U | UNIT-9, U§3.3 | `ExecStart=` under `/run` fails `unit_exec_under_run` |
| `helper_repair_requires_confirmation_and_prints_the_plan` | U (tempdir) | D-11 | the command lists each path with found and expected mode and owner and changes nothing without the owner's explicit confirmation |
| `helper_repair_refuses_symlinks_and_unexpected_types_and_reruns_the_checks` | U (tempdir) | D-11 | a symlink or wrong type is refused; the checks run again afterwards and report what is left |
| `helper_repair_is_not_reachable_from_start_agent_hub_or_chat` | U | D-11 | no start path, socket frame, chat command or policy invokes the repair |
| `runner_adopts_stdin_connection_without_unsafe` | U (Linux) | D-6, 05 section 7 | the connection on fd 0 becomes a stream, with `refuses_listen_fds_greater_than_zero` and `refuses_a_non_socket_descriptor` of 05 section 7 |
| `spawned_skill_gets_no_listen_vars_and_no_connection` | U (Linux) | D-6, 05 section 7 | the skill child sees no `LISTEN_*` and no socket descriptor |
| `untested_platform_allows_skills_with_recorded_acknowledgement_and_refuses_write_groups_without_opt_in` | U (fakes) | D-3, D-4 | the tier table of D-3 and D-4 as code, with fake check results |
| `blocked_platform_refuses_skills_and_write_groups` | U (fakes) | D-3, D-4 | any `fail` or required `nd` blocks |
| `real_user_skill_cannot_connect_to_helper_or_runner_socket` | P | F-ACT-4 | `EACCES` at connect, instance count unchanged |
| `real_user_skill_view_hides_sockets_state_and_key_under_corrected_unit` | P | F-MSK-6, ST-13 | the eight-point report passes |
| `rw39_unit_text_fails_with_226` | P (canary) | F-MSK-4 | documents the failure; fails if a future systemd changes it, so the ADR text is revisited |
| `plain_mask_on_absent_path_fails_closed` | P (canary) | F-MSK-7 | `226/NAMESPACE` |
| `dash_mask_on_absent_path_starts_and_skips` | P (canary) | F-MSK-8 | starts, no mask |
| `temporary_filesystem_on_absent_path_creates_host_directory` | P (canary) | F-MSK-10 | the directory appears; the test removes it |
| `runtime_max_sec_is_ignored_for_oneshot` | P (canary) | F-CAN-5 | warning and full runtime |
| `runtime_max_sec_stops_simple_unit_and_kills_grandchild` | P | F-CAN-4 | stopped near the limit, `setsid` grandchild dead |
| `closing_connection_does_not_end_instance_without_runner_cancel` | P (canary) | F-CAN-1 | an instance of a non-watching service is still active after 3 s |
| `failed_instance_lingers_until_reset_failed` | P (canary) | F-CAN-3 | `failed` after a non-zero exit; cleared by `reset-failed` |
| `polluted_skill_account_fails_start_check_and_is_still_masked` | P | F-GRP-1, F-GRP-4 | with a stray group the start check fails, and with the mask the sockets are `ENOENT` |
| `skill_given_agent_group_is_stopped_by_mask_not_by_group` | P | F-MSK-2 | sockets `ENOENT` with the group granted |
| `skill_with_agent_group_and_no_mask_is_stopped_by_helper_uid_check` | P | F-ACT-7 | `DENY` by uid (needs the production helper's check; until then the stand-in) |
| `corrected_units_start_isolate_cancel_time_out_and_clean_up` | P | 5.1 to 5.4, U§3.3 | the U stage G as a test: start, ST-05, ST-06, ST-13, EOF cancel with exit 0, `RuntimeMaxSec` kill of a `setsid` grandchild, `RemoveOnStop=yes`, no `failed` unit left |
| `noexec_run_blocks_direct_exec_under_run_while_usr_bin_units_start` | P (canary, a host with `noexec` `/run`) | U§3.3 | `203/EXEC` for a script under `/run`; the units start |
| `skills_dir_is_unreachable_without_mask_when_etc_domovoi_is_0750` | P (canary) | F-CUS-8 | `EACCES` without mask, readable with mask and bind |
| `protect_home_yes_closes_home_and_root_writes` | P | F-PRT-2, F-PRT-3 | `EACCES`; without the directive `/home` is writable |
| `agent_cannot_read_helper_key_list_directory_or_proc` | P | F-CUS-1, F-CUS-2 | all `EACCES`; `ptrace` `EPERM` |
| `docker_group_member_fails_selftest` | P | F-DOC-4 | ST-04 fails when a probe account joins `docker` (needs a Docker host) |
| `docker_group_probe_reads_helper_key_through_bind` | P (canary, Docker host) | F-DOC-5 | documents root-equivalence; a read-only bind of a fixture directory only |
| `selftest_detects_dropin_that_removes_the_mask` | P | ST-07, I-1 | `unit_modified` and the probe fails |
| `selftest_fails_on_world_connectable_socket` | P | ST-12 | 0666 node fails |
| `helper_restart_and_sigkill_keep_state_permissions` | P | F-CUS-4 | modes unchanged |
| `runner_writes_frames_to_inherited_socket_and_logs_reach_journal_under_run_mask` | P | 5.2 | a frame written to fd 0 reaches the client; stderr output reaches the journal with `/run` masked (measured once on U with a stand-in, U§3.3; to be run with the real runner) |

### 7.2 Gate tests

Gate tests protect invariants and are never weakened (`AGENTS.md`). The following belong in the gates of the crate or xtask that owns them:

- `runner_unit_has_no_inaccessible_path_under_masked_parent`
- `runner_unit_requires_protect_home_yes`
- `runner_unit_type_is_simple_when_runtime_max_sec_is_set`
- `runner_unit_declares_no_supplementary_groups`
- `socket_units_are_0660_root_owned_group_domovoi_agent`
- `runner_unit_binds_no_socket`
- `agent_unit_runs_as_domovoi_agent`
- `no_unit_or_compose_file_binds_docker_socket_read_write_into_the_agent`
- `skill_account_name_has_one_definition_and_no_numeric_ids_nearby`
- `every_selftest_check_has_a_test`
- `helper_start_never_chmods_an_existing_directory`
- `helper_repair_never_runs_at_start`
- `no_code_parses_systemd_instance_names`
- `no_unit_execs_from_run`
- `write_groups_are_refused_on_untested_platforms_without_owner_opt_in`

### 7.3 Fixtures

- A probe-layout script modelled on the shared library of M (`probes/realuser/lib.sh`) and S (`probes/host-c3/sys/lib.sh`): creates the `domovoi-probe-ci-` accounts, groups, directories, modes, sockets and a stand-in root listener (backlog large enough: the stand-in of N and S never accepted and showed `EAGAIN`, a harness artefact), and a teardown that deletes only what it recorded.
- Accounts: a clean skill account; a polluted one (extra group); an agent account with a second group standing for `systemd-journal`; a `nobody`-class account; a docker-group account on Docker hosts.
- Skill fixtures: one that lists and reads every forbidden path, one that tries every write in 5.9 item 4, one that forks a `setsid` grandchild, one that loops until killed, one that prints large output, one that needs `/usr/lib/python3`.
- A stand-in helper that performs the `SO_PEERCRED` uid check on the real socket path (as in N, S and U), until the production helper exists.
- Fake ports in `domovoi-testkit` for process identity (uid, gid, group list, `/proc/self/status` text), file metadata and `systemctl show` output, so tier U covers every start-check branch.

### 7.4 What needs real users and systemd, and how CI can run it

- Everything marked P needs root, `useradd`, systemd as PID 1 and the ability to write `/run/systemd/system`. None of tier U does.
- Hosted CI runners are Linux virtual machines with `sudo`; whether the image has systemd as PID 1 and cgroup v2 with the controllers used in F-CG-2 was not checked here and must be confirmed on the chosen image before relying on it. A self-hosted runner on a disposable virtual machine is the safe default for tier P; a container is not: the masks and `TemporaryFileSystem=` need mount privileges that an unprivileged container does not have (not measured).
- Run tier P behind an environment switch (for example `DOMOVOI_PRIVILEGED_TESTS=1`) under a separate CI job on Debian 13, Ubuntu 26.04 and Ubuntu 24.04 images (the three real-host builds measured: 257.13, 259.9, 255.4). The job fails if the teardown finds a leftover `domovoi-probe-ci-*` object.
- The privileged job also runs the canary tests; a canary failing means systemd changed behaviour and the ADR text for that row must be revisited, not that the product is broken.

### 7.5 Conventional-Linux validation matrix

What is needed is each row's run of the tier P suite plus the evidence table of section 1. Cells say what to record; none says what the result will be.

| Host | Why it matters | Record |
|---|---|---|
| Debian 13 (systemd 257.13), real kernel | measured (N) | rerun with the production binaries and installer |
| Ubuntu 26.04 (259.9), AppArmor and Yama | measured (S) | rerun with the production binaries and installer; ST-19 output |
| Ubuntu 24.04.5 (255.4), kernel 6.8, AppArmor with the userns restriction, Yama 1, `/run` `noexec` | measured (U): the corrected units and the ST checks with stand-ins; the first run below 257 | rerun with the production binaries and installer; the tolerant ST forms of 5.9.1; ST-17; `UMask=`; the R-10 harness (U had no toolchain) |
| Debian 12 (252) | N, S and U name it as the next older version; whether it is Untested or Blocked is decided by the gate (D-3, D-4) | the S10b form, `Type=oneshot`, `TemporaryFileSystem=` on an absent path, `ProtectProc=invisible` support, `Accept=yes` on AF_UNIX, the instance name format |
| Ubuntu 22.04 (249) | the oldest named version | same as Debian 12; expected to hit `unsupported` for any directive 249 lacks, which must be a clean `unsupported{reason}` |
| a SELinux-enforcing distribution (Fedora or an EL clone) | no SELinux result exists (F-LSM-6) | AVC denials against the units and the runner; whether sockets in `/run` and the masks work under the default policy |
| a host with Docker and a populated `docker` group | S saw this; ST-04, ST-16, ST-17 | the selftest results; no change to the Docker host |
| a host with rootless Docker or Podman | none measured | whether the agent container case ST-17 and the `docker` group check still make sense |
| Unraid and an OpenRC or runit host | none measured | that `selftest` reports `unsupported` and that no skill runs; nothing more is claimed |
| a host with `ptrace_scope=0` or Yama absent next to a host with 1 | separates Yama's contribution (F-CUS-3) | the cross-uid `ptrace` and `/proc` rows |
| btrfs, xfs, overlay and network state directories | `flock` and rename behaviour measured on ext4 and tmpfs only (F-FS-1, F-FS-3) | the lock and atomic-rename rows, and ST-20 latency |

## 8. Exact follow-up edits for the owner

A list only. Nothing in this section was applied. Line numbers are those of the merged tree this document was written on.

### 8.1 `docs/rework/03-architecture-decisions.md`, RW-39

| Where | Change |
|---|---|
| line 1319, FACT bullet | the citation `manifest.rs:79` becomes `manifest.rs:102` |
| line 1344, skill-runner service list | delete "`SupplementaryGroups=` empty"; add a bullet: the account `domovoi-skill` has no supplementary group, and the runner start check and `selftest` verify `getgroups()` (F-GRP-1, F-GRP-2) |
| lines 1340 to 1357, directive list | add `ProtectProc=invisible` and `ProcSubset=pid`; state that `ProtectHome=yes` is required because `ProtectSystem=strict` leaves `/home`, `/root` and `/run/user/<uid>` writable (F-PRT-2) |
| lines 1355 to 1356 | delete the `InaccessiblePaths=/var/lib/domovoi-helper` bullet (parent mask plus child fails `226/NAMESPACE`, F-MSK-4, F-MSK-11); replace by the rule "mask a parent or a child, never a child under a masked parent; `-` prefixed children skip absent paths and fail open" |
| line 1358 | `RuntimeMaxSec=` gets "with `Type=simple`; ignored with `Type=oneshot`" (F-CAN-5) |
| lines 1360 to 1363, known limitation | the per-package example with `%i` is marked not available under `Accept=yes` (F-NAR-3); mention the dispatcher-started template as the only measured form (F-NAR-1) |
| lines 1383 to 1385, cancellation | "On EOF the runner kills the process group" becomes a runner requirement: skill in its own process group, runner exits 0, `RuntimeMaxSec` as backstop (5.4) |
| the filesystem-view bullets | add that `/etc/domovoi` must be `root:domovoi-agent 0750` or document the chosen mode, and that the mask and bind are what make `skills/` reachable (F-CUS-8, decision D-1) |
| lines 1488 onward, Platforms | restate in terms of the platform matrix of 3.4 and the tier model of D-3 and D-4 (the owner decided against a hard floor); keep container and non-systemd hosts as "no skills, labelled legacy agent" |
| the unit text of RW-39 | add that no `ExecStart=` may point under `/run`, which may be mounted `noexec` (U§3.3) |
| line 1487 | verify the citation `04` §7.3: that section is "Evaluation", not the hard limit on root installs |
| helper bullet | state that the helper `Accept=` shape is an open choice (D-2) and that its directive set is measured only as a baseline (5.3) |

### 8.2 `docs/rework/03-architecture-decisions.md`, RW-12 and RW-26

- RW-12, "Process privileges": no wording change; add nothing until D-1 and D-2 are decided.
- RW-26, "Privileges": the sentence "They cannot write ... `/etc/domovoi/*`" stays; add that `packages_dir` is readable by `domovoi-skill` by file modes (0755) and by the bind, which F-CUS-9 and F-MSK-14 support.

### 8.3 `docs/rework/04-target-architecture.md`

| Where | Change |
|---|---|
| section 8.3, line 687 to 688, "Attestation key" | "Masked in the agent and skill-runner units" is wrong for the agent: the agent must reach the helper socket, and a mask in the agent unit removes its own sockets (M section 3 row (b), agent control run). The agent is kept from the key by `/var/lib/domovoi-helper` being root 0700 with a 0600 key (F-CUS-1, F-CUS-2). Rewrite as: root 0700 directory, 0600 key; masked in the skill-runner unit only |
| section 8.3 table, line 559 and following | add rows for `/etc/domovoi` itself (`root:domovoi-agent 0750`), `/etc/domovoi/skills` (`root:root 0755`), `/var/lib/domovoi-packages` (`domovoi-agent 0755`) and the helper state directory (`root 0700`) |
| section 8.4 threat table, line 705 | "no supplementary groups, no new privileges" gets "by account, verified at start"; add a row for the Docker-group member (F-DOC-5) |
| section 8.4 | add a note that the compromised-skill row assumes the corrected unit form and a clean account (F-GRP-1, F-MSK-4) |
| section 7.3 | check that the "hard limit" on root installs is stated where RW-39 line 1487 says it is |

### 8.4 `docs/rework/99-open-decisions.md`

| Where | Change |
|---|---|
| D-29 | add after "no access to the helper socket or agent-private state": "enforced by account properties (no supplementary groups) and a mount-namespace mask, not by `SupplementaryGroups=`". Add the Docker sentence: no Domovoi account is in the `docker` group and the agent gets no Docker socket mount (F-DOC-5) |
| D-31 | no wording change; its test list gains the `selftest` check ids ST-03 and ST-05 once A1 is settled |
| D-24 | add the evidence pointer: Docker-group membership and a mounted socket are root on a Docker host (F-DOC-5, F-DOC-6); no change to the decision |
| R-3 | no status change; append the measured and unmeasured lists of 2 and 4.2; name the four systemd builds (255.4, 257.13, 259.5, 259.9) |
| R-9 | no status change; append the same, the ST-13 isolation list in its corrected form (5.9.1) as the "effective filesystem view" evidence, and the S10b confound (2.4.1) |

### 8.5 `docs/adr/0030-three-identities.md`

| Where | Change |
|---|---|
| Decision 2, evidence lines | add M, N, S and U next to *Delivery §9* for mode and group, `SO_PEERCRED` and the mask layer |
| Decision 4 | the sentence "with `User=loginuser` the process kept groups ..." stays; add that real accounts reproduced it on four hosts (F-GRP-1) and that a clean real account gave a primary-gid-only list (F-GRP-2) |
| Decision 4, last paragraph | "Real ... accounts were never created in the research" is no longer true: real accounts were created on the Pi (N section 3.6) and probe accounts elsewhere (M, S). Reword to "not created by an installer; the production check is not run" |
| Decision 1 table | the agent row's "Must not reach" should say how: DAC on a 0700 root directory, not a mask (3.2) |
| Decision 7 | add the rules UNIT-2, UNIT-3, UNIT-8 and ID-2 of 5.14 as proposals; add the account rule for the Docker group |
| Unproven, "Real accounts", "No-groups verification", "Root-owned state against another uid", "Key custody across the boundary", "Other hosts" | each is partly or fully covered: update per 3.1 and 4.2; "Every directive result is from systemd 259.5 on WSL2" is no longer true |
| Open question A1 | the code sites are five, not four: the sites in `manifest.rs:102`, `tests/manifests.rs:16`, `skill_run.rs:16`, `skill_run.rs:74` **and the snapshot** `crates/domovoi-core/tests/snapshots/manifests__skill_manifest_json_snapshot.snap:53`. Changing the example changes a snapshot, which `AGENTS.md` says must be reviewed with `cargo insta review` |
| Open question A3 | add that the check was seen to separate clean from polluted accounts in real units (M section 3 row (c); N, S and U) |
| Open question A4 | add that `StandardInput=socket` gives fd 0 as the connection (F-ACT-2), so the descriptor is still adopted from a raw number; the safe route is evaluated in `05-socket-adoption-crate-evaluation.md` (std only from fd 0, confirmed by the owner, D-6) |
| Follow-up edits | add the correction to 04 section 8.3 (8.3 above) |

### 8.6 `docs/adr/0031-helper-and-skill-runner-delivery.md`

| Where | Change |
|---|---|
| header and Context | the evidence is now five runs on four hosts and four systemd builds; "one systemd version on WSL2" is no longer true |
| Decision 2 | add `Type=simple` as a requirement and the `setpgid` and exit-0 notes (F-CAN-3, 5.4); the sentence that instances "stay failed until reset" holds on 5 of 5 |
| Decision 4 table | `ProtectHome=yes` row: label now includes system scope (F-PRT-3); `RuntimeMaxSec` row: values 5.06 to 5.32 s and the `oneshot` caveat (F-CAN-5); the last row (`$HOME` writable) is resolved: it is `ProtectSystem=strict` leaving `/home`, `/root`, `/run/user/<uid>` writable (F-PRT-2); add rows for `ProtectProc` (F-MSK-15) and for `TemporaryFileSystem=` creating a host directory on an absent path (F-MSK-10); add the "`ProtectProc` hides the pid from the filesystem only" caveat |
| Decision 4 rules, "Fail closed on absent paths" | amend: a plain mask for `InaccessiblePaths=` and `BindReadOnlyPaths=` fails closed; `TemporaryFileSystem=` never fails and silently creates the directory, so the installer must create it and `selftest` ST-18 must check it |
| Decision 4 rules, "Mask order matters" | the `226/NAMESPACE` for `InaccessiblePaths=<root>` plus `BindReadOnlyPaths=<root>/%i` is a user-scope result on 259.5 only; two system-scope runs saw "starts, nothing visible" (2.4.1). Do not cite it as a version difference |
| Decision 6 | add that the harness did not repair a pre-existing directory mode (F-CUS-5) and that the helper start check must assert modes; the `flock` and `fsync` rows gain the 1.2 ms and 67 ms medians (F-FS-2) |
| Decision 7 | replace the matrix by 3.4 |
| Decision 8 | add F-DOC-4 to F-DOC-7 as the evidence behind the requirement |
| Decision 9 | "Every directive result above is from systemd 259.5" is no longer true; four builds (255.4 to 259.9) agree except the confounded row, the instance name format and rows not measured on 259.5 |
| What the checker enforces | extend `UNIT-1` as in 5.14 |
| Unproven | remove the unexplained `$HOME` item; keep power loss, `RestrictAddressFamilies`, seccomp, helper directive set |
| Open questions B1, B2, B3 | update the evidence column per 5.5, 5.12 and the tier model (D-3, D-4); B3 (minimum systemd) is answered by the owner decision of synthesis 00 section 5b: no hard floor |

### 8.7 `docs/adr/0032-helper-safety-attestation.md`

Only the isolation-dependent items; the rest belongs to the R-10 synthesis.

- Decision 8 and 9: add the permission faults of F-CUS-5 and F-CUS-6: `helper_state_permissions` is a distinct condition from `state_corrupt`, and the helper start check asserts the directory, key, state and lock modes. This overlaps C2, so it is flagged for the R-10 synthesis.
- Unproven, "Key custody across a real process boundary", "root-only visibility and ownership" and the deferred `helper_attestation_key_not_readable_by_agent_or_skill`: partly covered at the OS level on four hosts with a stand-in helper (F-CUS-1, F-CUS-2); the production helper is not covered.
- C6: add that the repeat needs write access to the 0700 directory, which also exposes the key (F-CUS-7).

### 8.8 The `domovoi-skills` and `domovoi-skill` occurrences

The decided spelling is `domovoi-skill` (D-29, D-31, RW-39). Occurrences of the plural found by search in the merged tree:

| File | Line | What |
|---|---|---|
| `crates/domovoi-core/src/skill/manifest.rs` | 102 | doc-comment example `run_as = "domovoi-skills"` |
| `crates/domovoi-core/tests/manifests.rs` | 16 | test manifest |
| `crates/domovoi-core/tests/snapshots/manifests__skill_manifest_json_snapshot.snap` | 53 | snapshot of `run_as` (not in ADR 0030's list) |
| `crates/domovoi-runner/tests/runner/skill_run.rs` | 16 | test manifest |
| `crates/domovoi-runner/tests/runner/skill_run.rs` | 74 | `assert_eq!(spec.run_as.as_deref(), Some("domovoi-skills"))` |
| `docs/rework/03-architecture-decisions.md` | 1319 | RW-39 FACT bullet, quoting the example (and citing line 79) |
| `docs/rework/investigations/J-r3-r9-helper-and-skill-runner.md` | 181 | records the mismatch (keep as a record) |
| `docs/adr/0030-three-identities.md` | 18, 85 | states and discusses the mismatch (keep; update the count to five) |

Two more occurrences are in planning files outside the ADR and investigation set; they are reported to the owner separately and are not listed here.

### 8.9 Edits that follow from the fourth host

A list only; nothing here was applied outside this document and the custody counts of `03`.

| Where | Change |
|---|---|
| any text that gives the instance name as `<counter>-<n>-<pid>_<n>-<uid>` for every host | the format differs between builds (2.1.1); nothing parses it |
| `00-overview-and-implementation-plan.md` section 1 and 5b | add the fourth host (four independent hosts, five runs); 5b calls the 24.04 server "untested, the first candidate to measure": it is now measured (U), and 5b's definition of Supported (release-tested) does not hold for the hosts it lists as Supported; reconcile the labels (measured candidate) |
| `03-r10-safety-attestation-synthesis.md` section 2.3 and 4 | the custody row and the host count of U (done, counts only); line 332 repeats the key-masked-in-the-agent-unit error of 8.3 and is not edited here |

## 9. Open owner decisions

Each lists the options, what they cost, and a marked recommendation. A recommendation is not a decision.

**D-1. The mode of `/etc/domovoi` and how the skill runner reaches `skills_dir` (F-2).**

| Option | Cost | |
|---|---|---|
| (a) keep `root:domovoi-agent 0750`; the mask and bind make `skills/` reachable | the runner cannot work without the mask, so the mask is functionally mandatory: a drop-in that removes it causes an outage, not a leak; the unit's bind path must follow a custom `skills_dir` (the installer renders it); `selftest` must catch an outage (it does: ST-13, measured on U with an unmasked runner and with a clearing drop-in) | **decided by the owner** |
| (b) `/etc/domovoi` at 0755 `root:root`; DAC alone lets the skill read `skills/` | a drifted unit no longer breaks the runner but also no longer fails closed; `agent.toml` (0644) becomes readable by the skill without the mask (the mask hides it today); `helper.toml` stays 0600 | |
| (c) move the default `skills_dir` out of `/etc/domovoi` to a root-owned 0755 path | edits the default in the config crate (the baseline cites `config/src/agent.rs:51,264`) and every existing install; more migration | |

Evidence: F-CUS-8 (M, N and U); alternatives (b) and (c) untested (N section 6, S section 6). **Decision recorded in synthesis 00 section 5a:** keep `root:domovoi-agent 0750`; the mask and bind stay mandatory; `selftest` must detect a missing mask as an outage. Options (b) and (c) stay documented as not chosen.

**D-2. `Accept=yes` or `Accept=no` (and the helper).**

| Option | Cost | |
|---|---|---|
| `Accept=yes` | the runner implements cancel and the process-group design (5.4); failed instances linger; `%i` is not a package | **decided by the owner** (00 5a); one instance per execution gives one sandbox and one time limit |
| `Accept=no` | one long-lived `domovoi-skill` process; shared sandbox; `RuntimeMaxSec` applies to the service; everything per run is in-process | |
| a transient unit per request (`systemd-run`) | rejected by the design: a privileged command-execution path behind the socket | |

Decided (00 5a): `Accept=yes` for the runner; the helper follows it unless the owner separates them.

**D-3 and D-4. Platforms at launch and the minimum systemd version. Decided by the owner (synthesis 00 sections 5a and 5b): no hard floor.** Everything is allowed; the project publishes a list of supported, untested and unaccounted-for platforms; the helper and the runner are gated by a runtime isolation check, not by a version number. This replaces the earlier recommendation of this document (Debian 13 and Ubuntu 26.04 only, floor at systemd 257, ST-08 as a version test), which is withdrawn. U is the evidence the tier model needed: a host below 257 passed every check that reads effective isolation (U§6). The tier names are the owner's (00 section 5b); the per-tier requirements below are this document's proposal, a recommendation.

| | Supported | Untested | Blocked |
|---|---|---|---|
| What qualifies | on the published list (measured with the production binaries and the installer, release-tested) **and** the runtime gate passes | a systemd host not on the list whose runtime gate passes | the gate fails (any `fail`), or no systemd, or a container without a measured design |
| Runtime gate (at install and at every start) | ST-01 to ST-07, ST-09 to ST-16 and ST-18 pass; ST-13 in the form of 5.9.1; the helper's own start checks pass | the same | any of those is `fail`, or `nd` where the check is required (ST-17 for a container install) |
| Skills | allowed | allowed, with a persistent "untested platform" notice and a recorded acknowledgement in the audit | refused (`skill_runner_status` is `unsupported{reason}` or `error{reason}`; never a fallback to the agent uid) |
| Helper read groups | allowed | allowed | allowed only if the helper's own checks (ST-10 to ST-12, ST-15, the start checks) pass; otherwise refused |
| Helper write groups (`unit.restart`, containment) | allowed once their own gates pass (the tests of synthesis 01, the containment tests of 04) | refused unless the owner opts in per host, because their safety rests on kernel and nft behaviour that is version-specific | refused |
| What is recorded | the release-test record for the build | the `selftest` output including the ST-19 facts, and the acknowledgement | the failing check ids and tokens |
| Where the measured hosts sit today | none is release-tested (no production binaries exist), so none is "Supported" in this sense | recommendation: hosts measured only with stand-ins (the "measured candidates" of 3.4: Debian 13, Ubuntu 26.04, Ubuntu 24.04.5) gate as Untested until release-tested, with the note "measured with stand-ins" | |

Write groups stay per host and per operation family. On Ubuntu 24.04.5 (kernel 6.8, nft 1.0.9) report W found an interval-set replace that is not packet-atomic (W §3.9 and §3.10); synthesis 04 owns that finding, and it is why U's verdict for this host is "write groups Untested".

Still open for the owner (recommendations, not decisions): whether measured-with-stand-ins hosts gate as Untested until release-tested (as proposed above); the wording of the untested-platform notice and of the acknowledgement; who may grant the per-host write-group opt-in; where the published list lives (a repository file or a book page); how a host that fails only a `warn` is listed. Costs of the tier model, from the evidence: three wordings of the checks failed on real hosts and had to be corrected to avoid a false Blocked verdict (5.9.1), so the gate is only as good as its checks; the gate cannot see an SELinux denial (no SELinux host was measured); a drop-in that weakens a unit is caught only when ST-07 and ST-13 run after it (I-1).

**D-5. Rootless namespaces versus AppArmor for the runner's sandbox.**

| Option | Cost | |
|---|---|---|
| no user namespace: systemd, as root, builds the sandbox from the unit | none new; works with AppArmor unchanged (F-LSM-1, F-LSM-2); not available on hosts without systemd | **recommended** for launch |
| a bubblewrap- or `unshare`-based sandbox for hosts without systemd | needs an AppArmor profile or the sysctl `apparmor_restrict_unprivileged_userns=0` on Ubuntu 24.04 and later (S section 3.3); a new design to measure | later, if non-systemd hosts are wanted |
| Landlock (I-4) | no namespace and no AppArmor rule needed; dependency or `unsafe`; unmeasured | an idea to measure |

**D-6. Adopting an inherited socket without `unsafe` (0030 A4).** The owner asked for a safe wrapper crate to be evaluated first (synthesis 00 section 5a). The evaluation is `05-socket-adoption-crate-evaluation.md`; its recommendation is below and **awaits the owner's confirmation**: it is not a decision.

| Option | Cost | |
|---|---|---|
| std only: take the connection from fd 0 (`Stdin::as_fd().try_clone_to_owned()` into a `UnixStream`, refuse a non-zero `LISTEN_FDS`, do not unset `LISTEN_*` because the runner spawns with `env_clear()`) | no dependency, no `unsafe`, no ADR exception; fits both shipped unit shapes (`Accept=yes` with `StandardInput=socket`); measured with `systemd-socket-activate --inetd`, not in a real unit; fd 3 and `Accept=no` are not supported by it | **recommended by 05, confirmed by the owner** |
| `listenfd` 1.0.2 as the fallback for fd 3 or `Accept=no`, behind the application's own strict `LISTEN_PID` check | one small dependency with about six internal `unsafe` calls (an R11 entry); the crate accepts a missing `LISTEN_PID` | fallback only |
| a wrapper crate in general (`sd-listen-fds`, `libsystemd`, `systemd`, `sd-notify`, `tokio-listener`) | rejected in 05: unsound, no safe owned fd, licence, or wrong layer | not recommended |
| an ADR exception to R7 for one module | cannot be done under `forbid` without a separate crate; costs more than a dependency | not recommended |
| bind the process's own socket | loses socket activation and the `Accept=yes` model | not recommended |

Evidence bearing on it: U§3.3 measured the unit shape the primary route relies on (fd 0 is the connection, `LISTEN_FDS` unset, frames on fd 0 and journal logging under the `/run` mask) with a Python stand-in. The adoption code itself and a real unit using it are unmeasured. The tests named in 05 section 7 are added to 7.1.

**D-7. The user's spelling (0030 A1).** Keep `domovoi-skill` and edit five code sites (8.8): small, one commit, a snapshot to review. **Recommended** (D-31 is settled). The alternative edits three planning documents and a register item, with no code change, and contradicts D-31.

**D-8. Per-package narrowing (0031 B4).** Defer (5.2, 5.6): **recommended**. Or design the dispatcher and per-instance socket; it needs a privileged start path the helper is not allowed to offer generically.

**D-9. An AppArmor profile or SELinux module for the runner at launch.** None ships: **recommended**, because both are unmeasured (F-LSM-4, F-LSM-6) and the directive results did not need one (F-LSM-1). Shipping one is a separate measured change.

**D-10. The slow-storage reaction (ST-20).** Warn only: **recommended**; a median above 100 ms (judgement; the measured spread is 1.2 ms on NVMe to 67 ms on the USB HDD, and the third NVMe host gave 1.4 to 1.5 ms) is reported, never blocking. Alternative: block helper writes above a limit, which is a design choice with no evidence for a threshold.

**D-11. Helper start with a wrong directory, key or state mode. Decided by the owner (synthesis 00 section 5a): refuse and report, and offer self-healing as a separate, explicit, owner-confirmed action; never repair automatically at start.**

1. At start the helper refuses and fails closed with `helper_state_permissions`. The installer and the checks ST-10 and ST-11 report `path_mode_wrong` or `helper_state_permissions` the same way. Nothing changes a mode or an owner at start.
2. The refusal report names every offending path, the found mode and owner, and the expected mode and owner (5.7).
3. Self-healing is a separate command run locally as root (name to be decided). It prints the plan (each path, found and expected mode and owner, the exact change); changes nothing without the owner's explicit confirmation at the terminal; refuses a symlink or a path of the wrong type; changes only the listed paths; and runs the checks again afterwards, reporting what is left. It is not reachable from the agent, the hub, a chat command or a policy, and it is not part of start or of the installer's normal run.
4. The alternative the research flagged (`chmod` and continue at start) is rejected: it hides a possibly compromised directory, and the harness's accept-silently behaviour is what F-CUS-5 flags.

Evidence: F-CUS-5 (the harness accepts a pre-existing 0755 directory); U§3.4 ST-11 (a pre-existing 0755 helper directory is refused by the check and not repaired). The repair command was not built or run; its design above is this document's reading of the owner's decision. Tests: the gate `helper_start_never_chmods_an_existing_directory` (kept) and `helper_repair_never_runs_at_start` (new), and the unit tests of 7.1 named `helper_repair_*`.

## 10. What remains unproven, and the cheapest way to prove it

| Item | Cheapest proof |
|---|---|
| Any result about the production helper, runner, installer and units (everything here used stand-ins or probe scripts) | build them, then run tier P of section 7 on the Debian 13 and Ubuntu 26.04 hosts already used; the probe scripts exist (`probes/pi3/sys`, `probes/host-c3/sys`, `probes/host-t3/sys` with the Python stand-ins and `selftest.py`, `probes/realuser`) |
| S10b scope confound (2.4.1) | one unit of the form at system scope on the WSL host (259.5) and one at user scope on the Pi, on host-c and on host-t; about ten minutes each with the existing `s10.sh` |
| Systemd 252 and 249 behaviour (255.4 is now measured) | a virtual machine each (Debian 12, Ubuntu 22.04); run the scripts of S section 3 and U's stage G unchanged; record which directives are rejected and which tier the gate gives |
| SELinux | a Fedora or EL virtual machine; the same scripts; read the AVC log per stage as S did for AppArmor |
| Individual effect of `PrivateTmp`, `PrivateDevices`, `RestrictSUIDSGID`, `AmbientCapabilities=` | leave-one-out runs of the runner unit with the ST-13 list and one attack probe each (a setuid binary, a device node, a tmp read) |
| Seccomp filters, `MemoryDenyWriteExecute`, and `RestrictAddressFamilies=` / `ProtectKernel*` / `RestrictNamespaces` under a real workload (start and effect were measured once on U) | a per-directive leave-one-out matrix on the runner with a probe that tries the denied operation, and the fixture skills of 7.3 |
| Yama's contribution to the cross-uid `ptrace` denial on a Yama host | set `ptrace_scope=0` in a disposable virtual machine and repeat the ST-15 probes (the Pi already shows denial without Yama) |
| Helper directive set per operation group; Docker access from the helper under `ProtectSystem=strict`; netlink and `nft` as a sandboxed root unit | measure each group at system scope with real accounts (0031 B5), one unit variant per group |
| `StandardOutput=journal` with `StandardInput=socket`, frames on fd 0 and journal logging under the `/run` mask with the real runner and the fd 0 adoption code (measured once on U with a Python stand-in) | the test `runner_writes_frames_to_inherited_socket_and_logs_reach_journal_under_run_mask` |
| Power-loss durability, `ENOSPC`, the rename-to-fsync window | a virtual machine whose disk is killed (hypervisor power-off) during a write loop, then the state is validated; repeat on ext4, xfs and btrfs |
| `flock` and atomic rename on non-ext4 or network filesystems | the lock and rename rows on btrfs, xfs, overlay and NFS |
| Containers: an inherited-fd helper in a container, container escape, remapped user namespaces | not worth proving at launch: containers are Blocked for skills and write groups under the tier model of D-3 and D-4; the cheapest course is ST-17 and a Blocked report |
| Non-systemd hosts | an OpenRC or runit virtual machine only to confirm `selftest` reports `unsupported` and no skill runs |
| `DynamicUser=` on host-c and host-t (`gdb -p` is now measured on the Pi and U) | repeat N's `DynamicUser` probe |
| A different `/etc/domovoi` mode or a group-readable `skills` tree (D-1 b, c) | one run each of the F-2 probe with `/etc/domovoi` 0755 and with `skills_dir` outside `/etc/domovoi` |
| An AppArmor profile or SELinux context attached to the runner | after D-9, measure with `AppArmorProfile=`; confirm the ST-13 list still passes |
| A hostile frame to the production helper; the helper's own `SO_PEERCRED` check | unit tests with a socketpair and a fake peer uid, then tier P with the real accounts |
| Behaviour under connection floods (`MaxConnections`) | open more than the limit from the agent account and record the refusals and instance count |
| `UMask=0077` and the umask a skill sees | a probe that forks a child and reads its umask (U's umask probe measured its own setting and is ignored) |
| The R-10 harness and custody through it on 255 (U had no toolchain) | install a toolchain in a disposable user directory and run `probes/r10`; or build it elsewhere and copy the binary |
| ST-17 container install check | a compose or container fixture on a Docker host |
| The reload-pending case of ST-07 for a running loaded unit | edit a drop-in under a running instance and run ST-07 before `daemon-reload` |
| What `/run/systemd/incoming` shows under the mask on 257.13 and 259.9, and whether a newer systemd drops it | list `/run` inside the corrected runner on the Pi and host-c; about five minutes each with the U script `sys-h` |
| The write-group gate: the replace-under-load self-test on each platform | the test of synthesis 04 (`replace_never_exposes_an_absent_window`) as an install-time probe on each host; 04 owns it |

## 11. Contradictions and inconsistencies found in this revision

Each item says what disagreed, what was checked and how it was resolved here.

| Item | What disagreed | Resolution |
|---|---|---|
| Instance name format | F-ACT-2 and 5.5 gave `<counter>-<n>-<pid>_<n>-<uid>`; F-ACT-8 and N gave `<counter>-<pid>-<uid>`; U reports the second form and calls the first wrong | checked in the raw output of five probe directories: the short form on 255.4 and 257.13, the long form on 259.5 and 259.9. Not a text error: the format differs between builds, and build and host are not separated because each build ran on one host (2.1.1) |
| ST-07, ST-13 points 3 and 4 against measurement | the first wording of 5.9 fails on real hosts although the isolation holds | corrected with negative cases (5.9.1); the ST-07 query failure is on four builds (R-8 and U raw output), the append `EACCES` on three hosts, the `/run/systemd/incoming` entry on 255 only among the hosts that recorded the listing |
| Version floor against the owner's decision | D-3, D-4, ST-08, 5.12, 7.4, 7.5 and 8.1 recommended a floor at 257 | replaced by the tier model (D-3 and D-4) in every place; the floor recommendation is withdrawn |
| Synthesis 00 section 5b | defines Supported as "measured, listed, release-tested" yet lists Debian 13, Ubuntu 26.04 and WSL 2 as Supported, and none is release-tested (no production binaries); also calls Ubuntu 24.04 untested although U measured it, and U's own label "Supported-candidate" is not one of the three tiers | this document uses the three tiers and labels hosts measured with stand-ins "measured candidates" (3.4, D-3 and D-4); 00 is not edited here and needs the same reconciliation (8.9) |
| Counting | the evidence is five runs on four independent hosts (WSL2 hosts two runs) | "N of 5" counts runs and "4 hosts" counts machines, as defined in 1.2 |
| Label of one U selftest line | U's ST-13 output names an item `listdir /run/systemd (0600 root, unlistable)` | the raw `sys-h_run_diag.txt` shows `/run/systemd` as 0755 and `/run/systemd/incoming` as the 0600 root directory the skill cannot list; the wording of 5.9.1 follows the raw listing, not the item label |
| Capability bounding set in U | U §4 item 3 says an unrestricted unit has `CapBnd` `1ffffffffff`; the ST-06 negative shows `000001fff7fdffff` | consistent: the negative unit has `PrivateDevices=yes`, which drops `CAP_MKNOD` and `CAP_SYS_RAWIO`; not a defect |
| U's wording "work" | U writes that the hardening candidates "start and work" | read as: start, ST-13 rows 1 to 6 unchanged, and the expected effect on socket families and namespaces; no workload and no `UMask=` check (U§2, U§3.3) |
| Key custody wording in `03` | `03` (around line 332) says the key is "masked in the agent and skill-runner units"; 8.3 of this document shows a mask in the agent unit removes the agent's own sockets | not edited: it is a mechanism claim, not a count; flagged for the owner (8.9) |
| Encoding of U | U line 187 contains one non-UTF-8 byte (a Windows-1252 section sign in front of "3.3") | U is not changed; no text from it was copied |

## 12. Revision log

This revision incorporates report U (Ubuntu 24.04.5, systemd 255.4, kernel 6.8) and the owner's decisions of synthesis 00 sections 5a and 5b. What changed, and why:

1. **Header, section 1.** Status line extended (decisions recorded as decisions, `99` untouched, `03` named only for custody counts). U added to the sources, the probe list and the environment table. "Four runs on three hosts" became "five runs on four hosts"; "N of 5" is defined as runs; U's host condition (a live host, a second session) is stated. Why: evidence discipline needs the independence count visible.
2. **Section 2 tables (68 rows).** A U column was added to every finding table of 2.1 to 2.8 and 2.10, a Reproduced column was added to the tables of 2.4, 2.9 and 2.10 that had none, and every Reproduced and Class cell was recomputed over five runs and four hosts. F-MSK-17 changed from "not measured" to "start and effect measured once (U only)"; F-PRT-5 and F-CUS-2 gained a second source; F-FS-2 gained 1.455 and 1.484 ms; F-CUS-8 gained U. Why: U ran items that earlier hosts could not (the corrected units, `gdb`, the hardening candidates).
3. **2.1.1 (new).** The instance name format differs between builds (255 and 257 short, 259.5 and 259.9 long), checked in raw output; build and host are not separated. Why: the text disagreed with itself and with U.
4. **2.4.1, 2.9, 2.11.** S10b now has three system-scope hosts that agree (the confound stands); F-LSM-1 to F-LSM-3 and F-LSM-5 are no longer single-source; F-LSM-7 added; the universal, host-dependent and single-source lists were rewritten (added: measured once on U with the units of section 5; `/run` mask content and `noexec` as host-dependent).
5. **3.1 to 3.4.** Counts updated (four hosts, four builds, five runs); six claim rows added (the units as written hold on 255; ST-13 points 3 and 4 and ST-07 fail as written; noexec `/run`; `RemoveOnStop=`); the AppArmor, Yama and threat-model rows now name two AppArmor hosts; `RestrictAddressFamilies=` is no longer "not measured"; the platform matrix gained the 24.04 row and a proposed tier column.
6. **4.1 and 4.2.** Counts updated; conclusions 12 to 14 added (the corrected units work on 255; three check wordings fail on real hosts; `noexec` `/run`); the open list now names `UMask=`, the reload-pending case of ST-07, seccomp and a real workload, and `DynamicUser=` on S and U instead of `gdb`.
7. **5.1 to 5.8.** `RemoveOnStop=`, journal output and frames on fd 0, and the hardening candidates are marked measured once; unit comments note the `noexec` `/run` and `ExecStart=` under `/usr/bin`; 5.4 item 6 points to the adoption evaluation; 5.5 records `Accept=yes` as decided and the instance name as opaque; 5.7 records the 0750 decision; 5.8 states that the installer never repairs a mode.
8. **5.9 and 5.9.1 (new).** ST-07 (instance name, `RuntimeMaxUSec`, a `unit_not_inspectable` failure), ST-08 (no version comparison), ST-09 (no `ExecStart=` under `/run`, proposed), ST-10 and ST-11 (report found and expected mode and owner, never repaired at start), ST-13 points 3, 4 and 7 (tolerant, structure-based forms), ST-19 and ST-20 reworded; a result `nd` added; 5.9.1 shows the exact corrected wording with the observed behaviour and the negative cases, run or designed. Why: the first wording would classify a sound host as Blocked.
9. **5.10 to 5.14.** Fail-closed text aligned with D-11; `unsupported` now leads to a tier, not a version refusal; 5.12 drops the floor; UNIT-9 and ID-3 added.
10. **Section 6.** Results of U added to I-1, I-4 and I-8.
11. **Section 7.** Test names added for the tolerant checks, the repair command, fd 0 adoption (from 05), the tier gate, the corrected units as a privileged test and the noexec canary; three gate tests added; CI image list and the validation matrix gained the 24.04 row.
12. **Section 8.** Edit lists updated for four builds and the tier model; 8.9 added.
13. **Section 9.** D-1 and D-2 marked decided; D-3 and D-4 merged into the tier model with a per-tier requirement table (the floor recommendation withdrawn); D-6 rewritten per the adoption evaluation and marked as confirmed by the owner; D-10 gained the 1.4 to 1.5 ms figure; D-11 rewritten as decided (refuse, report each path, offer an explicit owner-confirmed repair, never repair at start). D-5, D-7, D-8 and D-9 are unchanged.
14. **Section 10.** The unproven list gained the R-10 harness on 255, `UMask=`, ST-17, the reload-pending case of ST-07, the `/run/systemd/incoming` question on 257 and 259.9 and the write-group gate; the `gdb` item became a `DynamicUser=` item.
15. **Sections 11 and 12 (new).**

Not changed: the structure of sections 5.1 to 5.3 beyond comments, D-5, D-7 to D-9, the host facts of earlier reports, and every statement about R-3 and R-9 status: both stay open.
