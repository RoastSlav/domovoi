# Helper

The helper is a small root program that sits behind a socket on hosts installed with the three-identities layout (see [Security model](../security.md#three-identities-on-one-host)). The agent asks it for the few things that need root. It answers one request per connection and exits.

In this release it answers `health`, `groups`, the Docker, SMART, kernel-log and listening-socket reads described under [Read operations](#read-operations), `safety.attest` and the read-only `safety.public_key`. `systemd.read` is answered as not implemented. `safety.attest` answers a challenge with a report of the effective safety set (`[safety]` in `helper.toml`, the mandatory units, and the routes to the hub and the management networks as read from the kernel with `ip -j`), signed with the helper's attestation key; `safety.public_key` answers the public half of that key and nothing else. It has no write group: it cannot restart a unit, change the firewall, act on a container or run a command. The only programs it ever runs are `ip` (to read routes), `smartctl`, `dmesg` and `ss`, each with fixed arguments, an empty environment (`LC_ALL=C` for `ip`) and never through a shell. It reads Docker through a client that can only make five `GET` calls. Platform support is not claimed for any system.

Who is served: only the `domovoi-agent` account. The helper looks up that account's user id each time a connection arrives. Root is not served; use `domovoi agent selftest` for root diagnostics. A request is one length-prefixed JSON frame of at most 64 KiB, with a closed set of operations and no free-form field (a container name and a `/dev/` device name are checked against a fixed grammar), so there is nothing to carry a path, a command or a unit name. An answer may be up to 4 MiB and is cut, with a `truncated` flag, rather than sent larger. If the helper's own checks fail at start it answers every request with `StartRefused` and the list of faulty paths. See [Troubleshooting](../troubleshooting.md#the-helper-refuses-to-start).

## `helper.toml`

`/etc/domovoi/helper.toml`, owner root, mode 0600. The installer writes it only when it does not exist, and nothing else edits it. An unknown key, an unknown group name or a duplicate group makes the helper refuse to start with `helper_config_invalid`.

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
| `groups` | The operation groups to enable. No file, or an empty list, gives the set shown above. Remove `docker.read`, `smart.read`, `kernel.read` or `net.read` to turn that read group off; its requests are then answered as disabled. Write groups (`docker.write`, `systemd.write`, `nft`) are accepted by the parser but ignored in this release, and reported in the answer to `groups`. No write group is ever enabled by default. |
| `[platform] allow_write_groups_on_untested` | The per-host switch for write groups on an Untested platform. Off by default; you edit it yourself. It has no effect in this release, because no write group exists, but while it is on the helper logs a warning at every start and reports it. |
| `[docker] endpoint` | Optional. A `unix:///absolute/path` socket for the Docker read group. A `tcp://` or `http://` value, or a relative path, makes the file invalid: the root helper opens no network connection. Without it the helper tries `/var/run/docker.sock`, then `/run/podman/podman.sock`. |
| `[smart] devices` | Device patterns for the SMART read group. A device is read only when the whole name matches one pattern; an empty list means no device. The grammar is literal `a-z`, `0-9` and `/` characters plus single-character classes `[a-z]` and `[0-9]`. Any other character (`*`, `?`, `{`, `..`) makes the file invalid. |
| `[safety]` and `[safety.containment]` | Management networks, protected units and containers and the containment limits. Read, not used yet. |
| `[limits]` | Request timeout and the mutation budget. Read, not used yet. |

`[systemd]` and `[nft]` sections are accepted with their documented keys and are not used yet. `[docker] write_containers` is still accepted and not interpreted.

### Platform

The helper never trusts a tier a caller claims. `sudo domovoi agent selftest --installer --record` writes the result of the checks to `/var/lib/domovoi-helper/platform.json`, and the helper reads that file. If it is missing, or was written under a different kernel, write groups are refused with `platform_record_missing` or `platform_record_stale`. Nothing is Supported yet; a host whose checks all pass is Untested, and write groups on it need `allow_write_groups_on_untested = true`. The notice shown for an Untested host reads: "This platform has not been release-tested. Skills run here after you acknowledge that; privileged write operations stay off unless you enable them for this host in helper.toml."

## Read operations

Each operation belongs to one group. The group is the unit of enablement: there is no per-operation switch.

| Group | Operations | What it reads |
| --- | --- | --- |
| `docker.read` | probe, list, events, inspect, logs | The container engine, through five `GET` calls: the container list (500 at most), one container's `inspect` (every `Config.Env` value is removed, 1 MiB at most), the container events of a window of up to 24 hours (1,000 at most) and the last 1 to 500 log lines (256 KiB at most). The helper holds the socket; the agent never does. |
| `smart.read` | scan, report | `smartctl --scan-open --json` filtered by `[smart] devices` (64 devices at most) and `smartctl --all --json --nocheck=standby <device>` for one allowed device. A non-zero exit with JSON output is a normal report. |
| `kernel.read` | kernel log | `dmesg --kernel --time-format=iso`, cut to a window of 1 to 1,440 minutes and the last 1 to 500 lines. The agent asks only when `journalctl --dmesg` cannot open the journal. |
| `net.read` | listening sockets | `ss --listening --tcp --udp --processes`, at most 1,000 sockets, with the owning process name and pid. |
| `systemd.read` | none | Answered as not implemented. |

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
| `/etc/domovoi/helper.toml` | root, 0600 | Described above. |
| `/run/domovoi-helper.sock` | `root:domovoi-agent`, 0660 | Created by the socket unit. |

A wrong mode, owner, group or type on any of these, or a symlink at one of these paths, stops the helper at start and is reported path by path. The helper never repairs it; `sudo domovoi helper repair` does, after you confirm (see [Command line](cli.md#domovoi-helper-serve-init-and-repair)).

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
