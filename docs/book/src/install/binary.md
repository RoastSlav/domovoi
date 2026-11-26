# Binary and systemd

The installer puts the release binary on a Linux machine (x86_64 or arm64) and sets up systemd services for the hub, an agent, or both. It checks the machine first, and on a systemd host starts the hub and waits until it answers. It can also create the admin account and enroll an agent in the same run. For a script or an automation agent, see [Unattended install](unattended.md).

## Install the hub

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
```

The installer runs these steps: it checks the machine (preflight), downloads the archive, verifies its SHA-256, installs the binary, creates the user, directories and unit, then enables and starts the hub and waits until `GET /readyz` answers (60 seconds; `DOMOVOI_READY_TIMEOUT` changes it). It ends with the address to open and a single-use setup link (valid 24 hours) to create the admin account, and lists what is left for you: Discord, a model and the agent server URL. `--no-start` installs the unit and starts nothing. Without systemd the units are skipped and nothing is started.

By default the web UI listens only on loopback. To make it reachable, say so in the same command:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub \
  --bind lan --public-url http://192.168.1.50:7420
```

- `--bind lan` listens on `0.0.0.0:7420`, `--bind loopback` on `127.0.0.1:7420`, and `--bind HOST:PORT` on that address. With `lan` and no `--public-url`, the installer derives `http://<this machine's IPv4>:7420`.
- `--public-url URL` is the exact address people open, such as `https://domovoi.lan`. The hub accepts requests only from that address, so set it whenever you put a reverse proxy in front.
- `--agent-url wss://HOST:7421` is the address agents dial. With `--bind lan` it defaults to the host of the public URL.

These flags write only `DOMOVOI_WEB__BIND`, `DOMOVOI_WEB__PUBLIC_URL` and `DOMOVOI_AGENT_SERVER__PUBLIC_URL` into `/etc/domovoi/hub.env`. From another machine you can instead use an SSH tunnel (`ssh -L 7420:127.0.0.1:7420 <host>`) with the setup link unchanged.

To skip the setup link, give the installer the admin password. It creates the account `admin` before the first start, on a fresh install only; an existing admin or an upgrade is never changed:

```sh
sudo sh install.sh --role hub --admin-password-file /root/admin.pw
```

The file must be a regular file with mode 0600 or 0400, owned by you, and its first line is the password (at least 12 characters). `--admin-password-stdin` reads the first line of standard input instead (not with `curl | sh`, which uses standard input for the script).

Sign in. The guided first run then sets up the hub's name, time zone and public URL, Discord, a model, the daily digest and your first host. The address agents dial is **Agent server URL** under **Settings**, **Hub**. Every step except the first can be skipped and done later on the Settings page.

If the installer cannot reach the setup link or you started the hub by hand (`--no-start`), print the link from the journal:

```sh
sudo systemctl enable --now domovoi-hub
journalctl -u domovoi-hub -o cat | grep '/setup?token=' | tail -n 1
```

Restarting the hub replaces the setup token, so use the newest link.

To check the whole pipeline without network access or a model:

```sh
sudo -u domovoi domovoi selftest --offline
```

### Preflight, dry run and prompts

Every run starts with a preflight that prints one line per check: Linux, the architecture, root, the tools it needs, free disk space in the temp directory and the prefix (`DOMOVOI_MIN_FREE_MB`, default 200), systemd, the existing installation, a hub database from before 0.1.0 (see below) and an older version than the one installed (`--force-downgrade` allows it). If a check fails, the installer stops with exit 10 before it changes anything.

`--dry-run` runs the preflight, prints the plan (every file, user, unit and directory the install would create or change) and exits 0, without changing anything or downloading the archive. Use it to see what a command will do.

The installer asks no question except where a human must answer (`--purge` on a terminal). `--yes` or `DOMOVOI_NONINTERACTIVE=1` makes sure it never asks.

### Output, log and failures

