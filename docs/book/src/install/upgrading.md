# Upgrading and uninstalling

## Upgrading

**Binary installs.** Run the installer again with the same role. It replaces the binary and leaves the unit, the data and any config you added as they are. Then restart the service:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
sudo systemctl restart domovoi-hub
```

Or let the binary replace itself. `domovoi upgrade` downloads the latest release for this machine, checks its SHA-256 against the `.sha256` file published next to it, and swaps the binary in place. It changes nothing if the check fails. It does not restart anything: it prints the `systemctl restart` commands for the units that exist on this machine.

```sh
sudo domovoi upgrade
sudo systemctl restart domovoi-hub
```

`--version v0.9.9` installs that release instead of the latest, and `--dry-run` prints what would be downloaded and replaced without changing anything. Unlike the installer it leaves the unit files, users and examples alone, so use the installer when a release changes them. Run it with `sudo` when the binary is in `/usr/local/bin`. It works on Linux x86-64 and 64-bit ARM, the platforms with release builds.

**An agent that still runs as root.** A routine rerun of the installer, or `domovoi upgrade`, never moves an agent to the three-identities layout. The agent keeps running as root, and the host is labelled "privilege boundary not in effect" in the installer's output, in `domovoi agent selftest` and on the hub. The move is a separate step you choose, with its own plan and a typed confirmation:

```sh
sudo sh install.sh --role agent --migrate-identity
```

See [Binary and systemd](binary.md#already-installed) for what it changes and how to roll it back. After it, Docker, SMART, the kernel-log fallback and listening-port process names keep working through the helper; `security.fail2ban` reports an error.

**A host that was already migrated.** The installer keeps a helper unit that differs from this release's and saves the new text under `<prefix>/share/domovoi/domovoi-helper@.service`. Replace the unit with it and run `sudo systemctl daemon-reload` (then `domovoi agent selftest`, which reports the old unit as `unit_modified` until you do): the new unit gives the helper the device access and capabilities that SMART, kernel-log and listening-socket reads need. Docker reads work without it. Until you replace it, those three answer `command_failed`. An agent already on the new layout is checked on every rerun: the installer reports any path with a wrong mode or owner and changes nothing.

**The skill runner's unit.** Running the installer again re-renders the skill runner unit with the current `skills_dir` and `packages_dir` from `agent.toml`. A unit you edited is kept and the new text is written beside it. If you change either directory in `agent.toml` and do not run the installer, the unit still names the old ones: the agent reports `runner_roots_differ` and skills stop, and `domovoi agent selftest` reports `unit_modified` naming `bind_paths` (check ST-07). Run the installer again, then restart the agent. `domovoi upgrade` replaces only the binary, so new runner processes use the new version at once while a running agent keeps the old protocol until you restart it; the agent shows `runner_protocol_mismatch` in between.

**Containers.** Pull the new image and recreate the container on the same volume:

```sh
docker compose -f hub.yml pull
docker compose -f hub.yml up -d
```

That pulls the `latest` image. To name the release instead, put its version in front of both commands, as the update banner's Docker command does:

```sh
DOMOVOI_IMAGE_TAG=0.9.9 docker compose -f hub.yml pull && DOMOVOI_IMAGE_TAG=0.9.9 docker compose -f hub.yml up -d
```

[`hub.yml`](https://github.com/RoastSlav/domovoi/blob/main/deploy/compose/hub.yml) reads that variable, and its default is `latest`.

Peers talk only when their protocol `major.minor` are equal, so a protocol change makes every agent that still runs the old version incompatible at once. Upgrade in this order:

1. Upgrade the hub.
2. Upgrade each agent through its supported install path.
3. Watch the **Hosts** page until no host is **Incompatible** or **Expected incompatible**.

An agent from this release that the hub rejects as incompatible does not exit and does not reconnect: it stays up idle, logs the reason hourly, and `domovoi agent status` shows the protocols it and the hub speak. Upgrade the agent, then restart it; that is the only thing that ends the state. Agents older than this release exit on the same reject and are restarted by systemd every 5 seconds until they are upgraded.

**Expected incompatible** means the hub was upgraded and an agent has not reconnected since. It will show as incompatible if it still speaks the old protocol, and as compatible once it reconnects. See [the agent protocol](../reference/protocol.md).

Before a release changes the database, the hub copies `hub.db` into `backups/` as `pre-migrate-<time>.db`. Only the three newest of those copies are kept (`backup.keep_pre_migrate`), and they are kept apart from the nightly copies, so a nightly run never pushes one out. A hub refuses to open a database that a newer release has written, and says which release it needs; install that release or restore an older backup. `secret.key` and the agent certificate in `tls/` carry over unchanged, so saved secrets keep working and agents stay enrolled.

To go back a version, stop the hub, restore the backup taken before the upgrade (see [Backups](../reference/backup.md)), and install the older release with `--version`.

## Release notices

Twice a day the hub asks GitHub for the newest Domovoi release. Nothing about your installation is sent, only the request itself. When the release is newer than the hub, every page shows a banner with the version, its release notes, and commands that install it, starting with `domovoi upgrade` on a binary install. The commands name the release's tag, so they can be copied as they are: the hub and agent commands pass it to the installer, and the container command pins the image to the release, which keeps it off the `latest` tag. The **Hosts** page links each outdated agent to the same commands. Update the hub first, then the agents.

**Settings → About** shows the version this hub runs, when it last checked, and a **Check now** button. A manual check is allowed once a minute.

Turn the automatic check off in **Settings → Updates**, or in `hub.toml`:

```toml
[updates]
check = false
```

The environment variable `DOMOVOI_UPDATES__CHECK=false` does the same. Turning it off only stops the hub from checking on its own: **Check now** still works, and installing is always a command you run.

## Uninstalling

An agent:

```sh
sudo systemctl disable --now domovoi-agent domovoi-helper.socket domovoi-skill-runner.socket
sudo rm /etc/systemd/system/domovoi-agent.service
sudo rm /etc/systemd/system/domovoi-helper.socket /etc/systemd/system/domovoi-helper@.service
sudo rm /etc/systemd/system/domovoi-skill-runner.socket /etc/systemd/system/domovoi-skill-runner@.service
sudo rm -r /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper
sudo systemctl daemon-reload
sudo userdel domovoi-agent
sudo userdel domovoi-skill
```

On a host that never had the three-identities layout, the helper, skill runner, packages and `userdel` lines have nothing to remove and can be skipped. The helper directory holds nothing but a lock in this release.

Then revoke the host on the hub's **Hosts** page, so its key is no longer accepted.

The hub:

```sh
sudo systemctl disable --now domovoi-hub
sudo rm /etc/systemd/system/domovoi-hub.service
sudo systemctl daemon-reload
sudo rm -r /var/lib/domovoi   # the database, secret.key and backups: keep a copy if you may come back
sudo userdel domovoi
```

Then, on a machine that has neither role left:

```sh
sudo rm -r /etc/domovoi /usr/local/bin/domovoi /usr/local/share/domovoi
```

Containers: `docker compose -f hub.yml down`, then `docker volume rm domovoi_domovoi` to delete the data as well.

## Distribution packages

There are no `.deb`, `.rpm` or Alpine packages yet; the installer and the images are the supported ways in. Packages are planned.
