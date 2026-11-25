# 0036 Helper read operations

- Status: proposed
- Date: 2026-11-25
- Extends [0030](0030-three-identities.md) (the three identities) and [0031](0031-helper-and-skill-runner-delivery.md) (helper delivery and isolation).
- Changes `xtask/src/arch/rules.rs`, which is why it exists.

This record is **proposed**. It decides how the unprivileged agent reads Docker, SMART, the kernel ring buffer and other users' sockets once it has lost root, and what the helper unit must allow for that. The widened unit baseline is **unmeasured** (open item OD-1 below).

## Context

After [0030](0030-three-identities.md) the agent runs as `domovoi-agent`, without the `docker` group, without root and without raw disk access. Several checks and read tools need one of those: the container checks and tools need the engine socket, the SMART check and tool need the disks, and the kernel-log fallback and the owning-process names of listening ports need privileges the agent no longer has. Only SMART and Docker fail outright on a migrated host; `security.fail2ban` needs a root-only socket and stays an error (finding F-1 below). The journal, systemd units and the `/proc` network tables need no privilege and keep their direct paths.

[0030](0030-three-identities.md) Decision 8 said the helper spawns nothing, and the first helper baseline of [0031](0031-helper-and-skill-runner-delivery.md) (`PrivateDevices=yes`, `CapabilityBoundingSet=CAP_NET_ADMIN`) cannot run `smartctl`, `dmesg` or `ss --processes`.

## Decision

1. **Typed, closed read requests.** The helper's request type has one variant per read operation: `docker_probe`, `docker_list`, `docker_events`, `docker_inspect`, `docker_logs`, `smart_scan`, `smart_report`, `kernel_log` and `net_listeners`. A request carries only validated slots (a container name or id, a `/dev/<name>` device, bounded counts and windows), never a path, a program, an argument or a Docker API path. Anything else is refused as an unknown operation. There is no write variant.

2. **Read groups.** `docker.read`, `smart.read`, `kernel.read` and `net.read` are on by default on a fresh install; an owner turns one off by removing it from `groups` in `helper.toml`, and the helper answers it as disabled. `systemd.read` stays unimplemented here.

3. **The helper spawns three fixed programs, through the injected `ProcessExecutor`.** This amends the wording of Decision 8 of [0030](0030-three-identities.md): the helper may run `smartctl`, `dmesg` and `ss`, found at fixed root-owned paths, with a fixed argument vector in which a typed slot is its own element, an empty environment, no shell and a timeout and output cap. It still contains no `std::process` or `tokio::process` use, so rules R3 and R4 do not change.

4. **Docker through a read-only client with five calls.** The helper holds the socket and the agent never does. Its client can ask the engine for the container list, one container, one image, the container events and one container's logs, all `GET`. No other call can be written: a closed enum lists them and a gate test scans the helper for any other `bollard` method. A 403 or 405 from the engine or a proxy is reported and never retried another way. The endpoint is `unix://` only; the root helper opens no network connection. A raw proxy was rejected: it would pass any verb the agent could build.

5. **The agent chooses by boundary.** With the privilege boundary in effect the container checks and tools read Docker through the helper, the SMART check and tool read through the helper, the `journal` tool falls back to the helper's kernel log when `journalctl --dmesg` cannot open the journal, and the listening-ports tool adds owning-process names. An owner-set `tcp://` endpoint (a socket proxy, as container installs use) stays direct. With the boundary not in effect nothing changes. Nothing falls back to a privileged operation in the agent.

6. **Caching and rate limits live in the agent, not the helper.** One helper process serves one connection and keeps no state (D-36). The agent client caches the container list, listening sockets, SMART scan and SMART report for short fixed times, coalesces identical concurrent requests and refuses the 21st request in 10 seconds. A compromised agent can bypass this; it is accepted because every operation is a bounded read. The helper's own limits are `MaxConnections=8`, `RuntimeMaxSec=60s`, per-call timeouts and output caps. A response frame may be 4 MiB; a request frame stays 64 KiB.

7. **The unit baseline is widened (OD-1).** `PrivateDevices=no` with `DevicePolicy=closed` and read-only `DeviceAllow=` lines for `block-sd`, `block-nvme`, `char-nvme` and `char-sg`; `CapabilityBoundingSet` is exactly `CAP_NET_ADMIN CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_SYSLOG CAP_SYS_PTRACE CAP_DAC_READ_SEARCH`; `AmbientCapabilities` stays empty. Every other directive is unchanged. One shared unit serves every request, so every instance holds the whole set, including the one that answers `health`; disabling a group in `helper.toml` does not narrow it.

8. **Rules.** `UNIT-3` asserts the widened text: `PrivateDevices=no` and `DevicePolicy=closed` both present, the four `DeviceAllow=` lines and the exact capability set. `HELPER-1` allows the agent's `helper_client/` directory instead of one file. New rule `READ-1`: `bollard` is named only in the checks' Docker source and the helper's `reads/docker/` module. The helper's `R1` allowlist gains `bollard` and `futures`.

## Open decision OD-1 (owner confirmation)

The capability and device set of Decision 7 has not been measured on a real host. The recommendation is to **accept it for building** and to treat the privileged rows of `deploy/test/identities.sh` as the measurement; if the measured minimum differs, the unit and `UNIT-3` change to it, and a capability not named here needs a new decision. `CAP_SYS_ADMIN` is the largest addition (NVMe admin commands); the privileged row records whether SATA works without it. The owner may instead drop `net.read` (removes `CAP_SYS_PTRACE` and `CAP_DAC_READ_SEARCH`; nothing in the repository fails without it) or `kernel.read` (removes `CAP_SYSLOG`); those requests then answer as not implemented.

## Consequences

- A migrated host keeps Docker, SMART, the kernel-log fallback and process names; `security.fail2ban` reports an error until a later release adds a group for it (finding F-1). Skills whose manifest names `run_as` have no runner until the skill runner.
- An already migrated host keeps its old helper unit when it differs; the installer saves the new text and prints that replacing it is needed for SMART, kernel and net reads. Docker needs no unit change.
- `inspect` removes every `Config.Env` value in the helper; command lines and labels are not removed (finding F-2, unchanged from the direct path).
- Enforced by `UNIT-3`, `HELPER-1`, `READ-1`, R1, R3 and the gate tests of the read operations; each rule has a card and a failing-input test in `xtask/src/arch/read_rules_tests.rs`.

## Alternatives considered

- Put the agent in the `docker` group: root-equivalent for the whole agent, which 0030 exists to remove.
- A raw Docker proxy in the helper: lets a compromised agent build any verb.
- A second helper unit per group: narrower capabilities, but a larger socket and unit surface; revisit if the measurement shows `CAP_SYS_ADMIN` is needed only for NVMe.
- Cache in the helper: a helper process lives for one request, so it would need a state file the agent could influence.
