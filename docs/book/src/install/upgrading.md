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

Upgrade the hub first, then the agents. A hub accepts agents on its own protocol version or one minor version behind, and the Hosts page marks agents that are older than the hub. An agent newer than the hub, or further behind, is refused, and says so in its log. See [the agent protocol](../reference/protocol.md).

Before a new release changes the database, the hub copies `hub.db` into `backups/`. `secret.key` and the agent certificate in `tls/` carry over unchanged, so saved secrets keep working and agents stay enrolled.

To go back a version, stop the hub, restore the backup taken before the upgrade (see [Backups](../reference/backup.md)), and install the older release with `--version`.

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
