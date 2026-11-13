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

Before a new release changes the database, the hub copies `hub.db` into `backups/`. `secret.key` and the agent certificate in `tls/` carry over unchanged, so saved secrets keep working and agents stay enrolled.

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
sudo systemctl disable --now domovoi-agent
sudo rm /etc/systemd/system/domovoi-agent.service
sudo rm -r /var/lib/domovoi-agent
sudo systemctl daemon-reload
```

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