`--quiet` prints errors and the final summary only, `--verbose` adds the details of each step, and `--json` prints JSON lines instead of text (see [Unattended install](unattended.md#the-json-output)). The full text of every run goes to `/var/log/domovoi-install.log`, or `~/.cache/domovoi-install.log` when that is not writable; `DOMOVOI_INSTALL_LOG` names another file. The log never holds the setup link or a token.

A fresh install that fails before the first service starts is undone: the accounts, directories, files, units and binary that this run created are removed again, newest first, and the installer prints "Rolled back". Nothing that was there before is touched. `--keep-on-failure` leaves a failed install in place for debugging. An upgrade and a failure after a service was started are never rolled back; the installer says what to run instead.

### A hub from before 0.1.0

0.1.0 replaces the database schema, so it cannot open a hub database written by an older release. The installer refuses (exit 10) until you pass `--accept-data-reset`. Even then it never moves or deletes the old data: it does not restart the hub, and prints how to move the data directory aside and set the hub up again.

## Install an agent

On each machine to watch:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role agent
```

On a systemd host with no hub on it, a fresh agent install sets up three identities: the unprivileged `domovoi-agent` account that runs the agent, the `domovoi-skill` account that skills will run as, and a root helper behind a socket. It creates the accounts, the directories with their modes, `/etc/domovoi/helper.toml`, the helper units and the skill runner units, starts the helper socket and then runs `domovoi agent selftest --installer --record`. Only if that passes does it start the skill runner socket and run the self-test again with the runner up. If a check fails, it stops the sockets it started, prints the report and exits with an error; a failed second check leaves the helper socket running and the runner socket stopped, so skills stay unavailable. [Security model](../security.md#three-identities-on-one-host) says what each identity may reach.

### Enroll in the same run

In the hub's web UI, open **Hosts**, then **Add host**, or run `domovoi hub token create <host>` on the hub. Either gives you a single-use token (valid 15 minutes), the hub certificate's fingerprint and the agent server address. Put the token in a file with mode 0600 and run the installer with all three:

```sh
sudo sh install.sh --role agent --hub wss://hub.lan:7421 \
  --fingerprint 3F:A9:... --token-file /root/enroll.token --name nas
```

The installer enrolls the agent, enables and starts it, and waits until `domovoi agent status` shows the link connected (30 seconds; `DOMOVOI_AGENT_TIMEOUT` changes it). The token can instead come from the environment variable `DOMOVOI_ENROLL_TOKEN`. A flag value is refused (`--token VALUE`, exit 2), because it would land in the shell history and the process list. `--hub`, `--fingerprint` and a token go together or not at all. `--name` is the host's name in Domovoi: lower-case letters, digits and dashes, by default this machine's short host name.

### Enroll by hand

Without those flags the installer installs the agent and starts nothing. Run the enroll command from **Add host** on that machine, with `sudo`. Enroll runs as root, so it hands the agent's files to `domovoi-agent` when it is done and prints one line saying so. Then start the agent:

```sh
sudo systemctl enable --now domovoi-agent
```

The host shows as connected within a few seconds. The agent dials the hub on port 7421; nothing connects to the agent.

The helper and the skill runner start on demand when the agent connects to their sockets, so you never enable them by hand. The helper reads Docker, SMART, the kernel log and listening sockets for the agent, so those checks and tools work on a fresh install. It has no write operation, so skills that need privileged access (restarting a container or a unit, vacuuming the journal) report errors. See [What a skill can reach](../guide/skills.md#what-a-skill-can-reach).

### Where the boundary does not apply

These hosts run the agent as root, labelled "privilege boundary not in effect", and the installer does not touch their layout:

- no systemd on the host;
- a hub on the same host: both roles share `/etc/domovoi`, which the hub makes `root:domovoi` and the agent layout needs as `root:domovoi-agent`. Run the agent on its own host to get the boundary;
- container installs and the single-process mode.

A host in this state gets no unattended execution.

## Options

| Option | Meaning |
| --- | --- |
| `--role hub`, `agent` or `both` | What this machine runs. Required. `both` sets up the hub and an agent that watches this machine. |
| `--version v0.1.0` | Install this release instead of the latest. |
| `--prefix /opt/domovoi` | Install the binary as `<prefix>/bin/domovoi`. An absolute path. Default `/usr/local`. |
| `--no-service` | Install only the binary and examples: no user, directories or unit. Works without root when the prefix is writable. |
| `--no-start` | Hub only: install the unit but do not start it. |
| `--public-url`, `--bind`, `--agent-url` | Hub: the addresses people and agents use. Written to `/etc/domovoi/hub.env`. |
| `--admin-password-file FILE`, `--admin-password-stdin` | Hub, fresh install: create the admin account from the first line of the file or of standard input. |
| `--hub`, `--fingerprint`, `--token-file`, `--name` | Agent: enroll, start and wait in the same run. The token can also be in `DOMOVOI_ENROLL_TOKEN`. |
| `--accept-data-reset` | Hub: go on although the database is from before 0.1.0. |
| `--dry-run` | Run the preflight, print the plan and change nothing. |
| `--yes` | Never ask a question. Same as `DOMOVOI_NONINTERACTIVE=1`. |
| `--json`, `--quiet`, `--verbose` | Machine-readable, minimal or detailed output. |
| `--force-downgrade` | Allow installing an older version than the one found. |
| `--keep-on-failure` | Do not undo a failed fresh install. |
| `--uninstall`, `--purge` | Remove the installation. See [Upgrading and uninstalling](upgrading.md#uninstalling). |
| `--print-unit hub` | Print the systemd unit and exit. The other values are `agent`, `helper-socket`, `helper-service`, `runner-socket`, `runner-service` and `agent-root` (the root agent unit used where the boundary cannot be set up). |

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

The installer enables and starts the hub, and an agent it enrolled in the same run, and the helper and skill runner sockets on a fresh agent install; it starts nothing else. Running it again replaces the binary, restarts a hub that was running, and leaves everything else as it is; on an agent it also checks each path above and reports any wrong mode or owner without changing it (see [Troubleshooting](../troubleshooting.md#a-path-has-the-wrong-mode-or-owner)). A unit you changed is kept, and the new one is saved next to the examples so you can compare them. To change a unit, prefer a drop-in (`sudo systemctl edit domovoi-hub`), which upgrades never touch.

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

On a fresh install the agent runs as `domovoi-agent`, not root. Its unit keeps `/usr` and `/etc` read-only, gives it a private `/tmp`, and lets it write only its own state and the packages directory. Do not change these units with `systemctl edit`: the self-test reports any drop-in as `unit_modified`, and it will block skills. See [Hardening](../guide/hardening.md).

The helper runs as root in its own sandbox, one process per connection. `domovoi helper serve` is started by the socket and is not meant to be run by hand.

The skill runner runs skills as `domovoi-skill`, in a sandbox that hides the agent's and helper's state, both sockets and everything in `/etc/domovoi` except the skills directory. It needs the binary at a path it can execute: the installer refuses a `--prefix` under `/home`, `/root`, `/run` or `/tmp` for an agent on a systemd host. It also refuses a `skills_dir` or `packages_dir` it cannot put in a unit (a relative path, a space, or a place under `/run`, `/tmp`, `/home`, `/root`, `/proc`, `/sys` or `/dev`) and names the problem. The installer creates every directory the sandbox names before it starts the socket. `domovoi skill-runner serve` is started by the socket and is not meant to be run by hand.

## One machine

To watch the machine the hub runs on, use one command:

```sh
sudo sh install.sh --role both --admin-password-file /root/admin.pw
```

`--role both` installs the hub exactly as `--role hub` does and starts it, which creates its TLS certificate. It then stops the hub for a moment to make a 15-minute enrollment token with `domovoi hub token create` as the hub user (the command needs the database), starts the hub again, and enrolls and starts an agent that dials `wss://127.0.0.1:7421` (`--agent-url` changes it). The token lives in a private temporary file for the length of the run and is never printed or logged. Every hub flag applies, `--name` names the host, and `--no-start` is refused.

The two services stay separate. Because the hub and the agent then share `/etc/domovoi`, the agent runs as root and is labelled "privilege boundary not in effect".

The hub can instead run an agent for its own machine in the same process, with the `--with-local-agent` flag. [`deploy/systemd/single-box.conf`](https://github.com/RoastSlav/domovoi/blob/main/deploy/systemd/single-box.conf) is a drop-in that does this. The local agent then runs inside the hub's sandbox as the `domovoi` user, so SMART, journal checks and skills that switch users report errors, and the boundary is not in effect. It needs an agent config file, which the drop-in describes. Prefer two services unless you need a single process.

The in-hub agent is the agent of the hub's own host, so that host is named by `[hub] name`. The `host_name` in the agent config is ignored, and the hub logs a warning at start when the two differ.

## Next

- [Unattended install](unattended.md), for scripts and automation agents
- [Upgrading and uninstalling](upgrading.md)
- [Advanced: config as code](config-as-code.md), to manage the hub from files instead of the web UI.

## Commands on an installed agent

The files under `/etc/domovoi` are readable by root and the agent account only, so a normal user gets "Permission denied" without `sudo`. Name the config file every time:

```sh
sudo -u domovoi-agent domovoi agent selftest -c /etc/domovoi/agent.toml
sudo -u domovoi-agent domovoi agent status -c /etc/domovoi/agent.toml
sudo -u domovoi-agent domovoi check run -c /etc/domovoi/agent.toml --only disk.free
sudo -u domovoi-agent domovoi check skills /etc/domovoi/skills
```

Run the self-test as `domovoi-agent`, not as root. As root it reports "tier: blocked" because the agent must not run as root; that verdict describes the account you ran it as, not the install.
