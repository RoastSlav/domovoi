# 47d — Helper read operations: Docker, SMART, kernel log and listening sockets

| | |
|---|---|
| Branch | `helper-read-operations` |
| Milestone | H1 (fourth of four: 47a accounts and skeleton; 47b skill runner; 47c safety attestation; 47d read operations; see `../07-implementation-prompt-index.md` §6.4 and `../investigations/synthesis/00-overview-and-implementation-plan.md` §5) |
| Depends on (must be merged) | 47a (`helper-accounts-and-skeleton`: `domovoi_core::helper::*`, the closed `HelperRequest` enum with its `Read { group }` placeholder, `HelperGroup`/`ReadGroup`, framing, `RefusalToken`, `UnavailableReason`; `domovoi_core::isolation::{BoundaryState, PlatformTier}`; the `domovoi-helper` crate with `serve`, `start_check`, `selftest`; `domovoi_config::HelperConfig`; `domovoi_agent::helper_client`; the unit `deploy/systemd/domovoi-helper@.service` and rules `UNIT-3`, `HELPER-1`, `HELPER-2`, `ID-3`, `SELFTEST-1`; `deploy/test/identities.sh`). The 47a brief fixes the names below (section 3, "Dependence on 47a"); if the merged code differs, use the merged names and report each difference. |
| Can run in parallel with | nothing. Series order: 47a, 47b, 47c, 47d, 45, 46 (`README.md`). 47c also edits `HelperRequest`, `HelperResponse` and the dispatcher (it adds `SafetyAttest` behaviour) and merges first; this prompt rebases over it and keeps both. 45 starts after this prompt (it adds its two unit variants beside the typed read variants and extends `HelperRequest::group()`); 46 starts after 45. |
| Produces (shared contracts) | `domovoi_core::helper::read::*` (typed read requests, responses, slots `ContainerRef`, `DeviceRef`, wire DTOs, `ReadFailure`, `ReadToken`, per-operation limits); core ports `domovoi_core::ports::{DockerReadApi, PrivilegedReads}`; `domovoi_helper::reads::*` (dispatcher, Docker read client, smartctl, dmesg and ss runners, sanitiser, output parsers, device allowlist); `domovoi_agent::helper_client::{reads, cache, container_source, privileged}` (typed client methods, TTL cache and local rate limit, `HelperContainerSource`, `HelperPrivilegedReads`); testkit `FakeDockerReadApi`, `FakePrivilegedReads`; `HelperConfig` keys `[docker] endpoint` and the interpretation of `[smart] devices`; the widened baseline of `domovoi-helper@.service` and its `UNIT-3` table; rule `READ-1` and gate tests; ADR for the read operations |
| Consumes | 47a (above); `domovoi_core::ports::{ProcessExecutor, ProcessSpec, Clock}`; `domovoi_checks::containers::{ContainerSource, ContainerInfo, ContainerEvent, strip_env}`; `domovoi_checks::commands::{Commands, DevicePath}` (the existing agent-side templates stay); `bollard` (already a workspace dependency, `Cargo.toml:90`); decisions D-24, D-29, D-36, D-39, D-40, RW-39 operation groups, ADR 0018 (container runtime detection), ADRs 0030 to 0032 |
| Launch gate | 47a merged. R-3 accepted for building, exactly as 47a's "Launch gate" defines it (the decisions of ADRs 0030 and 0031 are final for building; unproven items are residual; production-binary evidence is an exit criterion). **One more owner confirmation is needed and is listed in section 2: the widened capability and device baseline of the shared helper unit (decision OD-1).** Nothing else waits on the owner. |
| Suggested model | large for the unit baseline, the Docker read path and the security review (every decision is made below, follow it exactly). The parsers (`ss`, dmesg timestamps, smartctl scan filter), the fakes, the cache and the docs may go to a mid-size subagent with a well-specified file list. |

## Hub use-case layer (read first)

This prompt changes no hub code and no hub-facing wire. Prompt 29 (`use-case-layer`) organised `crates/domovoi-hub/src/services/`; its rules (`MOD-1` to `MOD-4`, `CALLER-1`) stay in force and are not touched. If a change here seems to need an edit under `crates/domovoi-hub/` or `crates/domovoi-proto/`, stop and report: `HELPER-1` forbids helper concepts in the hub, and the helper read answers are used only by the agent's own checks and read tools. The hub keeps learning about the helper only through `agent_status.helper_status` (`Ok { ops }` already lists the enabled group names) and `hello.docker` (existing fields).

## 1. Objective

After 47a the agent runs as `domovoi-agent` without `docker`, without root and without raw disk access. Several local checks and read tools need one of those and would fail on every host that migrates. 47a implements no read operation (every `Read { group }` request is answered `unavailable`). This prompt gives the helper its **read groups** and switches the agent to them when the privilege boundary is in effect:

1. **Typed, closed read requests.** `Read { group }` is replaced by one typed variant per operation (Docker probe, list, events, inspect and logs; SMART scan and report; kernel log; listening sockets), each with validated slots and no free-form string. Anything else is refused with a typed token.
2. **The helper runs them.** `docker.read` through a **read-only Docker Engine API client** (five `GET` calls, nothing else; the helper holds the socket, the agent never does), `smart.read`, `kernel.read` and `net.read` through fixed argv templates via the injected `ProcessExecutor`, with timeouts and output caps, sanitised and bounded answers, device allowlisting, and a per-group enablement taken from `helper.toml`.
3. **The agent switches.** With `BoundaryState::InEffect` the container checks and tools read Docker through `HelperContainerSource`, the SMART check and tool and the kernel-log fallback read through `HelperPrivilegedReads`, and the listening-ports tool adds owning processes. With the boundary not in effect nothing changes: the direct paths stay exactly as they are today.
4. **The unit can do it.** The shared helper unit baseline of 47a (`PrivateDevices=yes`, `CapabilityBoundingSet=CAP_NET_ADMIN`) cannot run `smartctl`, `dmesg` or `ss -p`. This prompt widens the baseline minimally, pins it with `UNIT-3`, and measures it in the privileged script.
5. **Honest limits.** No write verb exists anywhere in the read path (gate test), results are cached and rate-limited on the agent side, and the migration text stops saying that Docker, SMART and kernel checks fail.

It does **not** add a write group, `systemd.read` (45), attestation (47c), the skill runner (47b), Docker restart (D-24), or any read that no current check or tool needs (section 5).

## 2. Architectural context

**What needs root or a group, worked out from the repository (differences with the planning text are in section 3).**

| Consumer (file:line) | What it touches | Needs after the identity split | 47d answer |
|---|---|---|---|
| `containers/docker.rs:100-257` (`DockerSource`: `list_containers`, `inspect_container`, `inspect_image`, `events`, `logs`) used by checks `container.*` and tools `docker_ps`, `docker_logs`, `docker_inspect`, `docker_events` (`requests/tools.rs:70-122`) | the Docker (or Podman) Engine socket, root-equivalent | membership of group `docker` or root; `domovoi-agent` has neither (ADR 0030) | `docker.read` through the helper |
| `commands.rs:385-397` (`smartctl_scan`, `smartctl_all`) used by `smart/mod.rs:243-256` and tool `smart_report` (`requests/tools.rs:151-155`) | raw block devices, `ioctl` | root plus `CAP_SYS_RAWIO` (NVMe admin commands may need more; unmeasured) | `smart.read` |
| `commands.rs:311-382` (`journal`, `journal_since_cursor`) used by `system/kernel.rs:212`, `security/ssh.rs:91`, `security/sudo.rs:80`, tool `journal` (`requests/tools.rs:124-140`) | `journalctl`, including `--dmesg` | **group `systemd-journal` only.** `journalctl --dmesg` reads kernel entries stored in the journal, not `/dev/kmsg`; the agent keeps that group (47a ST-02) | **no change; stays direct** |
| `units/failed.rs:89-96`, `units/watch.rs:136` (`systemctl list-units`, `systemctl show`) | systemd properties of units | nothing (any user may read these) | **no change; stays direct** |
| `security/ports.rs:147`, `net/gateway.rs:96` (`/proc/1/net/*` with a fallback to `/proc/net/*`) | network tables | nothing (world-readable; the fallback is already coded) | **no change; stays direct** |
| `system/{memory,disk,reboot,power,temperature}.rs`, `system/reboot_required.rs:59` | `/proc`, `/sys/class/*`, `statvfs` | nothing | **no change** |
| `security/fail2ban.rs:132` (`fail2ban-client status <jail>`, `commands.rs:448`) | the fail2ban control socket, root-only | root | **not covered** (no group exists in RW-39 for it; finding F-1 in section 3) |

So the privileged read set the repository needs is exactly **Docker and SMART**. `kernel.read` and `net.read` are in the RW-39 table and in `helper.toml`'s fresh-install default; this prompt implements them for the two roles described below and says plainly that no current check fails without them.

**Operation groups (RW-39 table, restricted to what this prompt builds).**

| Group | Requests built here | Why it is privileged | Capability needed (candidate, section 6.9) |
|---|---|---|---|
| `docker.read` | `DockerProbe`, `DockerList`, `DockerEvents`, `DockerInspect`, `DockerLogs` | the socket is root-equivalent (ADR 0018) | none: the socket is owned by root, and the helper is root |
| `smart.read` | `SmartScan`, `SmartReport` | raw disk access | `CAP_SYS_RAWIO` (and `CAP_SYS_ADMIN` for NVMe, unmeasured) |
| `kernel.read` | `KernelLog` | `kernel.dmesg_restrict=1` makes the ring buffer root-only | `CAP_SYSLOG` |
| `net.read` | `NetListeners` | other users' `/proc/<pid>/fd` | `CAP_SYS_PTRACE`, `CAP_DAC_READ_SEARCH` |
| `systemd.read` | none (stays `NotImplemented`) | unit identity belongs to 45 | n/a |

RW-39 also lists `docker.read` **stats** and **name to id resolution**. No check or tool in the repository uses either, so they are not built (they join the enum when a consumer exists, with their own typed variant).

**Decisions that are final (RW-39, D-24, D-29, D-36, D-39, D-40; do not reopen).**

| Decision | Consequence in 47d |
|---|---|
| D-24: no general Docker socket write | the helper's Docker client has a closed `DockerCall` enum of five `GET`s; no `POST`, `PUT`, `DELETE`, `HEAD` or `PATCH` can be constructed; the agent never holds the socket |
| D-29: privileged operations only through the narrow helper | the helper offers no command string, no unit name, no path; slots are validated newtypes |
| D-36: `Accept=yes`, one helper process per connection, no in-memory state outlives a request | **caching and rate limiting live in the agent client**, not the helper (section 6.7); the helper's only cross-request bounds are `MaxConnections=8`, `RuntimeMaxSec=60s`, per-call timeouts and output caps |
| D-39: tiers gated by a runtime check; read groups allowed on every tier except where the helper's own checks fail | section 6.8 |
| the helper spawns nothing (47a section 5, `0030` Decision 8) | **the series rule is stated once, in 47a section 5 (a later brief may let the helper spawn only the fixed programs it names, through the injected port); this brief names its three:** the helper spawns exactly three fixed programs (`smartctl`, `dmesg`, `ss`), only through the injected `ProcessExecutor`, never a shell, never from a request-supplied path. `R3` and `R4` are unchanged (`xtask/src/arch/rules.rs:167` allows spawning only in `domovoi-runner`; the helper holds the port, not a `Command`). The ADR of this prompt records the amendment |
| reads on by default on a fresh install (`04` §8.3: `docker.read`, `systemd.read`, `smart.read`, `kernel.read`, `net.read`, `safety.attest`) | unchanged; this prompt only makes four of them do something (section 6.6) |

