# Binary and systemd

The installer puts the release binary on a Linux machine (x86_64 or arm64) and sets up a systemd service for the hub or for an agent. The hub is set up from its web UI, and an agent by the enroll command the hub gives you. The only config file the installer writes is `/etc/domovoi/helper.toml` on an agent host, and only when it is absent.

## Install the hub

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
```

Then start it and open the setup link it prints once:

```sh
sudo systemctl enable --now domovoi-hub
sudo journalctl -u domovoi-hub | grep -A1 setup
```

The link points at `http://127.0.0.1:7420` until you set the hub's address. The web UI listens only on loopback by default. From another machine, either use an SSH tunnel (`ssh -L 7420:127.0.0.1:7420 <host>`) and keep the link unchanged, or put the UI behind a reverse proxy with TLS.

To open the UI directly on a trusted LAN, set both its listen address and the exact URL users will open. Replace the example IP with the hub's numeric LAN address:

```sh
printf 'DOMOVOI_WEB__BIND=0.0.0.0:7420\nDOMOVOI_WEB__PUBLIC_URL=http://192.168.1.50:7420\n' | sudo tee /etc/domovoi/hub.env
sudo systemctl restart domovoi-hub
journalctl -u domovoi-hub --no-pager -o cat | grep '/setup?token=' | tail -n 1
```

Both settings are required: the bind address makes the listener reachable, and the public URL lets the hub accept requests from that address. Restarting replaces the setup token, so use the complete new link printed after the restart. Do not use a placeholder such as `SERVER_LOCAL_IP` in the configuration.

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

