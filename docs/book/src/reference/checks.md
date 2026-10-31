# Built-in checks

Every agent check runs on its own cadence. A run reports one observation per target. An observation is either *firing* (the condition holds) or *ok* (the target is fine, so an open incident for it can resolve). Volatile values such as counts and measurements are evidence; they are never part of a target, so they never split one problem into several incidents.

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
| `container.expected_down` | Docker | 1m | normal | a learned container is missing or not running |
| `container.unhealthy` | Docker | 1m | normal | a container's health check fails |
| `container.restart_loop` | Docker | 1m | normal | 3 or more restarts within an hour |
| `container.new` | Docker | 1m | immediate | a new container outside any stack appears |
| `unit.failed` | `systemctl` | 1m | normal | a unit is in the failed state |
| `unit.watch` | `systemctl` | 1m | normal | a listed unit is not active; 3 or more restarts within an hour |
| `smart.health` | `smartctl` | 1h | normal | a SMART counter grew; NVMe critical warning, wear or heat |
| `security.ssh_login` | sshd journal | 1m | immediate | any SSH login (warning from the internet, info from the LAN) |
| `security.sudo_failure` | sudo journal | 1m | immediate | failed sudo authentication |
| `security.fail2ban` | `fail2ban-client` | 5m | normal | 20 or more bans within an hour (info) |
| `security.new_port` | `/proc/1/net/*` | 1m | immediate | a new non-loopback listener appears |

Network probes (`probe.tcp`, `probe.http`, `probe.gateway`) are described at the end of this page.

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

## Containers

The four container checks run when `[docker] enabled = true`, reading the Docker Engine API at `[docker] endpoint` (a unix socket or a `tcp://` socket proxy). Containers listed in `[containers] ignore` are skipped by all of them. Targets are container names.

A container is *managed* when it carries the `com.docker.compose.project` label, which compose and Portainer stacks set. Only managed containers without auto-remove are learned, so a throwaway `docker run --rm` never becomes expected.

### `container.expected_down`

A managed container seen running in two consecutive runs becomes expected. From then on, the check reports it every run: fine while it runs, firing while it is stopped or gone. An expected container gone for `[containers] baseline_expiry` (default 7 days) leaves the baseline.

- **Severity:** critical for names in `[containers] critical`, warning otherwise.
- **Evidence:** `state` (`missing` when the container is gone), `image`, `last_seen`.
- **Facts:**
  - `containers.expected_running`, such as "24/24 expected containers running".
  - `container.<name>.image_age_days` (days).
  - `baseline.expired` (the name of a container dropped from the baseline), for an advisory.
- Each run also keeps the last 24 hours of container events (at most 500) for the Docker events read tool and for triage.

### `container.unhealthy`

Fires with warning severity while a container's health check reports `unhealthy`. Containers without a health check report nothing.

- **Evidence:** `image`, `state`.

### `container.restart_loop`

Fires with warning severity when Docker's restart count grew by at least `restarts` within the last hour. A recreated container (the count drops) counts from zero.

| Setting | Default | Meaning |
| --- | --- | --- |
| `restarts` | 3 | Restarts within an hour that count as a loop |

- **Evidence:** `restarts` (growth in the hour), `window`, `restart_count`.

### `container.new`

Reports once, with info severity, each new container that is not managed and not auto-removed. The first run records the containers already there without reporting them.

- **Evidence:** `image`, `state`.

## `unit.failed`

One observation per failed systemd unit, with warning severity. The check reads `systemctl list-units --state=failed` as JSON where systemd supports it (version 246 and later) and as plain text otherwise. A unit that recovers resolves.

- **Target:** the unit, such as `backup.service`.
- **Evidence:** `load`, `active`, `sub`, `description`.

## `unit.watch`

Units you list must stay active.

| Setting | Default | Meaning |
| --- | --- | --- |
| `units` | `[]` | The units to watch (at most 64) |
| `critical` | `[]` | Units whose failure is critical |
| `restarts` | 3 | `NRestarts` growth within an hour that counts as a restart loop |

`criticality = "critical"` on the check makes every watched unit critical.

| Target | Fires when | Severity |
| --- | --- | --- |
| `<unit>` | `ActiveState` is not `active` | warning, or critical |
| `<unit>:restarts` | `NRestarts` grew by `restarts` within an hour | warning |

- **Evidence:** `active`, `sub`, `result`; `restarts` and `window` for restart loops.

## `smart.health`

Runs `smartctl --scan-open`, then `smartctl --all` per device, both as JSON. A disk in standby is not woken and reports nothing that run.

Counters fire on an **increase** over the value recorded in the check's state, never on a non-zero value alone:

- The first time a counter is seen, its value is recorded without firing.
- A counter that drops (a replaced disk) is recorded again silently.
- A counter keeps firing until the increase is acknowledged.

| Target | Counter | Severity on increase |
| --- | --- | --- |
| `<device>:reallocated_sector_ct` | ATA 5 | warning |
| `<device>:reported_uncorrect` | ATA 187 | warning |
| `<device>:current_pending_sector` | ATA 197 | critical |
| `<device>:offline_uncorrectable` | ATA 198 | critical |
| `<device>:udma_crc_error_count` | ATA 199 | warning, with a hint to check the cable |
| `<device>:media_errors` | NVMe media errors | critical |

The device itself (target `/dev/sda`, `/dev/nvme0`) fires when:

- The drive's overall assessment fails, or the NVMe critical warning is not zero: critical.
- NVMe `percentage_used` is 90 or more, or the NVMe temperature is 70 °C or more: warning.

Also:

- **Evidence:** `value`, `previous`, `delta` (and `hint`) for counters; `model`, `serial`, `power_on_hours`, `temperature_c` on every observation.
- **Facts:** `smart.<disk>.health` (`PASSED` or `FAILED`) and `smart.<disk>.temperature_c` (°C).

## `security.ssh_login`

Reads the `ssh.service` and `sshd.service` journal since the last run (with a cursor, like `kernel.errors`) and reports every `Accepted <method> for <user> from <ip> port <port>` line, one observation per user.

- **Severity:** warning when any login in the run came from the internet; info when all came from the local network.
- The local network covers RFC 1918, loopback, link-local, CGNAT (100.64.0.0/10) and unique-local IPv6 (fc00::/7). IPv4-mapped IPv6 addresses are judged as IPv4.
- **Target:** the user.
- **Evidence:** `ip`, `method`, `port`, `network` (`lan` or `wan`) of the latest internet login, else of the latest login; `count`.

## `security.sudo_failure`

Reads journal messages with the `sudo` syslog identifier since the last run and fires with warning severity for `authentication failure` and `incorrect password attempts`.

- **Target:** the user.
- **Evidence:** `count`, `example`.

## `security.fail2ban`

Optional: the check reads nothing until you list jails.

| Setting | Default | Meaning |
| --- | --- | --- |
| `jails` | `[]` | The jails to read |
| `bans_per_hour` | 20 | Bans within an hour worth a note |

Bans in the last hour come from the growth of `Total banned` in `fail2ban-client status <jail>`. The observation has info severity, and every count is evidence only.

- **Target:** the jail.
- **Evidence:** `bans_last_hour`, `currently_banned`, `total_banned`, `total_failed`.

## `security.new_port`

Watches listening sockets in `/proc/1/net/{tcp,tcp6,udp,udp6}`, which is the host's network namespace even when the agent runs in a container. It falls back to `/proc/net/*`.

- TCP sockets count when they are listening.
- UDP sockets count when they are unconnected and bound outside the ephemeral range in `/proc/sys/net/ipv4/ip_local_port_range`, so client sockets are ignored.
- Loopback listeners are ignored.

The first run learns every listener without reporting it. Later, a listener seen for the first time fires once, with warning severity, and is learned after two runs. Learned listeners gone for `expiry_days` are forgotten.

| Setting | Default | Meaning |
| --- | --- | --- |
| `expiry_days` | 7 | Days a learned listener may be gone before it is forgotten |

- **Target:** `addr:port/proto`, such as `0.0.0.0:22/tcp` or `[::]:443/tcp`.
- **Evidence:** `address`, `port`, `proto`.

## Network probes

Probes run from the hub, one per `[[probes]]` entry, and report on the probe's `host`. They are not part of the agent's checks.

### `probe.tcp`

Opens a TCP connection to `host:port` within 5 seconds.

- **Target:** `host:port`.
- **Severity:** warning, or critical with `criticality = "critical"`.
- **Evidence:** `latency_ms` when up; `error` when down.

### `probe.http`

Sends a GET to the URL (following at most 5 redirects) and expects an answer within 10 seconds. It fires with warning severity when:

- the request fails;
- the status is not one of the expected statuses;
- the HTTPS certificate expires within 14 days.

Certificates are verified against the operating system's trust store.

- **Target:** the URL without user info, query or fragment, so a token in the query string never becomes part of a fingerprint.
- **Evidence:** `status`, `latency_ms`, `cert_days_left` (HTTPS), `error`.

### `probe.gateway`

Reads the default route from `/proc/1/net/route` (falling back to `/proc/net/route`) and tries TCP ports 53, 80 and 443 on the gateway at once. Any answer counts, including a refused connection, since the refusal comes from the gateway itself. The hub uses the same test to tell its own network problems from a host's.

- **Target:** none.
- **Severity:** warning when there is no default route or the gateway does not answer.
- **Evidence:** `gateway`, `interface`.

## Command templates

Checks and the agent's read tools run only these commands. A journal call reads one scope (the kernel, units or syslog identifiers), because journalctl combines filters of different kinds with AND. Each program is looked up once in a fixed list of absolute paths, with no `PATH` search. Each value is its own argument and may not start with `-`. Every command has a 15-second timeout and captures at most 2 MiB per stream.

| Template | Program | Arguments |
| --- | --- | --- |
| `journal` | `journalctl` | `--no-pager --quiet --output=short-iso --since=-<minutes>min --lines=<n>`, then `--dmesg` or `--unit <unit>` |
| `journal_since_cursor` | `journalctl` | `--no-pager --quiet --output=cat --show-cursor`, then `--after-cursor=<cursor>` or `--since=-15min`, then one scope: `--dmesg`, `--unit <unit>` per unit, or `--identifier <id>` per syslog identifier |
| `smartctl_scan` | `smartctl` | `--scan-open --json` |
| `smartctl_all` | `smartctl` | `--all --json --nocheck=standby <device>` |
| `systemctl_show` | `systemctl` | `show --no-pager --property=<props> -- <unit>` |
| `systemctl_failed` | `systemctl` | `list-units --state=failed --no-legend --plain --no-pager` |
| `systemctl_failed_json` | `systemctl` | the same, then `--output=json` |
| `fail2ban_status` | `fail2ban-client` | `status <jail>` |

Slot rules:
- **Unit names:** `[A-Za-z0-9@._:-]`, 1 to 128 characters.
- **Jail names:** `[A-Za-z0-9_-]`, 1 to 64 characters.
- **Syslog identifiers:** `[A-Za-z0-9_.-]`, 1 to 64 characters.
- **Devices:** `/dev/` followed by `[a-z0-9]`.
- **Unit properties:** letters only.
- **Journal cursors:** printable ASCII without spaces, at most 512 characters.