**Open decision for the owner at the launch gate (OD-1: the widened helper unit).** 47a ships the measured baseline of synthesis 02 §5.3 (`CapabilityBoundingSet=CAP_NET_ADMIN`, `PrivateDevices=yes`) and leaves narrowing to "the first prompt that adds a group that needs it" (47a section 6.11, B5). One shared unit serves every request, so every added capability is held by every instance, including the one that answers `health`. The candidate set of section 6.9 adds `CAP_SYS_RAWIO`, `CAP_SYS_ADMIN`, `CAP_SYSLOG`, `CAP_SYS_PTRACE` and `CAP_DAC_READ_SEARCH`, turns `PrivateDevices` off and replaces it with `DevicePolicy=closed` plus read-only `DeviceAllow=` lines for disks. This is **unmeasured**. The brief's recommendation is to accept it for building, to treat the privileged rows of section 12 as the measurement, and to stop and report if the measured minimum differs. The owner may instead drop `net.read` (removes `CAP_SYS_PTRACE` and `CAP_DAC_READ_SEARCH`: nothing in the repository fails without it) or `kernel.read` (removes `CAP_SYSLOG`): the brief then ships those requests as `NotImplemented` and says so in the book. The default below keeps all four.

## 3. Repository context

Verified in the checkout this brief was written against (branch `research-synthesis`, HEAD `010ece76`). Re-open each item before editing and report differences.

**Dependence on 47a (names as defined in the 47a brief; stop and report if a merged name or shape differs materially).** From `47a-helper-accounts-and-skeleton.md` as committed: `HelperRequest` (`#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]`: `Health`, `Groups`, `Read { group: ReadGroup }`, `SafetyAttest { nonce }`, 6.2); `HelperResponse` (`Health`, `Groups`, `Unavailable { group, reason }`, `Refused { token }`, `StartRefused`); `UnavailableReason` (`GroupDisabled | NotImplemented | PlatformBlocked | WriteGroupNeedsOptIn`); `MAX_FRAME_BYTES = 65_536`, `frame_len(header)`, `decode_request`; `ServeContext { config, inspect, clock, read_deadline, build }` and `serve_connection`; `HelperConfig::effective_groups()`; `TierDecision`, `PlatformTier`, the `platform.json` record (`{format, recorded_at, kernel_release, platform, tier, blocking}`), `PlatformRecordState`; `HelperClient`, `probe_status`; `BoundaryState`; the unit text of 47a section 6.11 and the `UNIT-3` table; `FakeHostInspect`. If 47a merged with a different request spelling, use the merged one and report the difference.

**What exists today (this prompt reads and changes only the items marked).**

