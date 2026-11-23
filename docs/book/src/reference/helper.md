# Helper

The helper is a small root program that sits behind a socket on hosts installed with the three-identities layout (see [Security model](../security.md#three-identities-on-one-host)). The agent asks it for the few things that need root. It answers one request per connection and exits.

In this release it answers `health`, `groups`, the read groups and `safety.attest`. Every read group and `safety.attest` is answered as unavailable, because the operations behind them are not built yet. It has no write group: it cannot restart a unit, change the firewall, act on a container or run a command, and it starts no process of its own. Platform support is not claimed for any system.

Who is served: only the `domovoi-agent` account. The helper looks up that account's user id each time a connection arrives. Root is not served; use `domovoi agent selftest` for root diagnostics. A request is one length-prefixed JSON frame of at most 64 KiB, with a closed set of operations and no free-form field, so there is nothing to carry a path, a command or a unit name. If the helper's own checks fail at start it answers every request with `StartRefused` and the list of faulty paths. See [Troubleshooting](../troubleshooting.md#the-helper-refuses-to-start).

## `helper.toml`

`/etc/domovoi/helper.toml`, owner root, mode 0600. The installer writes it only when it does not exist, and nothing else edits it. An unknown key, an unknown group name or a duplicate group makes the helper refuse to start with `helper_config_invalid`.

```toml
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest"]

[platform]
allow_write_groups_on_untested = false

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
| `groups` | The operation groups to enable. No file, or an empty list, gives the set shown above. Write groups (`docker.write`, `systemd.write`, `nft`) are accepted by the parser but ignored in this release, and reported in the answer to `groups`. No write group is ever enabled by default. |
| `[platform] allow_write_groups_on_untested` | The per-host switch for write groups on an Untested platform. Off by default; you edit it yourself. It has no effect in this release, because no write group exists, but while it is on the helper logs a warning at every start and reports it. |
| `[smart] devices` | Device patterns for the SMART read group. Read, not used yet. |
| `[safety]` and `[safety.containment]` | Management networks, protected units and containers and the containment limits. Read, not used yet. |
| `[limits]` | Request timeout and the mutation budget. Read, not used yet. |

`[docker]`, `[systemd]` and `[nft]` sections are accepted with their documented keys and are not used yet.

### Platform

The helper never trusts a tier a caller claims. `sudo domovoi agent selftest --installer --record` writes the result of the checks to `/var/lib/domovoi-helper/platform.json`, and the helper reads that file. If it is missing, or was written under a different kernel, write groups are refused with `platform_record_missing` or `platform_record_stale`. Nothing is Supported yet; a host whose checks all pass is Untested, and write groups on it need `allow_write_groups_on_untested = true`. The notice shown for an Untested host reads: "This platform has not been release-tested. Skills run here after you acknowledge that; privileged write operations stay off unless you enable them for this host in helper.toml."

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
PrivateDevices=yes
CapabilityBoundingSet=CAP_NET_ADMIN
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
| `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes` | A read-only system for a root process; `strict` leaves `/home` writable, so `ProtectHome` is needed too. |
| `ReadWritePaths=/var/lib/domovoi-helper` | The only writable path. If it is missing the unit fails to start, which is why the installer creates it first. |
| `CapabilityBoundingSet=CAP_NET_ADMIN`, `AmbientCapabilities=` | The value that was measured. This release's operations need none of it. Narrowing it per operation is not done yet. |
| `RuntimeMaxSec=60s`, `KillMode=control-group`, `TimeoutStopSec=5s` | A time bound on one request, written out so a test can check it. |

The helper unit carries no other hardening setting on purpose: the rest have not been measured with the helper's later operations. Do not add a drop-in; see [Hardening](../guide/hardening.md#units-and-drop-ins).

## Files

| Path | Owner and mode | Notes |
| --- | --- | --- |
| `/var/lib/domovoi-helper` | root, 0700 | Created by `domovoi helper init`. |
| `/var/lib/domovoi-helper/state.lock` | root, 0600 | The lock held across a request. |
| `/var/lib/domovoi-helper/platform.json` | root, 0600 | Written by `agent selftest --installer --record`. |
| `/etc/domovoi/helper.toml` | root, 0600 | Described above. |
| `/run/domovoi-helper.sock` | `root:domovoi-agent`, 0660 | Created by the socket unit. |

A wrong mode, owner, group or type on any of these, or a symlink at one of these paths, stops the helper at start and is reported path by path. The helper never repairs it; `sudo domovoi helper repair` does, after you confirm (see [Command line](cli.md#domovoi-helper-serve-init-and-repair)).
