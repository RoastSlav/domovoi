# Upgrading and uninstalling

## Upgrading

**Binary installs.** Run the installer again with the same role. It replaces the binary and leaves the unit, the data and any config you added as they are. A hub that was running is restarted on the new version and waited for until `/readyz` answers; a stopped hub stays stopped, and an existing admin account is never touched. An agent is not restarted by the installer:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
sudo systemctl restart domovoi-agent   # on an agent host
```

Add `--dry-run` to see what a run would change. An older version than the installed one is refused (exit 10) unless you pass `--force-downgrade`. A hub database from before 0.1.0 cannot be upgraded; see [Binary and systemd](binary.md#a-hub-from-before-010).

Or let the binary replace itself. `domovoi upgrade` downloads the latest release for this machine, checks its SHA-256 against the `.sha256` file published next to it, and swaps the binary in place. It changes nothing if the check fails. It does not restart anything: it prints the `systemctl restart` commands for the units that exist on this machine.

```sh
sudo domovoi upgrade
sudo systemctl restart domovoi-hub
```

`--version v0.9.9` installs that release instead of the latest, and `--dry-run` prints what would be downloaded and replaced without changing anything. Unlike the installer it leaves the unit files, users and examples alone, and says so after the swap (it lists the Domovoi units it found). Use the installer when a release changes them. Run it with `sudo` when the binary is in `/usr/local/bin`. It works on Linux x86-64 and 64-bit ARM, the platforms with release builds.

**Unit files.** The installer owns the Domovoi unit files. When an installed unit differs from this release's text, a rerun replaces it, saves the old text under `<prefix>/share/domovoi/unit-backups/<unit>.<timestamp>`, prints the directive lines that changed, runs `systemctl daemon-reload` and restarts nothing but a socket whose unit file it replaced (`enable --now` would leave a listening socket on its old text). Restart `domovoi-agent` to use its new unit; the helper and the skill runner are started per connection, so new connections already use the new text. Changes of your own belong in a systemd drop-in (the self-test reports a drop-in as `unit_modified`). `--keep-units` keeps the installed files instead: the installer says so, saves the new text beside the examples, and `domovoi agent selftest` keeps reporting `unit_modified`. The self-test also checks the sandbox directives of the skill runner unit (`ProcSubset`, `ProtectProc`, `NoNewPrivileges`, `CapabilityBoundingSet`), so a unit from an older release does not pass.

**Starter skills.** A rerun replaces each starter skill that still equals the copy of the release installed before, and adds new ones. A skill you edited, or one that cannot be shown to be unedited, is kept and named in the output; move its folder away and run the installer again to take the new copy.

**An agent on the three-identities layout.** The new helper unit gives the helper the device access and capabilities that SMART, kernel-log and listening-socket reads need. Until the unit is replaced those three answer `command_failed`. An agent already on the new layout is checked on every rerun: the installer reports any path with a wrong mode or owner and changes nothing.

**The skill runner's unit.** Running the installer again re-renders the skill runner unit with the current `skills_dir` and `packages_dir` from `agent.toml` and the current sandbox settings. If you change either directory in `agent.toml` and do not run the installer, the unit still names the old ones: the agent reports `runner_roots_differ` and skills stop, and `domovoi agent selftest` reports `unit_modified` naming `bind_paths` (check ST-07). Run the installer again, then restart the agent. `domovoi upgrade` replaces only the binary, so new runner processes use the new version at once while a running agent keeps the old protocol until you restart it; the agent shows `runner_protocol_mismatch` in between.

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

**Settings**, **About and updates** shows the version this hub runs, when it last checked, and a **Check now** button. A manual check is allowed once a minute.

Turn the automatic check off in the same section, or in `hub.toml`:

```toml
[updates]
check = false
```

The environment variable `DOMOVOI_UPDATES__CHECK=false` does the same. Turning it off only stops the hub from checking on its own: **Check now** still works, and installing is always a command you run.

## Uninstalling

```sh
sudo sh install.sh --uninstall
```

This stops and disables the hub, agent, helper and skill runner units, deletes their unit files, `<prefix>/bin/domovoi` and `<prefix>/share/domovoi`, and lists what it kept: the configuration (`/etc/domovoi`), the data (`/var/lib/domovoi*`), the skills, the accounts and the identity-migration record. Use `--prefix` if you installed elsewhere, `--dry-run` to see the plan, `--json` for machine output and `--yes` to skip questions. It exits 10 when nothing is installed or it is not run as root.

To remove the data and accounts as well:

```sh
sudo sh install.sh --uninstall --purge
```

`--purge` also deletes `/etc/domovoi`, `/var/lib/domovoi`, `/var/lib/domovoi-agent`, `/var/lib/domovoi-packages`, `/var/lib/domovoi-helper`, the install log, the systemd drop-ins `/etc/systemd/system/domovoi-*.d`, and the `domovoi`, `domovoi-agent` and `domovoi-skill` accounts, but only accounts the installer created. This is destructive and cannot be undone: it lists what it will delete with the data sizes, then needs `--yes` or the word `purge` typed on the terminal. Keep a copy of `secret.key` and the database first if you may come back (see [Backups](../reference/backup.md)).

After removing an agent, revoke the host on the hub's **Hosts** page, so its key is no longer accepted.

Containers: `docker compose -f hub.yml down`, then `docker volume rm domovoi_domovoi` to delete the data as well.

## Distribution packages

There are no `.deb`, `.rpm` or Alpine packages. The installer and the images are the supported ways in.