- `crates/domovoi-checks/src/containers/docker.rs`: `DockerSource` (bollard, `connect` at `:34-47`, `REQUEST_TIMEOUT_SECS = 10` at `:20`, `MAX_EVENTS = 1000` at `:22`). Its five Engine calls are `list_containers` with `all=true` (`:163`), `inspect_container` (`:170`, `:227`), `inspect_image` (`:72`), `events` with a `type=container` filter (`:201`) and `logs` with `stdout`, `stderr` and `tail` (`:241`). `from_inspect` (`:113`) builds `ContainerInfo`. **The trait `ContainerSource` is `list`, `events_since`, `inspect`, `logs`** (`containers/mod.rs:145-160`).
- `containers/mod.rs`: `MAX_CONTAINERS = 500` (`:36`), `MAX_LOG_BYTES = 256 * 1024` (`:163`), `strip_env` (`:167`: replaces each `Config.Env` entry with its variable name; the sanitiser ADR 0018 refers to), `ContainerInfo` (`:95`), `ContainerEvent` (`:132`), `MANAGED_LABEL`.
- `containers/detect.rs`: `candidates(runtime_dir)` (`:20`: `/var/run/docker.sock`, then `$XDG_RUNTIME_DIR/docker.sock`, `/run/podman/podman.sock`, `$XDG_RUNTIME_DIR/podman/podman.sock`), `detect` (`:55`), `engine_name` (`:9`).
- `crates/domovoi-agent/src/docker.rs`: `DockerControl` (the hub's per-host switch, ADR 0018), `connect` (`:126`), `endpoint_for` (`:132`), the `Gated` source (`:145`). `crates/domovoi-agent/src/production.rs:30-52` runs detection and builds the source; `requests/tools.rs:20` is the `docker()` accessor; `requests/mod.rs:93` holds `containers: Option<Arc<dyn ContainerSource>>`.
- `crates/domovoi-checks/src/commands.rs`: the template module. `SMARTCTL` candidates `/usr/sbin/smartctl`, `/usr/bin/smartctl`, `/sbin/smartctl` (`:34`); `spec()` (`:258`: empty environment, `TIMEOUT` 15 s, `MAX_OUTPUT_BYTES` 2 MiB, `run_as: None`); `smartctl_scan` (`:385`: `--scan-open --json`); `smartctl_all` (`:391`: `--all --json --nocheck=standby <device>`); `DevicePath::parse` (`:201`: `/dev/` then `[a-z0-9]{1,32}`); `journal`/`journal_since_cursor` (`:311`, `:346`). The slot macro rejects a leading `-` (`:110`).
- `crates/domovoi-checks/src/smart/mod.rs:243-256`: scan, `parse_scan`, then `smartctl --all` per device with `MAX_DEVICES`; a non-parsable report is skipped, not an error.
- `crates/domovoi-checks/src/framework/mod.rs:85-100`: `CheckContext { host, fs, exec, commands, clock, settings, state }`; `run_command` (`:152`). There are five construction sites of `CheckContext` (`grep -rn "CheckContext {" crates`).
- `crates/domovoi-core/src/triage/tool_call.rs:11-13` (`MAX_LINES = 500`, `MAX_SINCE_MINUTES = 1440`), `:54` (`ReadToolCall`).
- `crates/domovoi-core/src/ports/process.rs:10-76`: `ProcessSpec` (`run_as: Option<String>`), `ProcessOutput`, `ProcessExecutor`; adapters refuse shells and relative programs.
- `xtask/src/arch/rules.rs:167`: `PROCESS_ALLOWED_PREFIXES = ["crates/domovoi-runner/"]`; `bollard = "0.21.1"` and `futures = "0.3.34"` are workspace dependencies (`Cargo.toml:90`, `:41`).
- 47a's unit baseline (47a section 6.11) has `PrivateDevices=yes` and `CapabilityBoundingSet=CAP_NET_ADMIN`.

**Differences between the planning documents and the repository (state each in the completion report, section 7 step 1).**

1. **`kernel.read` is not needed by any current consumer.** RW-39 says `kernel.read` exists because of `dmesg_restrict`. The repository never reads `dmesg` or `/dev/kmsg`: the kernel check (`system/kernel.rs:212`) and the `journal` tool use `journalctl --dmesg`, which needs only `systemd-journal`. 47d builds `KernelLog` as the **fallback** for the `journal` tool with `kernel = true` (section 6.7) and leaves the check on the journal.
2. **`net.read` is not needed by any current consumer.** `security/ports.rs:147-156` and `net/gateway.rs:96-100` read world-readable tables and already fall back from `/proc/1/net/*` to `/proc/net/*`. The planned "owning process" column does not exist yet. 47d adds it to the `listening_ports` tool output **only when the helper path is active**.
3. **`units` checks need no helper.** `systemctl show` and `systemctl list-units --state=failed` work for any user. `systemd.read` is `04` §8.3's "unit identity read" (D-Bus properties and unit files, because drop-ins may be unreadable): that is 45's `unit.identity`, not these checks. `systemd.read` stays `NotImplemented` here.
4. **RW-39 lists Docker `stats` and `resolve name to id`; no code uses them.** Not built (section 5).
5. **The Docker read path is not through `ProcessExecutor`.** The planning text says "typed argv templates"; the repository reads Docker with `bollard` over a unix socket, not an argv. The helper therefore holds a Docker Engine client (`DockerReadApi`), and `ProcessExecutor` is used only for `smartctl`, `dmesg` and `ss`.
6. **Every `*.read` operation needs a response larger than 47a's frame cap.** `MAX_FRAME_BYTES = 65_536` is right for requests. A `smartctl --all --json` report, a container list of up to 500 entries or 256 KiB of logs exceed it. 47d splits the constant (section 6.2).
7. **47a's helper unit cannot run the tools.** `PrivateDevices=yes` hides block devices and `/dev/kmsg`; the bounding set lacks `CAP_SYS_RAWIO`, `CAP_SYSLOG`, `CAP_SYS_PTRACE` and `CAP_DAC_READ_SEARCH`. Section 6.9.
8. **47a states "the helper spawns nothing"** (47a section 5; ADR 0030 Decision 8 text: "every spawn goes through `domovoi-runner`"). Section 2 amends it to "only through the injected `ProcessExecutor`, three fixed programs".
9. **47a's migration text promises that Docker, SMART and journal checks fail until a later prompt** (47a section 14, `--migrate-identity` step 2). After 47d, only the journal-free statement changes: the list becomes the single `fail2ban` check (finding F-1). 47d edits that printed text in `install.sh` and the book.
10. **`ADR 0018` still says "the agent's user needs access to the socket".** Under the boundary that is the helper's job; the ADR gets a revision note (section 15).
11. **47a's gate `helper_request_has_no_write_or_free_form_variant`** fails on any `String`, `PathBuf` or `Vec<String>` field in a request type. 47d's slots are newtypes that hold a `String` privately; the gate is extended, not weakened (section 12).

**Findings outside this brief's scope (note in the PR; do not fix here).**

- **F-1.** `security.fail2ban` (`security/fail2ban.rs`, `commands.rs:448`) needs root for the fail2ban socket. RW-39 has no group for it. After the migration it reports a process error until a design decision adds a group (it would be a `fail2ban.read` group and an edit of `04` §8.3). It is the only check still failing; 47d lists it in the migration text.
- **F-2.** `inspect` output removes `Config.Env` values only (`strip_env`). `Config.Cmd`, `Config.Entrypoint` and labels can also hold secrets. 47d keeps parity (no behaviour change) and the agent's existing redactor still runs on tool output; widening the sanitiser is a separate change.

## 4. Exact scope

Files to **add** (non-test files at most 500 lines; split rather than exceed).

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/helper/read/{mod,request,response,slots,wire,limits,token}.rs` | typed read requests and responses, slots, wire DTOs, per-operation limits, failure tokens (6.2) |
| `crates/domovoi-core/src/ports/docker_read.rs` | `DockerReadApi` port (6.4) |
| `crates/domovoi-core/src/ports/privileged_reads.rs` | `PrivilegedReads` port (6.5) |
| `crates/domovoi-helper/src/reads/{mod,gate,limits}.rs` | dispatcher: group enablement, tier gate, deadline, caps (6.8) |
| `crates/domovoi-helper/src/reads/docker/{mod,client,calls,sanitize,map}.rs` | the read-only Docker client, closed `DockerCall`, sanitiser, wire mapping (6.4) |
| `crates/domovoi-helper/src/reads/tools/{mod,smart,smart_glob,kernel,net}.rs` | argv templates, tool lookup, device allowlist, `dmesg` and `ss` parsers (6.3) |
| `crates/domovoi-agent/src/helper_client/{mod,reads,cache,container_source,privileged}.rs` | `helper_client.rs` becomes a directory; typed read methods, TTL cache and rate limit, the adapters (6.7) |
| `crates/domovoi-testkit/src/{docker_read,privileged_reads}.rs` | `FakeDockerReadApi`, `FakePrivilegedReads` |
| `docs/adr/0036-helper-read-operations.md` (expected number: 47a 0033, 47b 0034, 47c 0035, then this one; take the next free number if different) | required (rules change, amendment of "the helper spawns nothing", the widened unit baseline) |
| tests: `crates/domovoi-core/src/helper/read/tests.rs`, `crates/domovoi-helper/tests/{reads_docker,reads_smart,reads_kernel,reads_net,reads_gate,reads_unit}.rs`, `crates/domovoi-agent/tests/agent/{helper_reads,helper_cache,helper_switch}.rs`, `crates/domovoi-checks/tests/` additions, `xtask/src/arch/read_rules_tests.rs` | section 12 |

Files to **change** (minimal edits): `crates/domovoi-core/src/helper/{mod,request,response,frame}.rs` (47a files: remove `Read { group }`, add the typed variants, split the frame cap), `crates/domovoi-core/src/ports/mod.rs`, `crates/domovoi-config/src/helper.rs` (`[docker] endpoint`, `[smart] devices` grammar, 6.6), `crates/domovoi-helper/{Cargo.toml,src/lib.rs,src/serve/dispatch.rs,src/state or platform read helpers as needed}` (adds the `bollard` and `futures` workspace entries and `reads`), `crates/domovoi-checks/src/framework/mod.rs` (one optional field `privileged: Option<&'a dyn PrivilegedReads>`), the five `CheckContext` construction sites, `crates/domovoi-checks/src/smart/mod.rs` (use the port when present), `crates/domovoi-agent/src/{production.rs,docker.rs,runtime.rs,requests/tools.rs,requests/mod.rs,dry_run.rs}` (selection by boundary), `deploy/systemd/domovoi-helper@.service`, `install.sh` (the embedded `helper_service` text and the migration notice), `deploy/test/installer.sh` (unit equality is unchanged: it compares embedded text with the file), `deploy/test/identities.sh` (new rows), `xtask/src/arch/{rules,identity_rules,units}.rs` (`UNIT-3` table, `HELPER-1` allowed paths, `READ-1`), `ARCHITECTURE.md`, docs (section 15).

## 5. Explicit non-goals

- **No write group and no write verb.** No `docker.write`, no `systemd.*`, no `nft`; no `POST`, `PUT`, `DELETE`, `PATCH` or `HEAD` to the Docker socket; no `container.restart` (D-24; it stays unavailable unattended). `systemd.read` and `unit.identity` are 45's.
- **No attestation** (47c) and **no skill runner** (47b); no change to `SafetyAttest` behaviour.
- **No Docker `stats`, no name-to-id resolution, no image listing, no `exec`, no `attach`, no `top`, no `diff`, no volume, network or secret reads**: nothing in the repository uses them.
- **No new check and no new read tool.** Existing tools keep their names, arguments and (on the direct path) their output. The only visible change is the owning-process suffix of `listening_ports` on the helper path.
- **No `fail2ban` access** (finding F-1) and no change to the journal, units, memory, disk, power, temperature or reboot checks.
- **No helper-side persistent cache, counter or rate limiter** (D-36; section 6.7 puts them in the agent).
- **No change to the agent-hub wire** (`crates/domovoi-proto/` untouched; wire 1.2 and its golden frames do not change), **no hub or UI change**, **no migration**.
- **No new third-party dependency.** `bollard` and `futures` are existing workspace entries; the helper crate gains them (and their `R1` allowlist rows), nothing else.
- **No `rootless` Docker or user-owned Podman sockets through the helper** (the helper is root with a restricted capability set and cannot open another user's socket; section 6.4).
- **No tier or platform change**: `SUPPORTED_PLATFORMS` stays empty; no document claims a platform is supported.
- Do not weaken any gate test or arch rule; do not touch merged migrations (there is none here).

## 6. Required interfaces and contracts

Marked **fixed** (must match, tests pin it) or **convention** (follow repository style). Names of types are fixed unless marked; field order and private helpers are the implementer's.

### 6.1 Requests (`domovoi_core::helper::read`, pure; **fixed** semantics, **convention** field order)

47a's `Read { group: ReadGroup }` placeholder is **removed** and replaced by typed variants. `ReadGroup` stays (it is the group an operation belongs to, used by the gate and by `Unavailable`).

```rust
// helper/request.rs (47a file; one variant removed, nine added). Serde unchanged: tag = "op", snake_case, deny_unknown_fields.
pub enum HelperRequest {
    Health,
    Groups,
    DockerProbe,                                            // op "docker_probe"
    DockerList,                                             // "docker_list"
    DockerEvents { since_secs: u32, until_secs: u32 },      // "docker_events": seconds before now; the window is since_secs ago to until_secs ago, since_secs > until_secs, since_secs <= 86_400
    DockerInspect { container: ContainerRef },              // "docker_inspect"
    DockerLogs { container: ContainerRef, lines: u16 },     // "docker_logs": 1..=500
    SmartScan,                                              // "smart_scan"
    SmartReport { device: DeviceRef },                      // "smart_report"
    KernelLog { since_minutes: u32, lines: u16 },           // "kernel_log": 1..=1440 and 1..=500
    NetListeners,                                           // "net_listeners"
    SafetyAttest { nonce: Nonce },                          // 47a/47c, unchanged
}
impl HelperRequest { pub fn group(&self) -> Option<HelperGroup>; }   // DockerProbe..DockerLogs -> DockerRead; SmartScan, SmartReport -> SmartRead; KernelLog -> KernelRead; NetListeners -> NetRead; SafetyAttest -> SafetyAttest; Health and Groups -> None
```

Slots are validated newtypes; a value that fails validation makes `decode_request` return `RefusalToken::FrameMalformed`, so a handler never sees a bad slot. They hold a `String` privately and serialise as a plain string.

```rust
// helper/read/slots.rs
pub struct ContainerRef(String);   // Docker container name or id: first byte [A-Za-z0-9], then [A-Za-z0-9_.-], 1..=128 bytes in all (Docker's own name grammar); never '/' , '?', '#', '%' or whitespace, so it cannot alter a URL path
pub struct DeviceRef(String);      // "/dev/" then [a-z0-9]{1,32}; the grammar of domovoi_checks::commands::DevicePath (commands.rs:201). It is only a grammar: the allowlist of 6.3 is a separate check made by the helper
impl ContainerRef { pub fn parse(&str) -> Result<Self, SlotError>; pub fn as_str(&self) -> &str; }
impl DeviceRef    { pub fn parse(&str) -> Result<Self, SlotError>; pub fn as_str(&self) -> &str; }
```

Bounds are checked in `decode_request` too (`lines` outside 1..=500, `since_minutes` outside 1..=1440, `since_secs` above 86 400 or not greater than `until_secs`: `FrameMalformed`). The request enum has no `String`, `PathBuf`, `Vec<String>` or `Vec<u8>` field; the only strings are inside the slot newtypes above (gate in section 12). **No request carries a path, a program, an argv element, a unit name, a socket address or a Docker API path.**

### 6.2 Responses, frame caps and the failure tokens (**fixed**)

```rust
// helper/response.rs (47a file; Unavailable and Refused stay; variants added)
pub enum HelperResponse {
    /* 47a: Health, Groups, Unavailable, Refused, StartRefused */
    DockerEngine     { engine: Option<DockerEngineInfo> },                 // None: no socket answered; the reason is a ReadFailure
    DockerContainers { containers: Vec<ContainerWire>, truncated: bool },
    DockerEvents     { events: Vec<EventWire>, truncated: bool },
    DockerInspect    { inspect: serde_json::Value },                       // sanitised (6.4)
    DockerLogs       { text: String, truncated: bool },
    SmartScan        { devices: Vec<DeviceRef> },                          // already filtered by the allowlist
    SmartReport      { json: String, exit_code: Option<i32>, truncated: bool },
    KernelLog        { text: String, truncated: bool },
    NetListeners     { listeners: Vec<ListenerWire>, truncated: bool },
    ReadFailed       { group: HelperGroup, token: ReadToken, detail: Option<String> },   // detail: at most 200 printable ASCII bytes, never an env value or a path outside /dev
}
```

```rust
// helper/read/wire.rs: the wire shapes mirror domovoi_checks::containers::{ContainerInfo, ContainerEvent} (containers/mod.rs:95, :132); the agent maps them back
pub struct DockerEngineInfo { pub engine: String /* "Docker" | "Podman" */, pub endpoint: String }
pub struct ContainerWire { pub name: String, pub image: String, pub image_created: Option<Timestamp>, pub state: WireRunState,
                           pub health: Option<WireHealth>, pub restart_count: u64, pub labels: BTreeMap<String, String>,
                           pub auto_remove: bool, pub started_at: Option<Timestamp> }
pub enum WireRunState { Running, Restarting, Paused, Created, Exited, Dead }       // snake_case, same set as RunState
pub enum WireHealth { Starting, Healthy, Unhealthy }
pub struct EventWire { pub at: Timestamp, pub name: String, pub action: String, pub image: Option<String> }
pub struct ListenerWire { pub proto: WireProto /* tcp | udp */, pub addr: String, pub port: u16, pub process: Option<String>, pub pid: Option<u32> }
```

**Frame caps (changes 47a's `MAX_FRAME_BYTES`).** `MAX_FRAME_BYTES` keeps its value (65 536) and is renamed `MAX_REQUEST_BYTES` (`domovoi_core::helper`; 47b's `domovoi_core::skill_runner::{MAX_FRAME_BYTES, MAX_REQUEST_BYTES}` are separate constants of the runner protocol, in another module, and are not renamed or merged); a new `MAX_RESPONSE_BYTES = 4_194_304` bounds a response frame. `frame_len(header: [u8; 4], limit: usize)` takes the limit. The server checks the request header against `MAX_REQUEST_BYTES` before reading the body (unchanged); the agent client checks the response header against `MAX_RESPONSE_BYTES` before reading the body. Every operation's output cap (6.3, 6.4) is below this limit with room for JSON escaping: if a built response would exceed `MAX_RESPONSE_BYTES`, the helper truncates the payload (`truncated: true`) and never sends a larger frame. The response of `Health` and `Groups` is unchanged. A test pins both constants.

**Per-operation limits (`helper/read/limits.rs`, **fixed**).**

| Operation | Per-call timeout | Output cap before mapping | Item cap | Notes |
|---|---|---|---|---|
| `DockerProbe` | 2 s | n/a | n/a | a connect only (section 6.4); nothing is sent |
| `DockerList` | 10 s (parity `docker.rs:20`) | n/a | 500 containers (`MAX_CONTAINERS`, `containers/mod.rs:36`) | sorted by name |
| `DockerEvents` | 10 s | n/a | 1 000 events (`MAX_EVENTS`, `docker.rs:22`) | window at most 86 400 s |
| `DockerInspect` | 10 s | 1 MiB of JSON | n/a | sanitised |
| `DockerLogs` | 10 s | 256 KiB (`MAX_LOG_BYTES`, `containers/mod.rs:163`) | 500 lines | cut on a UTF-8 boundary |
| `SmartScan` | 15 s (parity `commands.rs` `TIMEOUT`) | 2 MiB | 64 devices | |
| `SmartReport` | 15 s | 2 MiB | n/a | raw smartctl JSON text, plus its exit code |
| `KernelLog` | 15 s | 2 MiB | 500 lines | last N lines after the time filter |
| `NetListeners` | 15 s | 2 MiB | 1 000 listeners | |

Every limit is a constant; `[limits] timeout` of `helper.toml` (47a, default 30 s) bounds the whole request and wins when smaller than a per-call timeout.

**Read tokens (`ReadToken`, closed enum with `as_str` and `ALL`, spellings fixed; each has a named test).**

| Token | Meaning | When |
|---|---|---|
| `tool_missing` | `smartctl`, `dmesg` or `ss` is not installed in any fixed candidate path | tool lookup failed |
| `engine_absent` | no Docker or Podman socket exists | probe and every Docker request |
| `engine_denied` | the socket exists and the helper may not open it (`EACCES`) | an unexpected root situation; reported, never retried with other means |
| `engine_error` | the engine answered a transport error or a non-2xx other than 404, 403 and 405 | |
| `engine_api_refused` | the engine (or a proxy in front of it) answered 403 or 405 to one of the five reads | |
| `target_not_found` | the engine answered 404 for the container | `DockerInspect`, `DockerLogs` |
| `device_not_allowed` | the device does not match `[smart] devices` | `SmartReport` |
| `command_failed` | the tool exited non-zero without usable output (`dmesg`, `ss`; for smartctl see 6.3) | detail carries the first stderr line, capped |
| `output_malformed` | the tool output did not parse in the expected shape | `SmartScan` JSON, `ss` lines |
| `timed_out` | the per-call timeout or the request deadline elapsed | any |

Existing 47a refusal tokens apply unchanged (`peer_not_agent`, `frame_malformed`, `unsupported_version`, `unknown_operation`, `frame_too_large`). A request whose group is not enabled gets 47a's `Unavailable { group, reason: GroupDisabled }`; `systemd.read` and every write group keep their 47a answers.

### 6.3 Tool-backed operations (`crates/domovoi-helper/src/reads/tools/`, **fixed** argv)

The helper builds a `ProcessSpec` and runs it through the injected `ProcessExecutor`. Templates return a spec or an error; there is no shell, no `PATH` lookup, no environment (`env` empty), no `cwd`, no `stdin`, `run_as: None` (the helper is root and the tools run as root), argv is a fixed vector in which a typed slot is its own element. Tool lookup tries the candidates in order with `HostInspect::lstat`/`open_facts` (never `PATH`); the found path must be a regular file owned by root and not writable by group or other, else `tool_missing`. Pure functions of `(tool path, typed slots)` live in `reads/tools/`, so they are unit-testable without a process.

| Operation | Candidates (in order) | argv (complete) | Timeout, cap |
|---|---|---|---|
| `SmartScan` | `/usr/sbin/smartctl`, `/usr/bin/smartctl`, `/sbin/smartctl` | `["--scan-open", "--json"]` | 15 s, 2 MiB |
| `SmartReport` | same | `["--all", "--json", "--nocheck=standby", "<device>"]` where `<device>` is the validated `DeviceRef` (it starts with `/dev/`, so it cannot be an option) | 15 s, 2 MiB |
| `KernelLog` | `/usr/bin/dmesg`, `/bin/dmesg` | `["--kernel", "--time-format=iso"]` | 15 s, 2 MiB |
| `NetListeners` | `/usr/bin/ss`, `/bin/ss`, `/usr/sbin/ss` | `["--no-header", "--listening", "--numeric", "--tcp", "--udp", "--processes"]` | 15 s, 2 MiB |

The `SmartScan` and `SmartReport` argvs are byte-identical to the agent-side templates (`commands.rs:385`, `:391`); a test in `domovoi-helper` pins them with a snapshot so a change to either side is noticed.

**Behaviour after the process ends (fixed).**

- **Smartctl.** Its exit code is a bit mask: a non-zero code with valid JSON on stdout is a normal report (a failing disk is the point), exactly as the direct path treats it today (`smart/mod.rs:256-263` takes `stdout` whatever the code). `SmartReport` returns the stdout text, the exit code and the truncation flag; stdout that is empty after a non-zero exit is `command_failed`. `SmartScan` parses stdout as JSON (`devices[].name`), keeps only names whose `DeviceRef::parse` succeeds **and** that match the allowlist, caps the list at 64, and returns them sorted; unparsable stdout is `output_malformed`.
- **Device allowlist (`reads/tools/smart_glob.rs`).** `[smart] devices` (default `["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]`) is a list of patterns. The grammar is exactly: literal ASCII `[a-z0-9/]` characters and single-character classes `[a-z]` or `[0-9]`; anything else (`*`, `?`, `{`, a class with other ranges, a `..` segment) makes the file fail to load (`helper_config_invalid`). A device matches when the whole string matches one pattern. `SmartReport` for a device that does not match is `ReadFailed { token: device_not_allowed }` and **nothing is spawned**. `SmartScan` never reports a device outside the allowlist. An empty list means no device.
- **Kernel log.** The helper takes stdout, keeps lines whose leading ISO timestamp (`YYYY-MM-DDTHH:MM:SS,ffffff+hh:mm`) is at or after `now - since_minutes` (now from the injected `Clock`; a line with no parsable timestamp is kept when the previous line was kept), then keeps the last `lines` lines. Non-zero exit with empty stdout is `command_failed`.
- **Listening sockets.** The helper parses each line of `ss` output into a `ListenerWire`: `proto` from the first column (`tcp`, `udp`), the local `addr:port` column split on the last `:` (IPv6 in brackets), and, when present, `users:(("name",pid=N,fd=M))` into `process` and `pid` (only the first entry; the name is truncated to 64 bytes and every byte outside printable ASCII is replaced by `?`). A line that does not parse is skipped; no parsable line at all from non-empty output is `output_malformed`. Loopback listeners are kept (the agent filters as `listeners` does today, `security/ports.rs:154`).
- A spawn error maps to `tool_missing` (`ProcessError::NotFound`, `NotExecutable`) or `command_failed`; `timed_out` on `ProcessOutput.timed_out`. `ProcessError::ShellRefused` cannot occur and is `command_failed` if it does.

### 6.4 Docker read path (`domovoi_core::ports::DockerReadApi`; `crates/domovoi-helper/src/reads/docker/`; **fixed**)

The helper holds the socket; the agent never opens it. The helper's Docker access is **read-only by construction**:

```rust
// core/src/ports/docker_read.rs: the port the dispatcher uses; production adapter in the helper, fake in testkit
#[async_trait]
pub trait DockerReadApi: Send + Sync {
    async fn engine(&self) -> Result<DockerEngineInfo, DockerReadError>;                            // connect only, nothing sent
    async fn list(&self) -> Result<Vec<ContainerWire>, DockerReadError>;
    async fn events(&self, since: Timestamp, until: Timestamp) -> Result<Vec<EventWire>, DockerReadError>;
    async fn inspect(&self, container: &ContainerRef) -> Result<serde_json::Value, DockerReadError>; // already sanitised
    async fn logs(&self, container: &ContainerRef, lines: u16) -> Result<String, DockerReadError>;
}
pub enum DockerReadError { Absent, Denied, Refused, NotFound, Engine(String), TimedOut }       // mapped to ReadToken in 6.2
```

```rust
// helper/src/reads/docker/calls.rs: the closed list of Engine API calls. Nothing else can be expressed.
pub enum DockerCall<'a> { ListContainers, InspectContainer(&'a ContainerRef), InspectImage(&'a str /* an image id taken from an inspect response, hex or sha256:hex only */),
                          Events { since: Timestamp, until: Timestamp }, Logs { container: &'a ContainerRef, tail: u16 } }
impl DockerCall<'_> { pub const fn method(&self) -> HttpMethod /* always HttpMethod::Get */; pub fn path(&self) -> String; }
```

The production adapter `BollardDockerRead` (the helper crate's only `bollard` user) maps each call to exactly one `bollard` method and nothing else:

| `DockerCall` | Engine request | `bollard` method |
|---|---|---|
| `ListContainers` | `GET /containers/json?all=true` | `list_containers` with `all(true)` |
| `InspectContainer(c)` | `GET /containers/{c}/json` | `inspect_container(c, None)` |
| `InspectImage(i)` | `GET /images/{i}/json` | `inspect_image(i)` (used inside `list` only, to fill `image_created`, one lookup per distinct image id; failure leaves it `None`, as `docker.rs:72-81` does) |
| `Events` | `GET /events?since=&until=&filters={"type":["container"]}` | `events` with the same filter as `docker.rs:190-201` |
| `Logs` | `GET /containers/{c}/logs?stdout=1&stderr=1&tail=N` | `logs` with `stdout`, `stderr`, `tail` |

Rules, all pinned by tests:

- **Allowed methods.** `HttpMethod` has one variant, `Get`. The helper crate contains no call to any other `bollard` `Docker` method: the gate `docker_read_client_calls_only_the_five_read_methods` (section 12) scans `crates/domovoi-helper/src/` for `bollard` method calls and fails on any outside `{list_containers, inspect_container, inspect_image, events, logs, connect_with_unix}` and on the substrings `create_`, `start_`, `stop_`, `restart_`, `kill_`, `pause`, `unpause`, `remove_`, `rename_`, `update_`, `prune_`, `exec`, `attach`, `commit`, `build`, `import`, `push`, `tag_`, `upload`, `download`, `resize`, `wait`, `connect_network`, `disconnect_network`, `post`, `put`, `delete`, `head`, `patch`.
- **Refusal.** A verb the enum does not have cannot be asked: a request with `op = "docker_restart"` (or any other unknown string) is `Refused { unknown_operation }` by 47a's decoder. If the engine, or a proxy in front of it, answers 403 or 405 to one of the five reads, the helper reports `engine_api_refused` and **does not retry another way**. A test posts the six write verbs of the Engine API (`restart`, `stop`, `kill`, `create`, `exec`, `remove`) as raw `op` strings and asserts `unknown_operation` for each.
- **Endpoint.** `[docker] endpoint` in `helper.toml` (new key, 6.6) must be `unix:///absolute/path`; a `tcp://`, `http://` or relative value fails the config load (`helper_config_invalid`): the root helper opens no network connection. Without the key the helper tries, in order, `/var/run/docker.sock` then `/run/podman/podman.sock` (the system sockets of `containers/detect.rs:20`; the rootless candidates there are per-user and the helper cannot use another user's socket, so they are not tried). The first that accepts a connection is used. Detection runs at every request (the process is per request), opens and closes a connection and sends nothing, as `detect` does (`detect.rs:55`). `DockerEngineInfo.endpoint` is that path string.
- **Sanitising.** `inspect` runs `strip_env` semantics inside the helper before the response is built: every `Config.Env` entry becomes its variable name (parity with `containers/mod.rs:167`), and the helper **ports the function** into `reads/docker/sanitize.rs` (the helper cannot depend on `domovoi-checks`, R1); a test feeds both the old function and the new one the same fixture and asserts equal output. A response never contains registry credentials (the helper holds none), the socket path (except in `DockerEngineInfo.endpoint`) or any `Config.Env` value. `list` carries labels exactly as today (`from_inspect` copies them); `logs` text is returned as the engine produced it and is cut at 256 KiB on a UTF-8 boundary; the agent's existing redactor keeps running on tool output (finding F-2 records the known gap).
- **Caps.** Per 6.2: 500 containers, 1 000 events, 1 MiB inspect, 256 KiB logs, 10 s per call. `list` inspects each container once (as `docker.rs:170` does) and skips one that disappeared between listing and inspection.
- **Podman and engines that speak the Docker API** keep working through the same five calls; `engine` is `Podman` when the endpoint contains `podman` (the rule of `detect.rs:9`).
- **The agent never holds the socket under the boundary** (section 6.7): ST-16 (47a) stays a required check.

### 6.5 The agent-side port for privileged reads (`domovoi_core::ports::PrivilegedReads`, **fixed** behaviour, **convention** names)

Checks and tools that need SMART or the kernel ring buffer take this port instead of building a `ProcessSpec`:

```rust
// core/src/ports/privileged_reads.rs
#[async_trait]
pub trait PrivilegedReads: Send + Sync {
    async fn smart_scan(&self) -> Result<Vec<DeviceRef>, PrivilegedError>;
    async fn smart_report(&self, device: &DeviceRef) -> Result<SmartReportOutput, PrivilegedError>;     // { json, exit_code, truncated }
    async fn kernel_log(&self, since_minutes: u32, lines: u16) -> Result<KernelLogOutput, PrivilegedError>; // { text, truncated }
    async fn net_listeners(&self) -> Result<Vec<ListenerWire>, PrivilegedError>;
}
pub enum PrivilegedError { HelperMissing, HelperError(String), GroupDisabled(HelperGroup), PlatformBlocked, Failed(ReadToken), RateLimited }
```

`domovoi-checks`'s `CheckContext` gains one optional field, `pub privileged: Option<&'a dyn PrivilegedReads>`; `None` is today's behaviour. The five construction sites set it. `SmartHealth::run` (`smart/mod.rs:243`) becomes: when `ctx.privileged` is `Some`, scan and report through it (`DevicePath::parse` of each name still guards the device; `PrivilegedError` maps to a `CheckError::Process`-like `CheckError::Helper(String)` variant, new, carrying the token text), else the existing `run_command` path **unchanged**. Parsing (`parse_scan`, `parse_report`) is the same code on the same text, so the check's observations are identical on both paths (test `smart_check_observations_are_identical_through_the_helper_port`).

### 6.6 `helper.toml` additions (`domovoi_config::HelperConfig`, 47a `crates/domovoi-config/src/helper.rs`; **fixed** keys)

```toml
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest"]   # unchanged fresh-install default

[docker]
endpoint = "unix:///var/run/docker.sock"   # optional; unix:// only. Absent: try the two system sockets (6.4)
# write_containers = [...]                 # 04 section 8.3 key, still accepted and not interpreted (45/D-24)

[smart]
devices = ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]   # now interpreted: grammar of 6.3
```

- **Defaults.** A fresh install enables `docker.read`, `smart.read`, `kernel.read` and `net.read` (as `04` §8.3 and 47a already write the default): the reads work out of the box and no write group exists. `systemd.read` stays listed and answers `NotImplemented` until 45.
- **Per-host narrowing.** The owner turns a read group off by removing it from `groups`. A request for a removed group is `Unavailable { GroupDisabled }`. There is no per-operation switch (the group is the unit of enablement) and no new key to enable a group.
- **Validation (changes 47a's loader).** `[docker] endpoint` must match `unix:///` followed by a non-empty absolute path without `..` segments, else `helper_config_invalid`. `[smart] devices` entries must match the pattern grammar of 6.3. Unknown keys stay refused (`deny_unknown_fields`).
- The agent never reads this file (mode 0600, root).
- Tests: `helper_config_accepts_docker_endpoint_and_smart_patterns`, `helper_config_rejects_tcp_docker_endpoint`, `helper_config_rejects_smart_pattern_outside_the_grammar`, plus 47a's `helper_config_accepts_the_documented_example` updated for the interpreted keys.

### 6.7 Agent side (`crates/domovoi-agent/src/helper_client/`, **fixed** behaviour)

`helper_client.rs` (47a) becomes the directory `helper_client/` with `mod.rs` (47a's `HelperClient`, `probe_status`), `reads.rs`, `cache.rs`, `container_source.rs`, `privileged.rs`. It stays the **only** agent code that names `HelperRequest`, `HelperClient` or the socket path (`HELPER-1`; the allowed path prefix changes from the file to the directory, an `xtask` rule edit covered by this prompt's ADR).

```rust
// reads.rs: typed methods, each builds exactly one request and decodes exactly one response; no method takes a free string
impl HelperClient {
    pub async fn docker_probe(&self, clock: &dyn Clock) -> Result<Option<DockerEngineInfo>, ReadClientError>;
    pub async fn docker_list(&self, clock: &dyn Clock) -> Result<(Vec<ContainerWire>, bool), ReadClientError>;
    pub async fn docker_events(&self, clock: &dyn Clock, since: Timestamp, until: Timestamp) -> Result<(Vec<EventWire>, bool), ReadClientError>;
    pub async fn docker_inspect(&self, clock: &dyn Clock, container: &ContainerRef) -> Result<serde_json::Value, ReadClientError>;
    pub async fn docker_logs(&self, clock: &dyn Clock, container: &ContainerRef, lines: u16) -> Result<(String, bool), ReadClientError>;
    pub async fn smart_scan(&self, clock: &dyn Clock) -> Result<Vec<DeviceRef>, ReadClientError>;
    pub async fn smart_report(&self, clock: &dyn Clock, device: &DeviceRef) -> Result<SmartReportOutput, ReadClientError>;
    pub async fn kernel_log(&self, clock: &dyn Clock, since_minutes: u32, lines: u16) -> Result<KernelLogOutput, ReadClientError>;
    pub async fn net_listeners(&self, clock: &dyn Clock) -> Result<Vec<ListenerWire>, ReadClientError>;
}
pub enum ReadClientError { Missing /* socket absent */, Unreachable, Protocol, Unavailable(UnavailableReason), Refused(RefusalToken), StartRefused(RefusalToken), Failed(ReadToken, Option<String>), RateLimited }
```

**Selection (one rule, decided here).** Evaluated once at agent start from the `BoundaryState` that 47a computes, and stored in `AgentDeps`:

| Boundary | Docker | SMART | Kernel log (`journal` tool, `kernel = true`) | Listening ports tool |
|---|---|---|---|---|
| `InEffect` | `HelperContainerSource` (helper `docker.read`), **never** a direct socket, except an owner-set `tcp://` or `http://` `[docker] endpoint` in `agent.toml`, which keeps the direct path (a network socket proxy is not root-equivalent access; it is what container installs use, ADR 0018) | `HelperPrivilegedReads` | direct `journalctl --dmesg` first (unchanged); when it exits non-zero or its stderr says the journal could not be opened, fall back to `KernelLog` once | the direct `listeners` result plus owning processes from `NetListeners` when `net.read` is enabled; if the helper call fails, the direct result alone (no error) |
| `NotInEffect` (legacy root agent, container, single-box, hub-and-agent host) | **today's direct path, unchanged** | **today's direct path, unchanged** | unchanged | unchanged |

`production.rs` (`:30-52`) therefore gains a branch: with `InEffect` and a unix or absent endpoint it does not run `detect()` against the sockets (the agent cannot open them) but asks the helper (`docker_probe`) and builds `DockerControl::new(mode, Some(&detection))` from the answer: `Found { endpoint, engine }` when the helper reports an engine, `NotFound { endpoint: "", problem }` otherwise, with `problem` text from the table below. Everything after that (the hub's per-host switch, the `Gated` source, `hello.docker`, the four container checks) is unchanged, because `HelperContainerSource` is an ordinary `ContainerSource`.

| Helper answer | `problem` text shown in `hello.docker` and logs |
|---|---|
| `Missing` (`HelperStatus::Absent`) | "the helper is not installed; Docker is read through it" |
| `Unreachable`, `Protocol`, `StartRefused` | "the helper is not usable (<token or unreachable>)" |
| `Unavailable(GroupDisabled)` | "docker.read is switched off in helper.toml" |
| `Unavailable(PlatformBlocked)` | "this platform's isolation checks failed; run domovoi agent selftest" |
| `ReadFailed engine_absent` | "no Docker or Podman socket was found" |
| `ReadFailed engine_denied` or `engine_error` | "the helper cannot use the container engine socket" |

Nothing falls back to a direct privileged operation when the boundary is in effect (47a section 10: "nothing falls back to running a privileged operation in the agent"). With `InEffect` and the helper absent, container checks report the usual "the container runtime is switched off or unavailable" error text through `ContainerError`, SMART reports `PrivilegedError::HelperMissing` as a check error, and the kernel and ports tools keep working on the direct paths.

**`HelperContainerSource`** (`container_source.rs`): implements `ContainerSource` (`containers/mod.rs:145`). `list` maps `ContainerWire` to `ContainerInfo` (field for field) and truncates to `MAX_CONTAINERS`; `events_since` calls `docker_events` with the window in seconds (a window longer than 86 400 s is clamped); `inspect(name)` and `logs(name, lines)` parse the name with `ContainerRef::parse` (an invalid name is `ContainerError("invalid container name")`, never sent) and clamp `lines` to 1..=500. Every `ReadClientError` becomes a `ContainerError` with the text of the table above. **`HelperPrivilegedReads`** (`privileged.rs`) implements `PrivilegedReads` over the typed methods.

**Cache and rate limit (agent side, `cache.rs`; D-36 forbids helper-side state).** One `ReadCache` per `HelperClient`, driven only by the injected `Clock`:

| Operation | Cached for | Notes |
|---|---|---|
| `docker_probe` | 30 s | |
| `docker_list` | 5 s | the four container checks and `docker_ps` share one answer within the window |
| `net_listeners` | 5 s | |
| `smart_scan` | 300 s | |
| `smart_report` | 60 s per device | |
| `docker_inspect`, `docker_logs`, `docker_events`, `kernel_log` | not cached | evidence must be fresh |

Identical concurrent requests that miss the cache are coalesced into one call (single flight). A **local rate limit** allows at most 20 helper read requests per sliding 10 s window per client; the 21st is answered `RateLimited` without sending anything. Errors are never cached. The helper's own bounds against a misbehaving agent are the `MaxConnections=8` socket limit, `RuntimeMaxSec=60s`, the deadlines and the output caps; a compromised agent can bypass the agent-side limiter, which is accepted because every operation is a bounded read (recorded as residual in the ADR).

### 6.8 Helper dispatcher, gate order, tier and audit (`crates/domovoi-helper/src/reads/`, **fixed**)

`serve_connection` (47a section 6.7) keeps its order (peer check, frame length, start checks, decode). Step (5) dispatch gains, for a request with `group() == Some(g)` and `g` a read group:

1. `g` not in `HelperConfig::effective_groups()`: `Unavailable { g, GroupDisabled }`. `SystemdRead`: `Unavailable { g, NotImplemented }` (45). Write groups: unchanged 47a answers (no variant can reach here).
2. **Tier gate (D-39, 47a 6.9).** Read the platform record (`platform.json`) the way `Groups` does. The request is refused `Unavailable { g, PlatformBlocked }` **only when** the record exists, is for this kernel, and its `blocking` list contains any of ST-10, ST-11, ST-12 or ST-15 (the helper-read decision of 47a: reads need the helper's own isolation checks, nothing else). A `Missing` or `Stale` record does **not** block reads (it blocks write groups only). `Untested` and `Blocked`-because-of-runner-checks (ST-13, ST-05, ST-06 `nd`, which is every host before 47b) allow reads. On `Untested`, `Groups.platform_tier` already says so; the agent logs one warn line per start ("this platform has not been release-tested; privileged reads run here") and surfaces nothing else.
3. Run the operation under the request deadline (`[limits] timeout`) on the injected `Clock` (no `tokio::time`), write one `HelperResponse`, close, exit 0 (47a). A handled failure is a `ReadFailed` response, **not** a non-zero exit.
4. **Logging and audit.** The helper logs one line per read request at info level (`op`, group, outcome token or `ok`, duration, truncated flag, peer uid): never a response body, an inspect document, log text, a device serial, an environment value or a container name (the name is a user identifier, not a secret, but logs do not need it). A refusal (`unknown_operation`, `peer_not_agent`, `device_not_allowed`, `engine_api_refused`) logs at warn level with the same fields. There is no hub-side audit row for reads (reads are not mutations; 47a's helper write opt-in audit belongs to 45). Test: `helper_logs_one_line_per_read_without_the_response_body`.
5. No helper-side cache, counter or state file is written for reads (D-36); the state directory is never touched by a read.

### 6.9 The widened helper unit baseline (`deploy/systemd/domovoi-helper@.service`, **fixed text, flagged unmeasured (OD-1)**)

47a owns the unit file and ships its baseline; this prompt **amends** it with the clearly marked candidate delta below (the owner confirms OD-1 at the launch gate; nothing else in 47a's text changes). Candidate text (the privileged rows of section 12 are the measurement; section 7 step 11 says what to do when they disagree):

```ini
PrivateDevices=no
DevicePolicy=closed
DeviceAllow=block-sd r
DeviceAllow=block-nvme r
DeviceAllow=char-nvme r
DeviceAllow=char-sg r
CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_SYSLOG CAP_SYS_PTRACE CAP_DAC_READ_SEARCH
AmbientCapabilities=
```

| Directive | Why |
|---|---|
| `PrivateDevices=no` with `DevicePolicy=closed` and the four `DeviceAllow=` lines | `PrivateDevices=yes` hides block devices, so `smartctl` finds none; `closed` keeps every other device out of reach except the always-allowed pseudo devices (`/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random`, `/dev/urandom`, `/dev/tty`). Read access only |
| `CAP_SYS_RAWIO` | SMART commands through SG_IO on SATA and SCSI |
| `CAP_SYS_ADMIN` | NVMe admin commands (smartctl on `/dev/nvme*`); **the largest single addition**; the privileged row `helper_smart_report_runs_in_the_shipped_unit` measures whether SATA works without it, and the owner may restrict `smart.read` accordingly |
| `CAP_SYSLOG` | `dmesg` (via `syslog(2)`) when `kernel.dmesg_restrict=1` |
| `CAP_SYS_PTRACE`, `CAP_DAC_READ_SEARCH` | `ss --processes` reads `/proc/<pid>/fd` of other users' processes |
| `CAP_NET_ADMIN` | kept from 47a (the measured baseline value); nothing in 47d uses it |

Everything else in 47a's unit (`ProtectSystem=strict`, `ReadWritePaths=/var/lib/domovoi-helper`, `ProtectHome=yes`, `PrivateTmp=yes`, `NoNewPrivileges=yes`, `RuntimeMaxSec=60s`, `KillMode=control-group`, `TimeoutStopSec=5s`, `Type=simple`, `StandardInput=socket`) is unchanged. The Docker socket needs no capability (it is owned by root; connecting to a unix socket works on a read-only filesystem). No `ProtectKernel*`, `RestrictAddressFamilies`, `RestrictNamespaces` or `LockPersonality` is added (still unmeasured candidates). `UNIT-3` (helper) in `xtask/src/arch/identity_rules.rs` is updated to this table: the directive values above are asserted, `PrivateDevices` must be `no` **and** `DevicePolicy=closed` must be present, and the capability set must equal exactly this list (a gate fails on any extra capability). `install.sh` embeds the new text (`helper_service`) and `deploy/test/installer.sh` keeps comparing it with the file. `systemd-analyze verify` stays clean.

## 7. Ordered implementation steps

1. **Inspect and verify; report differences.** Open every file, symbol and line in section 3 and 47a's merged code. Confirm 47a is merged and list its actual names (request enum, frame functions, `ServeContext`, `HelperConfig`, `UNIT-3` table, `BoundaryState`, `helper_client`). Confirm `DockerSource`'s five calls, `strip_env`, `commands.rs` templates, the five `CheckContext` construction sites and that `bollard` and `futures` are workspace entries. Write every difference down before editing; stop if one changes the design (for example if 47a's helper spawns through another path than the injected `ProcessExecutor`, or if `HELPER-1` was merged with different allowed paths).
2. **ADR and rule data.** Write the ADR (amendment of "the helper spawns nothing", the five-call Docker client, the widened unit baseline and OD-1, the agent-side cache decision, the `HELPER-1` path change, rule `READ-1`; status `proposed`, extends 0030 and 0031). Add the `bollard` and `futures` rows to the helper crate's `R1` allowlist in `rules.rs`. `cargo xtask arch-check` and `--base origin/overhaul-investigation` pass.
3. **Core:** `helper/read/*` (slots, requests, responses, wire DTOs, limits, tokens with `ALL`), the request and response enum edits, the frame-cap split, `DockerReadApi` and `PrivilegedReads` ports. Unit tests including the decode bounds and the closed-set gate.
4. **Testkit:** `FakeDockerReadApi` (scripted lists, events, inspect with env values, logs, errors; every call recorded) and `FakePrivilegedReads`.
5. **Config:** the `[docker] endpoint` key, the `[smart]` pattern grammar and their tests.
6. **Helper, tools half:** `reads/tools/*` (tool lookup over `HostInspect`, the four templates, `smart_glob`, the dmesg time filter, the `ss` parser) with tests over `FakeProcessExecutor` and `FakeHostInspect`.
7. **Helper, Docker half:** `reads/docker/*` (`DockerCall`, `BollardDockerRead`, sanitiser, mapping to the wire DTOs). Test the mapping and sanitiser with fixtures; test the adapter against a tiny in-test unix-socket HTTP responder that records the method and path of each request and fails on anything but the five `GET` paths.
8. **Helper dispatcher:** `reads/{mod,gate,limits}.rs` wired into `serve/dispatch.rs`: group enablement, tier gate, deadline, caps, logging. Tests over `UnixStream::pair()` with the fakes.
9. **Agent:** the `helper_client/` directory (typed methods, cache, rate limit), `HelperContainerSource`, `HelperPrivilegedReads`, the selection rule in `production.rs`, `docker.rs`, `runtime.rs`, `requests/tools.rs` (kernel fallback, ports suffix), `CheckContext.privileged` and the `SmartHealth` branch with its five construction sites, `dry_run.rs`.
10. **Arch rules and gates:** `READ-1`, the `HELPER-1` path change, `UNIT-3` update and tests (section 12).
11. **Unit text, installer, scripts:** the unit file and its embedded copy, the migration notice, the privileged rows of `identities.sh` (section 12). **Run the privileged rows on a root-capable VM. If the measured capability or device minimum differs from 6.9** (a capability is missing and the operation fails, or a listed one is unnecessary), change the unit and the `UNIT-3` table to the measured minimum, record both in the completion report and the ADR, and if the minimum needs a capability not named in 6.9, **stop and report**: it is an owner decision, not an implementation choice.
12. **Docs** (section 15), then the scoped checks of `AGENTS.md` and the completion report (hosts and results of the privileged rows go into it).

## 8. Database and persistence

No database change: no migration, no hub table, no store code. The hub is not touched.

The helper persists nothing for reads (D-36): no cache, no counter, no file. The agent keeps its cache in memory only. The helper state directory and its modes are untouched; `platform.json` is read, never written, by the read path.

## 9. Protocol and compatibility

- **The agent-hub wire is unchanged.** `crates/domovoi-proto/` is not touched; wire 1.2 and its golden frames stay (R14). `hello.docker` and `agent_status.helper_status` keep their shapes; only their values change on a migrated host (`hello.docker.endpoint` carries the helper-reported socket path, `problem` carries the table of 6.7).
- **The helper socket protocol (47a, number 1) changes inside the unreleased version.** `Read { group }` leaves, nine operations join, responses gain variants, and the frame cap splits into request and response caps. Nothing is released against 47a's protocol (series rule: no release is cut from the integration branch between 47a and 47d; 45 and 46 add their variants under the same rule), so `HELPER_PROTOCOL` stays `1` (the same reasoning as wire 1.2: the latest release carries neither). If 47a had already shipped in a release when this runs, bump `HELPER_PROTOCOL` to 2 and make the agent client refuse a helper that answers 1 with `Error { reason: "protocol" }` (47a section 9); report the choice.
- **Version skew.** One binary: the agent and the helper always come from the same build, so a request the helper does not know cannot be sent. An agent started before the helper's units are reloaded gets `unknown_operation` for a new `op`; the client maps it to `HelperStatus::Error { reason: "protocol" }` on the next probe and the direct-path rules of 6.7 do not apply under the boundary (the checks report unavailable until the helper restarts, which `systemctl daemon-reload` plus a new connection does).
- **Legacy and container hosts are unaffected** (6.7 table, `NotInEffect` row).

## 10. Error handling and recovery

- **Helper absent, group off, platform record blocking, helper start refused:** the consumer sees the corresponding `ReadClientError` and degrades as 6.7 says; nothing is retried by another route and nothing falls back to root in the agent.
- **A tool is missing** (`smartctl`, `dmesg`, `ss`): `ReadFailed tool_missing`; the SMART check reports the existing "smartctl is not installed" style error, the kernel fallback and the ports suffix are skipped silently.
- **Docker socket absent or denied:** `DockerProbe` answers `DockerEngine { engine: None }` with the reason as a `ReadFailed`; the agent reports "no engine"; container checks stay off, as on a host without Docker today.
- **A container disappears between list and inspect:** skipped (parity).
- **Slow engine or tool:** the per-call timeout kills the operation (processes through the executor's group kill) and answers `timed_out`; the request deadline and `RuntimeMaxSec` are backstops.
- **A response would exceed the frame cap:** the payload is cut (`truncated: true`); never a larger frame.
- **A malformed `ss` or smartctl output:** `output_malformed`, logged without the output.
- **Agent restarts or cancels mid-read:** the connection drops; the helper's write fails, the instance exits non-zero, and the spawned tool is killed with the control group (`KillMode=control-group`). Nothing is left running.

Tokens are in 6.2; each has a named test (section 12) and appears in a `// covers: <token>` comment for `SELFTEST-1` where the rule applies.

## 11. Security requirements

1. **No write verb anywhere in the read path.** The Docker call enum has five `GET`s; the helper crate calls no other `bollard` method; there is no request variant for restart, stop, kill, create, exec or remove; write groups remain unreachable. Tests: `docker_read_client_calls_only_the_five_read_methods` (gate), `docker_adapter_sends_only_get_requests_to_the_five_paths`, `write_verbs_are_unknown_operations`, `helper_request_has_no_write_or_free_form_variant` (extended).
2. **No free command string.** Requests carry only validated slots; argv is a fixed vector with typed slots as their own elements; a device is checked against the allowlist before any spawn. Tests: `container_ref_rejects_path_and_option_characters`, `device_ref_rejects_anything_but_dev_alnum`, `smart_report_for_an_unlisted_device_spawns_nothing`, `argv_templates_match_the_brief` (snapshot).
3. **The agent never holds the Docker socket or a root capability under the boundary.** Selection rule 6.7; ST-15 and ST-16 stay required. Test: `boundary_in_effect_never_builds_a_direct_docker_source_for_a_unix_endpoint`, `boundary_not_in_effect_keeps_the_direct_sources`.
4. **Spawning only through the port.** The helper builds specs and calls `ProcessExecutor::run`; it contains no `std::process` or `tokio::process` use (the existing `R3` scan) and spawns only the three fixed programs. Test: `helper_spawns_only_the_three_read_tools_through_the_executor`.
5. **No secrets in responses.** `inspect` has every `Config.Env` value removed; no response contains an env value, a registry credential or a path outside `/dev` and the engine endpoint; logs and details are bounded and ASCII-cleaned. Tests: `inspect_env_values_never_reach_the_response`, `read_failed_detail_is_bounded_ascii_and_has_no_paths`, `helper_logs_one_line_per_read_without_the_response_body`.
6. **Bounded work.** Every operation has a timeout, an output cap and an item cap; the frame cap bounds every response. Tests: `docker_list_is_capped_at_500_and_marks_truncated`, `logs_are_cut_at_256_kib_on_a_utf8_boundary`, `response_larger_than_the_frame_cap_is_truncated_not_sent`, `smart_scan_is_capped_at_64_devices`.
7. **Tier honesty.** Reads are allowed on Untested and on Blocked-for-runner-reasons platforms; refused only when the helper's own isolation checks failed on this kernel; write groups stay refused. Tests: `reads_are_allowed_on_an_untested_platform`, `reads_are_refused_when_the_record_blocks_on_a_helper_isolation_check`, `a_missing_or_stale_record_does_not_block_reads`.
8. **The widened unit is pinned.** `UNIT-3` asserts the exact capability set; a capability beyond 6.9 fails the gate.
9. **Peer and group gates unchanged.** Only `domovoi-agent` is served (47a); a disabled group is `GroupDisabled`.
10. **Hub and agent interplay.** No API, command or message can change `helper.toml`, enable a group or reach a read operation except through the agent's own checks and read tools; nothing under `crates/domovoi-hub/` or `crates/domovoi-proto/` changes.

## 12. Testing requirements

Fakes: `FakeClock`, `FakeProcessExecutor`, `FakeHostInspect` (47a), **new** `FakeDockerReadApi`, `FakePrivilegedReads`. Tests never sleep (deadlines, caches and the rate limit run on `FakeClock`); socket tests use `UnixStream::pair()` and tempdirs. Tier **U** runs in ordinary CI without root (`cargo nextest run -p domovoi-core -p domovoi-config -p domovoi-helper -p domovoi-agent -p domovoi-checks`, `cargo test -p xtask`); tier **P** needs root, `useradd` and systemd as PID 1.

**Core** (`helper/read/tests.rs`): `container_ref_rejects_path_and_option_characters`, `container_ref_accepts_docker_names_and_ids`, `device_ref_rejects_anything_but_dev_alnum`, `read_request_bounds_are_enforced_in_decode` (lines, minutes, windows), `helper_read_requests_round_trip_every_variant` (snapshot of the wire JSON, review it), `read_op_group_mapping_is_exact`, `read_tokens_have_stable_spellings` (snapshot), `request_and_response_frame_caps_are_split` (65 536 and 4 194 304), `frame_len_takes_the_limit`, `helper_request_has_no_write_or_free_form_variant` (47a's gate, extended here and still owned by 47a: it now also fails on any request variant whose name contains `restart`, `stop`, `kill`, `create`, `exec`, `remove`, `write` or `post`, and allows `String` only inside `ContainerRef`, `DeviceRef` and `Nonce`).

**Config:** `helper_config_accepts_docker_endpoint_and_smart_patterns`, `helper_config_rejects_tcp_docker_endpoint`, `helper_config_rejects_smart_pattern_outside_the_grammar`, `helper_config_keeps_read_groups_on_by_default`, 47a's `helper_config_accepts_the_documented_example` is updated, not duplicated (it stays 47a's test).

**Helper, tools** (`crates/domovoi-helper/tests/reads_{smart,kernel,net}.rs`): `argv_templates_match_the_brief` (snapshot of the four argvs; the two smartctl vectors also asserted equal to the agent-side `Commands` output), `tool_lookup_uses_fixed_candidates_and_rejects_a_writable_or_non_root_binary`, `smart_scan_filters_devices_by_the_allowlist_and_caps_at_64`, `smart_scan_malformed_json_is_output_malformed`, `smart_report_with_nonzero_exit_and_json_is_a_normal_report`, `smart_report_with_nonzero_exit_and_no_output_is_command_failed`, `smart_report_for_an_unlisted_device_spawns_nothing`, `smart_pattern_matches_whole_strings_only`, `kernel_log_filters_by_iso_timestamp_and_keeps_continuation_lines`, `kernel_log_returns_the_last_n_lines`, `net_listeners_parse_tcp_udp_ipv4_ipv6_and_users`, `net_listeners_sanitise_process_names`, `net_listeners_skip_unparsable_lines_and_fail_when_none_parse`, `tool_timeout_is_timed_out`, `helper_spawns_only_the_three_read_tools_through_the_executor`.

**Helper, Docker** (`reads_docker.rs`): `docker_probe_prefers_the_configured_endpoint_then_the_two_system_sockets`, `docker_probe_reports_absent_and_denied_as_tokens`, `docker_list_maps_every_field_and_sorts_by_name`, `docker_list_is_capped_at_500_and_marks_truncated`, `docker_list_skips_a_container_that_vanished`, `docker_events_filter_type_container_and_cap_at_1000`, `inspect_env_values_never_reach_the_response`, `strip_env_port_matches_the_checks_function_on_the_same_fixture`, `docker_inspect_not_found_is_target_not_found`, `logs_are_cut_at_256_kib_on_a_utf8_boundary`, `engine_403_and_405_are_engine_api_refused_and_not_retried`, `docker_adapter_sends_only_get_requests_to_the_five_paths` (the in-test responder records method and path), `docker_read_client_calls_only_the_five_read_methods` (gate: source scan), `write_verbs_are_unknown_operations` (posts `docker_restart`, `docker_stop`, `docker_kill`, `docker_create`, `docker_exec`, `docker_remove` as `op` strings).

**Helper, dispatch and gate** (`reads_gate.rs`): `a_disabled_group_is_group_disabled_for_every_operation_of_the_group`, `systemd_read_stays_not_implemented`, `reads_are_allowed_on_an_untested_platform`, `reads_are_refused_when_the_record_blocks_on_a_helper_isolation_check`, `a_missing_or_stale_record_does_not_block_reads`, `request_deadline_on_the_fake_clock_ends_a_slow_read`, `response_larger_than_the_frame_cap_is_truncated_not_sent`, `read_failed_detail_is_bounded_ascii_and_has_no_paths`, `helper_logs_one_line_per_read_without_the_response_body`, `a_read_never_writes_to_the_state_directory`, plus 47a's peer, frame and start-check tests still pass unchanged.

**Agent** (`tests/agent/helper_{reads,cache,switch}.rs`): `typed_client_methods_build_exactly_one_request_each`, `client_refuses_a_response_header_above_the_response_cap`, `container_source_maps_wire_to_container_info_field_for_field`, `container_source_never_sends_an_invalid_container_name`, `docker_list_is_cached_for_5_seconds_on_the_fake_clock`, `docker_inspect_and_logs_are_never_cached`, `identical_concurrent_requests_share_one_call`, `rate_limit_refuses_the_21st_request_in_10_seconds_without_sending`, `errors_are_not_cached`, `boundary_in_effect_never_builds_a_direct_docker_source_for_a_unix_endpoint`, `boundary_in_effect_keeps_an_owner_set_tcp_endpoint_direct`, `boundary_not_in_effect_keeps_the_direct_sources`, `docker_problem_text_follows_the_helper_answer` (the table of 6.7), `hub_docker_switch_still_gates_the_helper_source`, `kernel_tool_falls_back_to_the_helper_when_journalctl_cannot_open_the_journal`, `kernel_tool_does_not_use_the_helper_when_journalctl_succeeds`, `listening_ports_tool_adds_owning_processes_only_through_the_helper`, `listening_ports_tool_keeps_the_direct_result_when_the_helper_fails`. **Checks:** `smart_check_observations_are_identical_through_the_helper_port`, `smart_check_without_a_privileged_port_runs_the_existing_commands` (existing tests unchanged and green), `smart_check_maps_helper_errors_to_a_check_error`.

**Arch and gates** (`xtask/src/arch/read_rules_tests.rs`): `unit_parser_flags_helper_unit_with_an_extra_capability` (UNIT-3), `unit_parser_flags_helper_unit_with_private_devices_yes_or_without_device_policy_closed` (UNIT-3), `helper_client_directory_is_the_only_agent_code_naming_helper_requests` (HELPER-1, new path), `bollard_is_used_only_in_checks_and_the_helper_read_module` (READ-1: `bollard` appears only in `domovoi-checks/src/containers/docker.rs` and `domovoi-helper/src/reads/docker/`), `helper_crate_spawns_nothing_directly` (no `std::process`, `tokio::process`, `Command::new` in `domovoi-helper`, existing `R3`), `shipped_units_pass_every_unit_rule` and `rules_file_change_has_an_adr` are 47a's gates: this prompt keeps them green over the new unit text and the new ADR and adds no copy. Gates are never weakened.

**Privileged real-account tests (tier P, rows added to `deploy/test/identities.sh`, same gating and teardown rules as 47a; Rust rows in `crates/domovoi-helper/tests/privileged.rs`, `#[ignore = "needs root, useradd and systemd as PID 1"]`):**

- `helper_unit_starts_under_the_widened_directives` (no `226/NAMESPACE`, `CapEff` of an instance equals the 6.9 set, no `failed` unit left);
- `helper_smart_report_runs_in_the_shipped_unit` (a request as `domovoi-agent`: on a VM without SMART disks the answer is a typed `SmartScan { devices }` (possibly empty) or `ReadFailed tool_missing`, never a unit failure; with a SMART-capable disk the report parses; records whether SATA works without `CAP_SYS_ADMIN`);
- `helper_kernel_log_reads_with_dmesg_restrict_on` (`sysctl kernel.dmesg_restrict=1`, the request returns lines; the agent account's own `dmesg` fails);
- `helper_net_listeners_show_the_owning_process_of_another_user` (a probe account runs a listener; the helper reports its process name);
- `helper_docker_read_works_through_the_real_socket_as_domovoi_agent` (needs Docker; skipped with a notice otherwise; a request for a container list succeeds while the agent account cannot open the socket, ST-16);
- `agent_with_boundary_in_effect_reads_docker_and_smart_through_the_helper` (an end-to-end `domovoi agent selftest` plus a read request over the real socket);
- `selftest_still_passes_on_a_clean_host_with_the_widened_unit` (47a's row, rerun: ST-07 compares the unit, ST-10 to ST-16 unchanged).

Run Linux-only tests in WSL as the repository rules describe, and say which ran where.

## 13. Acceptance criteria

1. Nothing under `crates/domovoi-proto/` or `crates/domovoi-hub/` changes; no golden frame changes; no migration; no new third-party dependency (`bollard` and `futures` are existing workspace entries, added to the helper crate and its `R1` allowlist only).
2. `HelperRequest` has no `Read { group }`, has exactly the typed read variants of 6.1, carries no `String`, `PathBuf` or `Vec<String>` outside the three validated newtypes, and has no write variant; the extended gate test passes.
3. Each operation of 6.3 and 6.4 behaves as specified: fixed argv (snapshot), device allowlist enforced before any spawn, five `GET` calls only, sanitised inspect, caps and timeouts, every token of 6.2 produced by a named test.
4. With the boundary in effect the container checks and tools, the SMART check and tool, the kernel-log fallback and the listening-ports suffix use the helper; with it not in effect nothing changes (existing agent and checks tests pass unmodified).
5. The agent never opens a Docker unix socket under the boundary (test), and never builds a `ProcessSpec` for `smartctl` on the helper path.
6. Group enablement follows `helper.toml`: a read group removed from `groups` answers `GroupDisabled`; fresh-install defaults enable the four read groups; `systemd.read` and every write group stay unavailable.
7. The tier gate behaves as 6.8: reads allowed on Untested and on runner-blocked hosts, refused only when the recorded blocking list contains ST-10, ST-11, ST-12 or ST-15 for this kernel, unaffected by a missing or stale record.
8. Caching, coalescing and the local rate limit behave as 6.7 on the fake clock; the helper writes no cache or counter.
9. The unit text matches 6.9 (or the measured minimum, reported), `UNIT-3` asserts it, `systemd-analyze verify` is clean, `install.sh` embeds it and `deploy/test/installer.sh` passes.
10. Request and response caps are split as 6.2; a response never exceeds `MAX_RESPONSE_BYTES`.
11. `cargo xtask arch-check` and `--base` pass with the ADR; `READ-1` and the `HELPER-1` and `UNIT-3` edits have cards and failing-input tests.
12. The privileged rows of section 12 have been run on at least one real host with systemd as PID 1 (host, distribution, systemd version and kernel recorded in the PR) and every row passes, or the failing row is reported as a failure and the measured minimum of OD-1 is recorded; **no platform is declared Supported**.
13. All tests named in section 12 exist and pass (tier U in CI; tier P as recorded); non-test files are at most 500 lines; no `unsafe`, no new `#[allow]`, no `unwrap`, `expect` or `panic!` in library code.

## 14. Migration and compatibility

- **Hosts that stay legacy** (no systemd, hub on the same host, container installs, single-box, a root agent): nothing changes; their agent keeps reading Docker, SMART and the journal directly. This is the `NotInEffect` row of 6.7.
- **A host that runs `install.sh --migrate-identity` (47a)** has its root agent replaced by `domovoi-agent`. After this prompt the plan printed before the typed confirmation says: Docker, SMART, the kernel-log fallback and owning-process names of listening ports are read through the helper's read groups and keep working; the check `security.fail2ban` (it reads a root-only socket) reports an error until a later release; skills whose manifest names a `run_as` have no runner until 47b. 47a's wording ("checks that need root (Docker through the socket, SMART, the journal of every unit) report errors until the helper's read groups carry them") is replaced; the journal statement is dropped because the journal needs only `systemd-journal`.
- **Fresh installs (47a Steps A)** write `helper.toml` with the default groups, install the widened unit (it is the shipped unit), and the agent works against it. A host whose helper has no Docker socket, no `smartctl` or no `ss` reports the matching token and keeps the rest.
- **An existing migrated host** upgrading the binary gets the widened unit text through the installer's usual rule: an existing differing unit is kept and the new text is written to `<prefix>/share/domovoi/`, and ST-07 reports a changed property until the owner replaces it. The installer prints that replacing the unit is required for SMART, kernel and net reads (Docker needs no unit change). The helper answers a missing capability as `command_failed` with a bounded detail, never silently.
- **Owner opt-out.** Removing a group from `helper.toml` disables it without touching the unit. The unit's capabilities cannot be narrowed per group (one shared unit, OD-1); the ADR records that.
- **Rollback.** `--rollback-identity` (47a) restores the legacy root agent, which does not use the helper reads; the helper units stay inert.

## 15. Documentation requirements

- `docs/adr/0036-helper-read-operations.md` (new, step 2; expected number, next free at implementation time): amends "the helper spawns nothing" to three fixed programs through the injected executor; the five-call read-only Docker client and why not a raw proxy; the unit baseline widening with the OD-1 recommendation, measurements and residual; the agent-side cache decision (D-36); the `HELPER-1` path change and `READ-1`; finding F-1. Update ADR 0018 with a revision note (under the boundary Docker is read through the helper's `docker.read`; detection results come from the helper), ADR 0030 (Decision 8 wording: helper spawns only through the port) and ADR 0031 (B5: the baseline is widened by this prompt, still unmeasured per group). Statuses stay as they are.
- `ARCHITECTURE.md`: §4 (the helper crate gains `bollard` and `futures` as external dependencies), §15 (a "Helper read operations" paragraph: closed typed requests, no write verb, read groups on by default, agent-side cache, rule ids `READ-1`, `UNIT-3` as updated), §7.2 (the agent's selection rule).
- `docs/book/src/reference/helper.md` (47a): the read operations per group, the `[docker] endpoint` and `[smart] devices` keys with the pattern grammar, the default enablement, how to turn a group off, the widened unit and why each directive is there (6.9), the token table.
- `docs/book/src/security.md`: what the helper can read, that it cannot write, that it never accepts a command string, and the residual (one shared unit, capability set).
- `docs/book/src/install/binary.md` and `upgrading.md`: what keeps working after `--migrate-identity`, that `security.fail2ban` does not (until a later release), the unit-replacement notice for an already migrated host.
- `docs/book/src/guide/hardening.md` and `docs/book/src/reference/agent.md`: Docker under the three-identities layout (the agent is not added to the `docker` group; the helper reads the socket), the owner-set `tcp://` socket-proxy endpoint that stays direct, the `hello.docker` problem texts.
- `docs/book/src/guide/troubleshooting.md`: the read tokens and what to do for each (`tool_missing`, `engine_absent`, `engine_denied`, `device_not_allowed`, `group_disabled`).
- `docs/rework/04-target-architecture.md` §8.3 (internal planning document): only the `[docker]` table gets the `endpoint` key and the `[smart]` text says the patterns are interpreted. No other edit of the planning documents.
- `deploy/README.md`, `TESTING.md`: the new privileged rows. `CHANGELOG.md` is generated; do not edit it.
- Run `cargo xtask docs-links`. Commits and the PR must not mention prompts, the build plan or coding agents.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

Task-specific: no new third-party dependency is added anywhere. `bollard` and `futures` are existing workspace
entries and are added only to `domovoi-helper`'s `[dependencies]` (and its `R1` allowlist row); the root `Cargo.toml`
changes nowhere else. 47a must be merged; if its merged names, the unit text or the `UNIT-3` table differ from what this
brief quotes, use the merged text and report each difference. Do not add a Docker call, a request variant or a capability
that this brief does not list, and do not widen the helper unit beyond section 6.9 without stopping to report. Run
Linux-only tests in WSL (`wsl.exe -d Ubuntu --`, `CARGO_TARGET_DIR=$HOME/domovoi-target`) and say which tests ran
where. Run `deploy/test/identities.sh` only on a disposable virtual machine, never on a machine you care about: it creates
accounts and changes owners and modes, and its new rows start listeners and change `sysctl` values.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

Task-specific checks: crates edited are `domovoi-core`, `domovoi-config`, `domovoi-helper`, `domovoi-agent`,
`domovoi-checks`, `domovoi-testkit` and `xtask`, so run `cargo nextest run -E 'rdeps(domovoi-core)'`,
`-E 'rdeps(domovoi-checks)'`, `-E 'rdeps(domovoi-config)'` and `-E 'rdeps(domovoi-testkit)'`, `cargo test -p xtask`,
`cargo xtask arch-check` (and `--base origin/overhaul-investigation`), `cargo xtask docs-links`,
`shellcheck install.sh deploy/test/*.sh`, `sh deploy/test/installer.sh` in a root container where Docker is available,
`systemd-analyze verify` on the shipped units, and `DOMOVOI_PRIVILEGED_TESTS=1 sh deploy/test/identities.sh` on a
root-capable VM with systemd as PID 1. Nothing under `web/`, `crates/domovoi-proto/` or `crates/domovoi-hub/` changes.
Commit as `Rostislav <raikov0411@gmail.com>`; the PR title has one scope, for example
`feat(helper): read Docker, SMART, kernel log and listening sockets through the helper`.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
