# Helper

The helper is a small root program that sits behind a socket on hosts installed with the three-identities layout (see [Security model](../security.md#three-identities-on-one-host)). The agent asks it for the few things that need root. It answers one request per connection and exits.

In this release it answers `health`, `groups`, the Docker, SMART, kernel-log and listening-socket reads described under [Read operations](#read-operations), `safety.attest` and the read-only `safety.public_key`. `systemd.read` answers the identity of one unit, and `systemd.write` restarts one allowlisted unit (see [Restarting a service](../guide/unit-restart.md)). `safety.attest` answers a challenge with a report of the effective safety set (`[safety]` in `helper.toml`, the mandatory units, and the routes to the hub and the management networks as read from the kernel with `ip -j`), signed with the helper's attestation key; `safety.public_key` answers the public half of that key and nothing else. Its write groups are `systemd.write` and `nft`. `nft` blocks one external address or small network for a limited time (see [Firewall operations](#firewall-operations)); the helper cannot act on a container or run a command. The only programs it ever runs are `ip` (to read routes), `smartctl`, `dmesg` and `ss`, and, for the `nft` group, `nft` and `conntrack`, each with fixed arguments, an empty environment (`LC_ALL=C` for `ip`, `nft` and `conntrack`) and never through a shell. It reads Docker through a client that can only make five `GET` calls. Platform support is not claimed for any system.

Who is served: only the `domovoi-agent` account. The helper looks up that account's user id each time a connection arrives. Root is not served; use `domovoi agent selftest` for root diagnostics. A request is one length-prefixed JSON frame of at most 64 KiB, with a closed set of operations and no free-form field (a container name and a `/dev/` device name are checked against a fixed grammar), so there is nothing to carry a path, a command or a unit name. An answer may be up to 4 MiB and is cut, with a `truncated` flag, rather than sent larger. If the helper's own checks fail at start it answers every request with `StartRefused` and the list of faulty paths. See [Troubleshooting](../troubleshooting.md#the-helper-refuses-to-start).

## `helper.toml`

`/etc/domovoi/helper.toml`, owner root, mode 0600. The installer writes it only when it does not exist, and nothing else edits it. An unknown key, an unknown group name or a duplicate group makes the helper refuse to start with `helper_config_invalid`. After editing the file, check it with `sudo domovoi check config /etc/domovoi/helper.toml`, which parses it as the helper does.

```toml
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest"]

[platform]
allow_write_groups_on_untested = false

[docker]
endpoint = "unix:///var/run/docker.sock"

[smart]
devices = ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]

[safety]
management_cidrs = []
protected_units = ["sshd.service", "domovoi-agent.service", "domovoi-helper.socket", "domovoi-skill-runner.socket"]
protected_containers = []

[safety.containment]
max_ttl = "1h"
max_active = 0

[limits]
timeout = "30s"
mutations_per_hour = 0
```

| Key | Meaning |
| --- | --- |
| `groups` | The operation groups to enable. No file, or an empty list, gives the set shown above. Remove `docker.read`, `smart.read`, `kernel.read` or `net.read` to turn that read group off; its requests are then answered as disabled. `systemd.write` and `nft` are the write groups that do anything in this release; `docker.write` is accepted by the parser and ignored. No write group is ever enabled by default, and both need the platform rules below. |
| `[platform] allow_write_groups_on_untested` | The per-host switch for write groups on an Untested platform. Off by default; you edit it yourself. It lets `systemd.write` run on an Untested platform. While it is on, the helper logs a warning at every start and reports it, and every restart it allows says so in the audit log. |
| `[docker] endpoint` | Optional. A `unix:///absolute/path` socket for the Docker read group. A `tcp://` or `http://` value, or a relative path, makes the file invalid: the root helper opens no network connection. Without it the helper tries `/var/run/docker.sock`, then `/run/podman/podman.sock`. |
| `[smart] devices` | Device patterns for the SMART read group. A device is read only when the whole name matches one pattern; an empty list means no device. The grammar is literal `a-z`, `0-9` and `/` characters plus single-character classes `[a-z]` and `[0-9]`. Any other character (`*`, `?`, `{`, `..`) makes the file invalid. |
| `[safety]` and `[safety.containment]` | Management networks, protected units and containers and the containment limits. `protected_units` is used: a protected unit is never restarted, whoever asks. The helper always adds the four default units to the list, so removing one from the file does not unprotect it. The helper also reads `[systemd] restart_units`, the units it may restart. They are part of the signed attestation (one `restart_unit` line each; an empty list leaves the config hash unchanged), so the hub shows them under "May restart" on the Helper tab and refuses a restart of a unit that is not listed with `helper_unit_not_listed`. An old hub cannot parse the report of a helper that lists restart units: upgrade the hub first. `management_cidrs` and the `[safety.containment]` limits (`max_ttl`, `max_active`) are used by the `nft` group. |
| `[nft]` | The firewall table and its limits, described under [Firewall operations](#firewall-operations). |
| `[systemd] restart_units` | The unit ids `systemd.write` may restart. Empty means none. A unit that is not listed is refused with `target_not_allowlisted` even when the agent and the hub both allow it. |
| `[limits]` | The request timeout and `mutations_per_hour`, the most restarts the helper accepts in an hour (0 means the helper accepts none). A restart also has a per-unit limit. The counts are kept in the helper's own state file, so they survive a restart of the helper. |

`[docker] write_containers` is still accepted and not interpreted.

### Platform

The helper never trusts a tier a caller claims. `sudo domovoi agent selftest --installer --record` writes the result of the checks to `/var/lib/domovoi-helper/platform.json`, and the helper reads that file. If it is missing, or was written under a different kernel, write groups are refused with `platform_record_missing` or `platform_record_stale`. No platform is Supported in this release; a host whose checks all pass is Untested, and write groups on it need `allow_write_groups_on_untested = true`. The notice shown for an Untested host reads: "This platform has not been release-tested. Skills run here after you acknowledge that; privileged write operations stay off unless you enable them for this host in helper.toml."

## Read operations

Each operation belongs to one group. The group is the unit of enablement: there is no per-operation switch.

| Group | Operations | What it reads |
| --- | --- | --- |
| `docker.read` | probe, list, events, inspect, logs | The container engine, through five `GET` calls: the container list (500 at most), one container's `inspect` (every `Config.Env` value is removed, 1 MiB at most), the container events of a window of up to 24 hours (1,000 at most) and the last 1 to 500 log lines (256 KiB at most). The helper holds the socket; the agent never does. |
| `smart.read` | scan, report | `smartctl --scan-open --json` filtered by `[smart] devices` (64 devices at most) and `smartctl --all --json --nocheck=standby <device>` for one allowed device. A non-zero exit with JSON output is a normal report. |
| `kernel.read` | kernel log | `dmesg --kernel --time-format=iso`, cut to a window of 1 to 1,440 minutes and the last 1 to 500 lines. The agent asks only when `journalctl --dmesg` cannot open the journal. |
| `net.read` | listening sockets | `ss --listening --tcp --udp --processes`, at most 1,000 sockets, with the owning process name and pid. |
| `systemd.read` | unit identity | One unit's loaded properties from `busctl --json=short`, its unit file and drop-in files, and the checks that bind them. It never reloads systemd and never reads an environment file. |
| `systemd.write` | unit restart | `systemctl restart -- <unit id>` for one unit that is in `[systemd] restart_units`, not protected, and whose identity still matches the request. Needs a platform record that allows write groups. See below. |

Every operation has a timeout (2 to 15 seconds), an output cap and an item cap; `[limits] timeout` bounds the whole request when it is smaller. The helper writes nothing for a read and keeps no cache; the agent caches the container list and listening sockets for 5 seconds, the SMART scan for 5 minutes and a SMART report for 1 minute per device, and sends at most 20 read requests in 10 seconds. A read that cannot be done is answered with a token:

| Token | Meaning |
| --- | --- |
| `tool_missing` | `smartctl`, `dmesg` or `ss` is not installed at a fixed path as a root-owned file that others cannot write. |
| `engine_absent` | No Docker or Podman socket exists. |
| `engine_denied` | The socket exists and the helper may not open it. |
| `engine_error` | The engine answered a transport error or an unexpected status. |
| `engine_api_refused` | The engine, or a proxy in front of it, answered 403 or 405. The helper does not try another way. |
| `target_not_found` | The engine has no such container. |
| `device_not_allowed` | The device does not match `[smart] devices`. Nothing was run. |
| `command_failed` | The tool exited without usable output; the answer carries the first line of its error, cut short. A missing capability shows here. |
| `output_malformed` | The tool's output was not in the expected shape. |
| `timed_out` | The per-call timeout or the request deadline passed. |

A request for a group that is not enabled is answered as disabled. Reads are refused only when the platform record for this kernel lists a failed helper isolation check (ST-10, ST-11, ST-12 or ST-15); a missing or stale record, an Untested platform and a host where only the skill-runner checks fail still allow reads. See [Troubleshooting](../troubleshooting.md#the-helper-answers-a-read-with-a-token).

## The units

`/etc/systemd/system/domovoi-helper.socket`:

```ini
[Unit]
Description=Domovoi helper socket
After=nftables.service

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

`/etc/systemd/system/domovoi-helper@.service` (the installer rewrites `/usr/local/bin/domovoi` to your prefix):

```ini
[Unit]
Description=Domovoi helper (connection %i)
After=nftables.service

[Service]
Type=simple
# Root: no User= line on purpose.
StandardInput=socket
StandardOutput=journal
StandardError=journal
ExecStart=/usr/local/bin/domovoi helper serve
Restart=no
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/domovoi-helper
ProtectHome=yes
PrivateTmp=yes
RestrictNamespaces=yes
# Disks and /dev/kmsg are needed by the read operations (smartctl, dmesg); every other
# device stays closed. The capability set is the widened baseline of ADR 0036: the
# privileged rows of deploy/test/identities.sh measure it (decision OD-1 is unmeasured
# until they have run on a real host).
PrivateDevices=no
DevicePolicy=closed
DeviceAllow=block-sd r
DeviceAllow=block-nvme r
DeviceAllow=char-nvme r
DeviceAllow=char-sg r
CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_SYSLOG CAP_SYS_PTRACE CAP_DAC_READ_SEARCH
AmbientCapabilities=
RuntimeMaxSec=60s
KillMode=control-group
TimeoutStopSec=5s
```

| Directive | Why it is there |
| --- | --- |
| `SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660` | The first layer: a user outside the group cannot connect. 0660 is the widest allowed; a 0666 socket is not protected by any other setting. |
| `Accept=yes`, `MaxConnections=8` | One process per connection, so nothing stays in memory between requests. `MaxConnections` was loaded as written; what happens at the limit has not been measured. |
| `RemoveOnStop=yes` | Otherwise the socket file survives a stop. |
| `Type=simple` | `RuntimeMaxSec` is ignored with `oneshot`. |
| no `User=` | The helper is root. The user check is its first layer, not the isolation. |
| `StandardInput=socket`, `StandardOutput=journal`, `StandardError=journal` | The connection is the helper's standard input and carries the requests; its output goes to the journal, not to the connection. |
| `ExecStart=` outside `/run` | `/run` can be mounted `noexec`, which stops a program there from starting. |
| `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes` | A read-only system for a root process; `strict` leaves `/home` writable, so `ProtectHome` is needed too. |
| `RestrictNamespaces=yes` | The long-lived helper never creates a namespace. Only the self-test unit and the drill command do. |
| `After=nftables.service` | The firewall rules are loaded before the helper answers a block request. |
| `PrivateDevices=no`, `DevicePolicy=closed`, four `DeviceAllow=` lines | `PrivateDevices=yes` hides the block devices, so `smartctl` would find none. `closed` keeps every other device out of reach except the always-allowed pseudo devices; the four lines give read access to SATA and SCSI disks, NVMe disks and controllers, and SCSI generic nodes. |
| `ReadWritePaths=/var/lib/domovoi-helper` | The only writable path. If it is missing the unit fails to start, which is why the installer creates it first. |
| `CapabilityBoundingSet=` (six capabilities), `AmbientCapabilities=` | `CAP_SYS_RAWIO` for SMART commands on SATA and SCSI disks; `CAP_SYS_ADMIN` for NVMe admin commands (the largest single addition); `CAP_SYSLOG` for `dmesg` when `kernel.dmesg_restrict=1`; `CAP_SYS_PTRACE` and `CAP_DAC_READ_SEARCH` so `ss --processes` can name the owner of another user's listener; `CAP_NET_ADMIN` is kept from the first baseline and nothing uses it now. The set is shared by every request and is not narrowed when you turn a group off. It has not been measured on a real host yet. |
| `RuntimeMaxSec=60s`, `KillMode=control-group`, `TimeoutStopSec=5s` | A time bound on one request, written out so a test can check it. |

The helper unit carries no other hardening setting on purpose: the rest have not been measured with the helper's operations. An already migrated host keeps its old unit when it differs; replace it with the file the installer saved under `<prefix>/share/domovoi/` (then `systemctl daemon-reload`) to get SMART, kernel-log and listening-socket reads. Docker reads need no unit change. Do not add a drop-in; see [Hardening](../guide/hardening.md#units-and-drop-ins).

## Files

| Path | Owner and mode | Notes |
| --- | --- | --- |
| `/var/lib/domovoi-helper` | root, 0700 | Created by `domovoi helper init`. |
| `/var/lib/domovoi-helper/state.lock` | root, 0600 | The lock held across a request. |
| `/var/lib/domovoi-helper/attest.key` | root, 0600 | The Ed25519 seed that signs safety attestations. Created by `domovoi helper init`; replaced only by `rotate-key` and `recover`. |
| `/var/lib/domovoi-helper/state` | root, 0600 | The attestation generation and the hashes it was made from, with a checksum. |
| `/var/lib/domovoi-helper/retired/` | root, 0700 | The public key and the old state of every retired key. Never a private key. |
| `/var/lib/domovoi-helper/platform.json` | root, 0600 | Written by `agent selftest --installer --record`. |
| `/var/lib/domovoi-helper/containment.journal` | root, 0600 | The `nft` group's write-ahead journal, one JSON line per record. |
| `/var/lib/domovoi-helper/nft-selftest.json` | root, 0600 | The result of the leak self-test. |
| `/var/lib/domovoi-helper/drills.json` | root, 0600 | The result of `domovoi helper containment-drill`. |
| `/etc/domovoi/helper.toml` | root, 0600 | Described above. |
| `/run/domovoi-helper.sock` | `root:domovoi-agent`, 0660 | Created by the socket unit. |

A wrong mode, owner, group or type on any of these, or a symlink at one of these paths, stops the helper at start and is reported path by path. The helper never repairs it; `sudo domovoi helper repair` does, after you confirm (see [Command line](cli.md#domovoi-helper-serve-init-and-repair)).

## Firewall operations

The `nft` group has four operations: `firewall_apply`, `firewall_confirm`, `firewall_rollback` and `firewall_list_blocks`. It is a write group, so it needs `"nft"` in `groups` and the platform rules above. It blocks inbound traffic from one external address or small network for a limited time, in a table of its own, and nothing else: it never touches another table, never flushes the ruleset and never saves anything to disk.

```toml
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest", "nft"]

[nft]
table = "domovoi"
max_elements = 1000
call_timeout = "5s"

[safety]
management_cidrs = ["192.0.2.0/24"]

[safety.containment]
max_ttl = "1h"
max_active = 0
```

| Key | Meaning |
| --- | --- |
| `[nft] table` | The table the helper owns, in the `inet` family. Lower-case letters, digits and underscores, starting with a letter, at most 32 characters. Default `domovoi`. |
| `[nft] max_elements` | The size of each block set, 1 to 10000. Default 1000. |
| `[nft] call_timeout` | How long one `nft` or `conntrack` call may run, 1 to 30 seconds. Default 5 seconds. |
| `[safety] management_cidrs` | The networks the host is managed from. The helper refuses to block any address that overlaps one, and refuses all blocking while the list is empty (`no_management_cidrs`). |
| `[safety.containment] max_ttl` | The longest a block may last, above 30 seconds and at most 365 days. A longer request is refused, never shortened. Default 1 hour. |
| `[safety.containment] max_active` | The most blocks the helper holds at once, counting every element in the block sets, including ones it did not create. Default 0, which refuses every block; it may not exceed `max_elements`. |

An unknown key is still a start error. The shortest network the helper will block is a `/24` for IPv4 and a `/64` for IPv6, whatever the caller says; `/0` is never accepted.

**The table.** The helper creates `inet <table>` the first time it is needed, and only when it is absent. It holds a chain `contain` at hook `prerouting`, priority -310, with no `ct` rule, and sets `excl4` and `excl6` (the addresses never blocked), `blocked4` and `blocked6` (single hosts, with a timeout) and, on hosts where the self-test passed it, `blockednet4` and `blockednet6` (networks). Every request first reads the table back as JSON and compares it with what the helper would create. If they differ, new blocks are refused with `table_drift` and nothing is repaired automatically. If the table has disappeared while a block was live, for example after `nftables.service` was reloaded with `flush ruleset`, the helper records those blocks as lost, re-creates the empty table and reports `containment_lost`; it never applies a lost block again. Do not save `inet domovoi` into a persisted ruleset: a block must not outlive a reboot.

**Two steps.** `firewall_apply` creates the element with a 30 second kernel timeout. `firewall_confirm` replaces it, in one batch, with the requested lifetime. If nothing confirms, the kernel removes the element after 30 seconds. `firewall_rollback` removes it and succeeds whether the element is present, absent or already expired. Each phase carries the block's id, the source network and the lifetime.

**Checks.** `apply` and `confirm` refuse, before any `nft` call, when `expected_safety` is missing (`malformed_safety`) or does not match the helper's own identity (`safety_changed`), when the target is not a canonical network (`invalid_target`, `mapped_form`), is too wide (`prefix_too_wide`) or overlaps an excluded range in either direction (`exclusion_overlap`), when the lifetime is 30 seconds or less (`ttl_too_short`) or above the maximum (`ttl_above_maximum`), when the active count is reached (`max_active_reached`) or the hourly limit is (`mutation_limit`). The excluded ranges are the management networks, the hub's address, the addresses the helper finds on its own interfaces, its default gateways and the loopback, link-local and multicast ranges. `rollback` is the one operation that skips the safety, overlap and limit checks, because it only removes a block; it still takes the lock, writes the journal and canonicalises its target. Every mutation holds `state.lock` and is written to `containment.journal` before the `nft` call it describes. A journal that cannot be read refuses all blocking (`journal_corrupt`).

**Existing connections.** The chain has no `ct` accept, so a connection that is not offloaded is cut by the drop. On a host with an nftables flowtable the helper also runs `conntrack -D -s <source>` for a single address and for networks of at least `/24` (IPv4) or `/120` (IPv6), and reports `existing_flows` as `cut`, `n/a` (no flowtable) or `may_persist`. Hardware offload is not covered. The helper reports `bridge_gap` when a physical interface is a port of a bridge: the table does not see traffic that the bridge forwards.

**The leak self-test.** Replacing an element must never leave a window in which the source is not blocked. `sudo domovoi helper nft-selftest` checks that on this kernel and this `nft` version, in a throwaway network namespace with its own table, and writes `nft-selftest.json`, keyed by kernel release, `nft` version and layout. It takes minutes. It never runs inside the long-lived helper. The one-shot unit `domovoi-helper-selftest.service` runs it with `--if-changed` at every boot and after the installer, before the helper's socket; the installer starts it too. The result decides what the helper does:

| Result | What the `nft` group does |
| --- | --- |
| Single hosts pass | Blocks single addresses. Networks are refused with `prefix_unsupported_platform`. |
| Single hosts and networks pass | Blocks both. |
| Any packet leaked for single hosts | Refuses everything (`containment_blocked`). |
| Inconclusive, not run, or the key changed | Refuses everything (`containment_blocked`). A run that could not be completed is never a pass. |

A passing result is weak evidence: it does not cover a physical NIC, several receive queues, a conntrack rule, lower packet rates or what the live set holds.

**The drills.** `sudo domovoi helper containment-drill` runs the helper's own operations in a throwaway namespace (expiry, an excluded address, an unconfirmed block, a restart in the middle of a block, a repeated request, the active limit) and writes `drills.json`. A result written under another kernel, `nft` version or layout counts as not run. The hub does not dispatch a block on a policy's say-so until the host's drills have passed.

`sudo domovoi helper nft-recreate` deletes the helper's own table when it is empty and no block is live, so the next request builds it again. Use it after changing `max_elements` or the layout.

**Privilege.** The long-lived helper holds `CAP_NET_ADMIN` to write the table and never creates a namespace (`RestrictNamespaces=yes`). Only the self-test unit and the drill command create a namespace and links; the unit's capability bounding set, `CAP_NET_ADMIN` and `CAP_SYS_ADMIN`, has not been measured on a real host yet.

## Safety attestation

`safety.attest` takes the hub's 32-byte nonce and returns a report of the effective safety set, signed with `attest.key`. The report names the key id, the nonce, the boot id, a generation, two hashes (one over the effective `[safety]` configuration, one over the route state), the configured networks, protected units and containers, the containment limits and the routes to the management endpoints. The generation rises by exactly one whenever either hash changes; a new boot with the same content does not change it. The helper holds the lock in `state.lock` for the whole request, writes the new state durably before it signs, and does not sign a report over 64 KiB.

The management endpoints are the hub's address and every address in `management_cidrs`. The hub's address is the literal IP in `[agent] hub_url` of `agent.toml`. **If `hub_url` names a host, the helper finds no hub address.** It does not resolve names, because a name the agent controls must not decide what is protected. Cover the hub with a `management_cidrs` entry. There are no link-peer lines in this release.

Routes are read by running `ip -N -j` with the environment `LC_ALL=C`: the policy rules, the tables they name plus `main` and `local`, the interface addresses and, for each endpoint, `ip -N -j -d route get <address>`. If the report cannot say what is true, the helper signs an incomplete report that names the reason, and nothing is claimed:

| Reason | Meaning |
| --- | --- |
| `route_unrepresentable` | A policy rule that is not a table lookup, a selected route that is a blackhole, multipath or local route, or an interface name that is not UTF-8. A hub on the same host (a loopback hub URL, as in single-box mode) is reached through the local table and lands here. |
| `rules_unreadable`, `table_unreadable`, `addresses_unreadable`, `lookup_failed`, `route_read_partial` | `ip` was missing, failed, timed out, printed too much or printed something that could not be read completely. |
| `config_unreadable`, `config_invalid` | `helper.toml` or `agent.toml` could not be read, or a value breaks its grammar. |
| `state_missing`, `state_corrupt`, `state_key_mismatch` | The state file is absent, fails its checksum or format, or belongs to another key. Run `sudo domovoi helper recover`. |
| `state_write_failed` | The new state could not be written, so no generation change is visible. |
| `report_too_large` | The report would exceed 64 KiB. |

When the helper has no usable key it cannot sign, and answers with an unsigned token instead. The agent relays it; the hub shows it as a hint and treats the host as unverified.

| Token | Cause | What to do |
| --- | --- | --- |
| `uninitialized` | Neither key nor state exists. | `sudo domovoi helper init`. |
| `key_missing` | The state exists and the key does not. The helper never makes a key itself. | `sudo domovoi helper recover`, then enrol the new key. |
| `key_corrupt` | `attest.key` is not a 32-byte seed. | `sudo domovoi helper recover`. |
| `helper_state_permissions` | A mode, owner or type is wrong on the directory, key, state or lock. | `sudo domovoi helper repair`. |
| `lock_unavailable` | `state.lock` cannot be opened or locked. | Check the file and what holds it, then retry. |
| `helper_unavailable` | Sent by the agent: the helper is absent, unreachable or does not serve the operation. | Check the socket and the helper units. |

None of these changes state by itself. The helper never resets state or creates a key implicitly.
