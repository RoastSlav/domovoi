# Binary and systemd

The installer puts the release binary on a Linux machine (x86_64 or arm64) and sets up a systemd service for the hub or for an agent. It writes no config file: the hub is set up from its web UI, and an agent by the enroll command the hub gives you.

## Install the hub

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
```

Then start it and open the setup link it prints once:

```sh
sudo systemctl enable --now domovoi-hub
sudo journalctl -u domovoi-hub | grep -A1 setup
```

The link points at `http://127.0.0.1:7420` until you set the hub's address. From another machine, replace that part with this machine's address, or reach it through an SSH tunnel (`ssh -L 7420:127.0.0.1:7420 <host>`). The web UI listens only on loopback by default. Put it behind a reverse proxy with TLS, or set its listen address as described in [config as code](config-as-code.md#bootstrap-settings).

Create the admin account. The guided first run then sets up the hub's name and addresses, Discord, a model, the daily digest and your first host. Every step except the first can be skipped and done later on the Settings page.

To check the whole pipeline without network access or a model:

```sh
sudo -u domovoi domovoi selftest --offline
```

## Install an agent

On each machine to watch:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role agent
```

In the hub's web UI, open **Hosts**, then **Add host**, and run the enroll command it shows on that machine, with `sudo`. Then start the agent:

```sh
sudo systemctl enable --now domovoi-agent
```

The host shows as connected within a few seconds. The agent dials the hub on port 7421; nothing connects to the agent.

## Options

| Option | Meaning |
| --- | --- |
| `--role hub` or `--role agent` | What this machine runs. Required. |
| `--version v0.1.0` | Install this release instead of the latest. |
| `--prefix /opt/domovoi` | Install the binary as `<prefix>/bin/domovoi`. Default `/usr/local`. |
| `--no-service` | Install only the binary and examples: no user, directories or unit. Works without root when the prefix is writable. |
| `--print-unit hub` | Print the systemd unit and exit. |

## Verify the download

The installer downloads `domovoi-<version>-<target>.tar.gz` with its `.sha256` file, and installs nothing unless the checksum matches. Every release archive also carries a signed build attestation. To check it, download the archive yourself and run:

```sh
gh attestation verify domovoi-v0.1.0-x86_64-unknown-linux-musl.tar.gz --repo RoastSlav/domovoi
```

To read the script before running it, download it first:

```sh
curl -fsSLO https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh
less install.sh
sudo sh install.sh --role hub
```

## What it creates

| Path | Role | Notes |
| --- | --- | --- |
| `/usr/local/bin/domovoi` | both | The binary, mode 0755. |
| `/usr/local/share/domovoi/examples/` | both | Example config files, for reference. |
| `domovoi` user and group | hub | A system user with no login shell. |
| `/var/lib/domovoi` | hub | Mode 0750, owned by `domovoi`: the database, `secret.key`, the agent certificate and backups. |
| `/var/lib/domovoi-agent` | agent | Mode 0700, owned by root: the agent's key, its settings and its spool. |
| `/etc/domovoi` | both | Mode 0750. Empty unless you add files there. |
| `/etc/systemd/system/domovoi-hub.service` | hub | See [`deploy/systemd`](https://github.com/RoastSlav/domovoi/tree/main/deploy/systemd). |
| `/etc/systemd/system/domovoi-agent.service` | agent | |

The installer never enables or starts a service. Running it again replaces the binary and leaves everything else as it is. A unit you changed is kept, and the new one is saved next to the examples so you can compare them. To change a unit, prefer a drop-in (`sudo systemctl edit domovoi-hub`), which upgrades never touch.

## The units

The hub runs as the `domovoi` user in a tight sandbox: no new privileges, a read-only system, no access to home directories, and write access only to `/var/lib/domovoi`.

A `cli` model provider (Claude Code or Codex) runs as the `domovoi` user too, with `/var/lib/domovoi` as its home, and cannot see other home directories. Install the tool where that user can run it, and log it in as that user:

```sh
sudo -u domovoi -H claude login
```

Claude Code runs on Node.js, whose JIT the unit's `MemoryDenyWriteExecute=yes` blocks. Allow it with a drop-in (`sudo systemctl edit domovoi-hub`):

```ini
[Service]
MemoryDenyWriteExecute=no
```

The agent runs as root, because SMART needs the disks, the journal checks read every unit's log, and skills run as the user their manifest names. Its unit still keeps `/usr` and `/etc` read-only, gives it a private `/tmp`, and lets it write only its own state.

## One machine

To watch the machine the hub runs on, install both roles on it and enroll the agent like any other, with the hub at `wss://127.0.0.1:7421`. The two services stay separate, so the agent keeps its root access while the hub stays sandboxed.

The hub can instead run an agent for its own machine in the same process, with the `--with-local-agent` flag. [`deploy/systemd/single-box.conf`](https://github.com/RoastSlav/domovoi/blob/main/deploy/systemd/single-box.conf) is a drop-in that does this. The local agent then runs inside the hub's sandbox as the `domovoi` user, so SMART, journal checks and skills that switch users report errors. It needs an agent config file, which the drop-in describes. Prefer two services unless you need a single process.

## Next

- [Upgrading and uninstalling](upgrading.md)
- [Advanced: config as code](config-as-code.md), to manage the hub from files instead of the web UI.