On a systemd host with no hub on it, a fresh agent install sets up three identities: the unprivileged `domovoi-agent` account that runs the agent, the `domovoi-skill` account that skills will run as, and a root helper behind a socket. It creates the accounts, the directories with their modes, `/etc/domovoi/helper.toml`, the helper units and the skill runner units, starts the helper socket and then runs `domovoi agent selftest --installer --record`. Only if that passes does it start the skill runner socket and run the self-test again with the runner up. If a check fails, it stops the sockets it started, prints the report and exits with an error; a failed second check leaves the helper socket running and the runner socket stopped, so skills stay unavailable. [Security model](../security.md#three-identities-on-one-host) says what each identity may reach.

In the hub's web UI, open **Hosts**, then **Add host**, and run the enroll command it shows on that machine, with `sudo`. Enroll runs as root, so it hands the agent's files to `domovoi-agent` when it is done and prints one line saying so. Then start the agent:

```sh
sudo systemctl enable --now domovoi-agent
```

The host shows as connected within a few seconds. The agent dials the hub on port 7421; nothing connects to the agent.

The helper and the skill runner start on demand when the agent connects to their sockets, so you never enable them by hand. This release's helper answers only `health` and `groups`, so skills that need privileged access (restarting a container or a unit, vacuuming the journal) report errors on a fresh install. See [What a skill can reach](../guide/skills.md#what-a-skill-can-reach).

### Already installed

An agent installed before this layout existed keeps running as root. A plain run of the installer replaces the binary and nothing else; it prints "privilege boundary not in effect" and the command below. Nothing about the old agent's behaviour changes until you move it.

```sh
sudo sh install.sh --role agent --migrate-identity
```

The migration is never done by a routine rerun, because it takes root access away from checks you may rely on. It needs a terminal, shows the full plan first (accounts to create, every path whose owner or mode will change, the new unit, and what stops working) and asks you to type `migrate`. There is no `--yes`. It then saves the old unit and the old owners and modes under `/etc/domovoi/identity-migration/`, stops the agent, applies the layout, starts the agent and checks that it runs as `domovoi-agent`.

What loses root access: checks that need it (Docker through its socket, SMART, the journal of every unit) report errors on a migrated host until the helper carries them, and skills run as `domovoi-skill` through the skill runner, so a skill that names another `run_as` is refused until you edit its manifest. If the migration fails midway it rolls itself back. To roll back later:

```sh
sudo sh install.sh --role agent --rollback-identity
```

Rollback restores the saved unit and owners and stops the helper socket. The accounts, `/var/lib/domovoi-helper`, `helper.toml` and the helper unit files stay, unused; nothing deletes them.

The migration refuses when a drop-in under `/etc/systemd/system/domovoi-agent.service.d/` sets `User=`, `Group=` or `SupplementaryGroups=`: it names the file, and you remove it yourself.

### Where the boundary does not apply

These hosts keep today's agent, labelled "privilege boundary not in effect", and the installer does not touch their layout:

- no systemd on the host;
- a hub on the same host: both roles share `/etc/domovoi`, which the hub makes `root:domovoi` and the agent layout needs as `root:domovoi-agent`. Run the agent on its own host to get the boundary;
- container installs and the single-process mode.

A host in this state gets no unattended execution.

## Options

| Option | Meaning |
| --- | --- |
| `--role hub` or `--role agent` | What this machine runs. Required. |
| `--version v0.1.0` | Install this release instead of the latest. |
| `--prefix /opt/domovoi` | Install the binary as `<prefix>/bin/domovoi`. Default `/usr/local`. |
| `--no-service` | Install only the binary and examples: no user, directories or unit. Works without root when the prefix is writable. |
| `--print-unit hub` | Print the systemd unit and exit. The other values are `agent`, `helper-socket`, `helper-service`, `runner-socket` and `runner-service`. |
| `--migrate-identity` | Move an existing root agent to the three-identities layout, after a printed plan and a typed confirmation. Agent role, root, needs a terminal. |
| `--rollback-identity` | Undo that migration from the saved record. |

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
| `domovoi-agent` and `domovoi-skill` users | agent | Two system users with no login shell. `domovoi-agent` is also in `systemd-journal` when that group exists. Neither is ever in `docker`. An account that already exists is checked, never changed. |
| `/var/lib/domovoi-agent` | agent | Mode 0700, owned by `domovoi-agent`: the agent's key, its settings and its spool. |
| `/var/lib/domovoi-packages` | agent | Mode 0755, owned by `domovoi-agent`: installed packages. |
| `/var/lib/domovoi-helper` | agent | Mode 0700, owned by root: the helper's lock, and later its key and state. The agent cannot read it. |
| `/etc/domovoi` | both | Mode 0750. On an agent host, group `domovoi-agent`. |
| `/etc/domovoi/skills` | agent | Mode 0755, owned by root. The starter skills are copied here. |
| `/etc/domovoi/helper.toml` | agent | Mode 0600, root. Written only when absent. See [Helper](../reference/helper.md). |
| `/etc/systemd/system/domovoi-hub.service` | hub | See [`deploy/systemd`](https://github.com/RoastSlav/domovoi/tree/main/deploy/systemd). |
| `/etc/systemd/system/domovoi-agent.service` | agent | |
| `/etc/systemd/system/domovoi-helper.socket` and `domovoi-helper@.service` | agent | The helper, started on demand. |
| `/etc/systemd/system/domovoi-skill-runner.socket` and `domovoi-skill-runner@.service` | agent | The skill runner, started on demand, one process per connection, as `domovoi-skill`. The installer writes your `skills_dir` and `packages_dir` into the service. |

Apart from the helper and skill runner sockets on a fresh agent install, the installer never enables or starts a service. Running it again replaces the binary and leaves everything else as it is; on an agent it also checks each path above and reports any wrong mode or owner without changing it (see [Troubleshooting](../troubleshooting.md#a-path-has-the-wrong-mode-or-owner)). A unit you changed is kept, and the new one is saved next to the examples so you can compare them. To change a unit, prefer a drop-in (`sudo systemctl edit domovoi-hub`), which upgrades never touch.

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

On a fresh install the agent runs as `domovoi-agent`, not root. Its unit keeps `/usr` and `/etc` read-only, gives it a private `/tmp`, and lets it write only its own state and the packages directory. An agent installed earlier still runs as root until you migrate it (see above). Do not change these units with `systemctl edit`: the self-test reports any drop-in as `unit_modified`, and it will block skills. See [Hardening](../guide/hardening.md).

The helper runs as root in its own sandbox, one process per connection. `domovoi helper serve` is started by the socket and is not meant to be run by hand.

The skill runner runs skills as `domovoi-skill`, in a sandbox that hides the agent's and helper's state, both sockets and everything in `/etc/domovoi` except the skills directory. It needs the binary at a path it can execute: the installer refuses a `--prefix` under `/home`, `/root`, `/run` or `/tmp` for an agent on a systemd host. It also refuses a `skills_dir` or `packages_dir` it cannot put in a unit (a relative path, a space, or a place under `/run`, `/tmp`, `/home`, `/root`, `/proc`, `/sys` or `/dev`) and names the problem. The installer creates every directory the sandbox names before it starts the socket. `domovoi skill-runner serve` is started by the socket and is not meant to be run by hand.

## One machine

To watch the machine the hub runs on, install both roles on it and enroll the agent like any other, with the hub at `wss://127.0.0.1:7421`. The two services stay separate. Because the hub and the agent then share `/etc/domovoi`, the agent stays on the old layout, runs as root and is labelled "privilege boundary not in effect".

The hub can instead run an agent for its own machine in the same process, with the `--with-local-agent` flag. [`deploy/systemd/single-box.conf`](https://github.com/RoastSlav/domovoi/blob/main/deploy/systemd/single-box.conf) is a drop-in that does this. The local agent then runs inside the hub's sandbox as the `domovoi` user, so SMART, journal checks and skills that switch users report errors, and the boundary is not in effect. It needs an agent config file, which the drop-in describes. Prefer two services unless you need a single process.

## Next

- [Upgrading and uninstalling](upgrading.md)
- [Advanced: config as code](config-as-code.md), to manage the hub from files instead of the web UI.
