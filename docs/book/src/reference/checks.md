# Built-in checks

Every check runs on the agent on its own cadence. A run reports one observation per target. An observation is either *firing* (the condition holds) or *ok* (the target is fine, so an open incident for it can resolve). Volatile values such as counts and measurements are evidence; they are never part of a target, so they never split one problem into several incidents.

Checks read host files only through the agent's host filesystem (rooted at `/`, or `/host` in container mode). They run commands only through the fixed templates listed at the end of this page. A run that takes longer than 20 seconds is abandoned.

Thresholds are set per check in `agent.toml`:

```toml
[checks."disk.free"]
interval = "5m"
thresholds = { warning_free_pct = 10, critical_free_pct = 4 }
```

An unknown threshold key is an error that names the key. Every check is enabled unless its section sets `enabled = false`.

## Summary

| Key | Source | Default cadence | Class | Fires when (defaults) |
| --- | --- | --- | --- | --- |
| `disk.free` | `/proc/self/mounts`, `statvfs` | 5m | normal | free space below 15 % (warning) or 6 % (critical) |
| `memory.available` | `/proc/meminfo` | 1m | normal | available memory below 1.5 GiB; swap more than 50 % used |
| `temperature.cpu` | `/sys/class/hwmon`, `/sys/class/thermal` | 1m | normal | hottest CPU sensor at or above 88 °C (warning) or 95 °C (critical) |
| `system.reboot` | `/proc/sys/kernel/random/boot_id` | 1m | immediate | the boot id changed since the last run |
| `system.reboot_required` | `/var/run/reboot-required` | 1h | normal | the file exists |
| `kernel.errors` | kernel journal | 1m | normal | a known error pattern appears |
| `power.supply` | `/sys/class/power_supply` | 30s | normal, immediate on battery | running on battery; low battery; charge limit not applied |

## `disk.free`

Free space on every real filesystem. The check skips pseudo filesystems and read-only images: `autofs`, `binfmt_misc`, `bpf`, `cgroup*`, `configfs`, `debugfs`, `devpts`, `devtmpfs`, `efivarfs`, `fusectl`, `hugetlbfs`, `mqueue`, `nsfs`, `overlay`, `proc`, `pstore`, `ramfs`, `rpc_pipefs`, `securityfs`, `squashfs`, `sysfs`, `tmpfs` and `tracefs`. A mount point that appears twice is checked once.

| Setting | Default | Meaning |
| --- | --- | --- |
| `warning_free_pct` | 15 | Warning below this share of free space |
| `critical_free_pct` | 6 | Critical below this share |
| `mounts.<mount point>` | none | Per-mount `warning_free_pct` and `critical_free_pct` |
| `ignore` | `[]` | Mount points to skip |

- **Target:** the mount point, such as `/srv`.
- **Evidence:** `free_pct`, `free_gib`, `total_gib`, `fstype`, `device`.
- **Facts:** `disk.<mount point>.free_gib` (GiB).

## `memory.available`

| Setting | Default | Meaning |
| --- | --- | --- |
| `warning_available_gib` | 1.5 | Warning when `MemAvailable` is below this |
| `critical_available_gib` | none | Critical when `MemAvailable` is below this |
| `warning_swap_used_pct` | 50 | Warning when more than this share of swap is used |

- **Targets:** `available`; `swap` on hosts that have swap.
- **Evidence:** `available_gib`, `total_gib`; for `swap`, `swap_used_pct` and `swap_total_gib`.
- **Facts:** `memory.available_gib` (GiB).

## `temperature.cpu`

The hottest CPU sensor. The check reads hwmon devices whose driver is `coretemp`, `k10temp`, `zenpower`, `cpu_thermal`, `cpu-thermal`, `soc_thermal` or `x86_pkg_temp`, and uses the `temp*_label` of each sensor. Without any of those, it falls back to thermal zones: CPU zones first (`x86_pkg_temp`, `cpu-thermal`, `cpu_thermal`, `soc_thermal`, `acpitz`), otherwise every zone. Readings outside -50 to 200 °C are ignored as broken sensors. A host with no sensors reports nothing.

| Setting | Default | Meaning |
| --- | --- | --- |
| `warning_c` | 88 | Warning at or above this temperature |
| `critical_c` | 95 | Critical at or above this temperature |

