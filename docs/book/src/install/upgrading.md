# Upgrading and uninstalling

## Upgrading

**Binary installs.** Run the installer again with the same role. It replaces the binary and leaves the unit, the data and any config you added as they are. Then restart the service:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
sudo systemctl restart domovoi-hub
```

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

Upgrade the hub first, then the agents. A hub accepts agents on its own protocol version or one minor version behind, and the Hosts page marks agents that are older than the hub. An agent newer than the hub, or further behind, is refused, and says so in its log. See [the agent protocol](../reference/protocol.md).

Each side also refuses a counterpart below its minimum release: the hub refuses an agent older than release 0.0.5, and an agent refuses a hub that says it runs a release older than 0.0.5. The Hosts page names why, for example `agent 0.0.1 is older than the minimum 0.0.5; update it`, and the hub logs the same sentence. A release that does not parse, as a development build's, is never refused. Upgrading either side to at least 0.0.5 clears the refusal; the Hosts page stops showing it as soon as the agent connects.

Before a new release changes the database, the hub copies `hub.db` into `backups/`. `secret.key` and the agent certificate in `tls/` carry over unchanged, so saved secrets keep working and agents stay enrolled.

To go back a version, stop the hub, restore the backup taken before the upgrade (see [Backups](../reference/backup.md)), and install the older release with `--version`.

## Release notices

Twice a day the hub asks GitHub for the newest Domovoi release. Nothing about your installation is sent, only the request itself. When the release is newer than the hub, every page shows a banner with the version, its release notes, and commands that install it. The commands name the release's tag, so they can be copied as they are: the hub and agent commands pass it to the installer, and the container command pins the image to the release, which keeps it off the `latest` tag. The **Hosts** page links each outdated agent to the same commands. Update the hub first, then the agents.

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
