# P07 — Host checks I: framework and system checks

| | |
| --- | --- |
| Phase | 2, Agent side |
| Depends on | P06 |
| Crates | `domovoi-checks` (feature `host`) |
| Size | L |
| Invariants touched | INV-2 (commands only through `ProcessExecutor` templates) |

## Goal

Create the check framework, the fixed command templates, and the system checks: disk, memory and swap, temperatures, uptime and reboots, pending reboot, kernel errors and power. Checks read files through `HostFs` and run commands only by handing template-built `ProcessSpec`s to the `ProcessExecutor` port. Everything is tested against fixture trees.

## Read first

- `ARCHITECTURE.md` §5.2, §5.4, §7.2
- `docs/build-plan/README.md` (v0.1 replaces a cron monitor: these checks are the parity floor)
- P03 ports (`HostFs`, `ProcessExecutor`, `Clock`), P06 executor behaviour

## In scope

Add `async-trait`, `serde`, `serde_json`, `thiserror`, `tracing`, `jiff`, `nix` (`fs` for statvfs), `tokio` (`fs`). Real `HostFs` lives here: `RealHostFs { root: PathBuf }` (root `/` normally, `/host` in container mode) using `tokio::fs`.

### Command templates (`commands.rs`)

A small, closed set of fixed argv templates used by checks and by the agent's read tools. Each takes typed, validated slots and returns a `ProcessSpec` (absolute program path, argv, timeout, output cap): `journal(unit: Option<UnitName>, since_minutes, lines, kernel)`, `journal_since_cursor(cursor, units, kernel)`, `smartctl_scan()`, `smartctl_all(device: DevicePath)`, `systemctl_show(unit, props)`, `systemctl_failed()`, `fail2ban_status(jail: JailName)`. `UnitName`, `JailName` and `DevicePath` are validated newtypes (`^[A-Za-z0-9@._:-]{1,128}$`, `^[A-Za-z0-9_-]{1,64}$`, `^/dev/[a-z0-9]+$`). The program path is resolved once from a fixed candidate list (`/usr/bin/journalctl`, `/bin/journalctl`, ...) using `HostFs::exists`; no `PATH` lookup. Arguments are always separate argv elements.

### Framework (`framework/`)

- `trait Check: Send + Sync { fn key(&self) -> CheckKey; fn default_cadence(&self) -> SignedDuration; fn class(&self) -> SignalClass; async fn run(&self, ctx: &CheckContext<'_>) -> Result<CheckReport, CheckError>; }`
- `CheckContext { host: HostId, fs: &dyn HostFs, exec: &dyn ProcessExecutor, clock: &dyn Clock, settings: &CheckSettings, state: &mut CheckState }` where `CheckSettings` holds thresholds from `[checks.<key>]` (typed per check with defaults) and `CheckState` is a small persistent key/value map owned by the agent (for cursors, previous boot id, previous counters).
- `CheckReport { observations: Vec<Observation>, facts: Vec<DigestFact> }`, `Observation { target: Option<TargetKey>, firing: bool, severity, summary, evidence }`.
- `Registry` that builds all enabled checks from config.
- A check must never panic, must bound its runtime (internal timeout 20 s) and must not hold the full log in memory (stream or cap at 2 MiB).

### Checks (each in its own module, each documented with thresholds and evidence keys)

| Key | Source | Fires when (defaults) | Facts |
| --- | --- | --- | --- |
| `disk.free` | `/proc/self/mounts` + statvfs per real filesystem (skip tmpfs, overlay, squashfs, proc, sysfs, devtmpfs, cgroup*) | warning < 15 % free, critical < 6 % (per mount override) | free GB per mount |
| `memory.available` | `/proc/meminfo` | warning when MemAvailable < 1.5 GiB; swap used > 50 % warning | available GB |
| `temperature.cpu` | `/sys/class/hwmon/*/{name,temp*_input,temp*_label}`, fallback `/sys/class/thermal/thermal_zone*/temp` | warning ≥ 88 °C, critical ≥ 95 °C | max °C |
| `system.reboot` | `/proc/sys/kernel/random/boot_id` vs state | boot id changed → Immediate info observation "rebooted", evidence `uptime_s`, previous boot id | uptime |
| `system.reboot_required` | `/var/run/reboot-required`, `/var/run/reboot-required.pkgs` | file exists → warning with package list (first 10) | none |
| `kernel.errors` | `journal_since_cursor` template with `-k`, cursor in state | patterns below | none |
| `power.supply` | `/sys/class/power_supply/*` (`type`, `online`, `status`, `capacity`, `energy_full`, `energy_full_design` or `charge_*`, `charge_control_end_threshold`) | on battery (AC offline, discharging) → critical Immediate; battery < 30 % while discharging → critical; configured charge limit not applied → warning | battery %, health % |

`kernel.errors` patterns (case-insensitive, per line), each its own target so they fingerprint separately:

- `io_error`: `I/O error` → warning, evidence: device, count
- `oom_kill`: `Out of memory: Killed process` / `oom-kill:` → warning, evidence: process name
- `nvme_reset`: `nvme.*(reset|timeout)` → critical
- `pcie_uncorrected`: `PCIe Bus Error.*severity=Uncorrected` → critical
- `pcie_corrected`: `PCIe Bus Error.*severity=Corrected` → info, evidence `count` (volatile, never in target)

Counts and lines are evidence; targets are the pattern names. Keep at most 5 example lines in evidence, redacted.

## Out of scope

- Containers, units, SMART, security, ports, probes (P08). Scheduling and signal conversion (P09).

## Tests required

Fixture trees under `crates/domovoi-checks/tests/fixtures/system/<scenario>/` using `FixtureHostFs`; journal output via `FakeProcessExecutor`.

1. `disk_free_fires_warning_and_critical_per_mount_and_skips_pseudo_filesystems` (statvfs behind a small internal trait so tests can fake it).
2. `memory_fires_below_threshold_and_reports_swap`.
3. `temperature_reads_hwmon_with_labels_and_falls_back_to_thermal_zones`.
4. `reboot_is_detected_from_boot_id_change_and_not_on_first_run`.
5. `reboot_required_lists_packages`.
6. `kernel_errors_classify_patterns_and_keep_counts_out_of_targets`.
7. `kernel_errors_advance_cursor_and_do_not_repeat_lines`.
8. `power_on_battery_is_immediate_critical`, `low_battery_while_discharging_is_critical`, `charge_limit_not_applied_warns`, `desktop_without_battery_reports_nothing`.
9. `checks_never_panic_on_garbage_input`: proptest feeding random file contents to each parser.
10. `facts_are_emitted_with_units`.

## Architecture gates

- No `std::process` or `tokio::process` in `domovoi-checks` (R3); commands are `ProcessSpec`s from `commands.rs` executed through the `ProcessExecutor` port. `domovoi-checks` does not depend on `domovoi-runner` (R1).
- Files under 500 lines.

Add to tests: `templates_put_every_slot_in_its_own_argv_element_and_reject_invalid_slots`.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/reference/checks.md` lists every check key, its source, defaults and evidence keys.

## Deliverables

- `crates/domovoi-checks/src/{framework/*,hostfs,commands,system/*}.rs`
- Fixtures and tests; `docs/book/src/reference/checks.md`

## PR

Title: `feat(checks): check framework and system host checks`