- **Target:** none (the whole host).
- **Evidence:** `max_c`, `sensor` (driver and label), `sensors` (how many were read).
- **Facts:** `temperature.cpu.max_c` (°C).

## `system.reboot`

Reports a reboot once, with info severity, when the kernel's boot id differs from the one recorded on the previous run. The first run only records the boot id. The observation is immediate, so it is not held back for confirmation runs.

- **Target:** none.
- **Evidence:** `uptime_s`, `boot_id`, `previous_boot_id`.
- **Facts:** `system.uptime_days` (days), on every run.

## `system.reboot_required`

Fires with warning severity while `/var/run/reboot-required` exists, as written by Debian and Ubuntu after updates. It resolves once the file is gone.

- **Target:** none.
- **Evidence:** `packages` (the first 10 from `/var/run/reboot-required.pkgs`, comma-separated) and `package_count`.

## `kernel.errors`

Reads the kernel journal since the last run, using a journal cursor kept in the check's state, so every line is looked at once. The first run reads the last 15 minutes. Lines are matched case-insensitively, and each pattern is its own target:

| Target | Matches | Severity |
| --- | --- | --- |
| `io_error` | `I/O error` | warning |
| `oom_kill` | `Out of memory: Killed process` or `oom-kill:` | warning |
| `nvme_reset` | `nvme` followed by `reset` or `timeout` | critical |
| `pcie_uncorrected` | `PCIe Bus Error` followed by `severity=Uncorrected` | critical |
| `pcie_corrected` | `PCIe Bus Error` followed by `severity=Corrected` | info |

- **Evidence:** `count`, `examples` (at most 5 lines, already redacted); `device` for `io_error`, `process` for `oom_kill`.
- Only patterns seen in the current run are reported.
- Journal output is capped at 2 MiB. If the cap cuts off the cursor, the next run starts again from the last 15 minutes.

## `power.supply`

Laptops and hosts with a UPS battery exposed in `/sys/class/power_supply`. A host without a battery (most desktops and servers) reports nothing.

| Setting | Default | Meaning |
| --- | --- | --- |
| `low_battery_pct` | 30 | Critical when a discharging battery is below this charge |
| `charge_limit_pct` | none | The charge limit the battery should apply (`charge_control_end_threshold`) |

| Target | Fires when | Severity |
| --- | --- | --- |
| `ac` | No mains supply is online and a battery is discharging | critical, immediate |
| `<battery>` (such as `BAT0`) | Discharging below `low_battery_pct` | critical |
| `<battery>.charge_limit` | `charge_limit_pct` is set and the battery applies a different limit | warning |

- **Evidence:** `capacity_pct`, `status` and `health_pct` (full capacity over design capacity); `charge_limit_pct` and `applied_pct` for the charge limit.
- **Facts:** `power.<battery>.capacity_pct` and `power.<battery>.health_pct` (%).

## Command templates

Checks and the agent's read tools run only these commands. Each program is looked up once in a fixed list of absolute paths, with no `PATH` search. Each value is its own argument and may not start with `-`. Every command has a 15-second timeout and captures at most 2 MiB per stream.

| Template | Program | Arguments |
| --- | --- | --- |
| `journal` | `journalctl` | `--no-pager --quiet --output=short-iso --since=-<minutes>min --lines=<n>`, then `--dmesg` or `--unit <unit>` |
| `journal_since_cursor` | `journalctl` | `--no-pager --quiet --output=cat --show-cursor`, then `--after-cursor=<cursor>` or `--since=-15min`, then `--dmesg` and `--unit <unit>` per unit |
| `smartctl_scan` | `smartctl` | `--scan-open --json` |
| `smartctl_all` | `smartctl` | `--all --json --nocheck=standby <device>` |
| `systemctl_show` | `systemctl` | `show --no-pager --property=<props> -- <unit>` |
| `systemctl_failed` | `systemctl` | `list-units --state=failed --no-legend --plain --no-pager` |
| `fail2ban_status` | `fail2ban-client` | `status <jail>` |

Slot rules:
- **Unit names:** `[A-Za-z0-9@._:-]`, 1 to 128 characters.
- **Jail names:** `[A-Za-z0-9_-]`, 1 to 64 characters.
- **Devices:** `/dev/` followed by `[a-z0-9]`.
- **Unit properties:** letters only.
- **Journal cursors:** printable ASCII without spaces, at most 512 characters.
