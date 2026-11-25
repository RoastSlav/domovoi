# Helper

The helper is a small root program that sits behind a socket on hosts installed with the three-identities layout (see [Security model](../security.md#three-identities-on-one-host)). The agent asks it for the few things that need root. It answers one request per connection and exits.

In this release it answers `health`, `groups`, the read groups, `safety.attest` and the read-only `safety.public_key`. Every read group is answered as unavailable, because the operations behind them are not built yet. `safety.attest` answers a challenge with a report of the effective safety set (`[safety]` in `helper.toml`, the mandatory units, and the routes to the hub and the management networks as read from the kernel with `ip -j`), signed with the helper's attestation key; `safety.public_key` answers the public half of that key and nothing else. It has no write group: it cannot restart a unit, change the firewall, act on a container or run a command. The only program it ever runs is `ip`, with fixed arguments and the environment `LC_ALL=C`, to read routes. Platform support is not claimed for any system.

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
