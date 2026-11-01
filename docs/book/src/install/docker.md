# Docker

The hub image is `ghcr.io/roastslav/domovoi`, for `linux/amd64` and `linux/arm64`. It is a static binary on a distroless base, running as an unprivileged user. It needs no config file.

## Run it

```sh
docker volume create domovoi
docker run -d --name domovoi --restart unless-stopped \
  -p 7420:7420 -p 7421:7421 \
  -v domovoi:/var/lib/domovoi \
  ghcr.io/roastslav/domovoi:latest
docker logs domovoi 2>&1 | grep -A1 setup
```

Open the setup link, create the admin account, and follow the guided first run. The link points at `http://127.0.0.1:7420`; from another machine, replace that part with the Docker host's address.

- Port 7420 serves the web UI and the API. Put it behind a reverse proxy with TLS before you expose it beyond your network.
- Port 7421 is where agents connect. They pin the hub's certificate, so it needs no proxy.

## Compose

[`deploy/compose/hub.yml`](https://github.com/RoastSlav/domovoi/blob/main/deploy/compose/hub.yml) is the same setup for Compose:

```sh
curl -fsSLO https://raw.githubusercontent.com/RoastSlav/domovoi/main/deploy/compose/hub.yml
docker compose -f hub.yml up -d
docker compose -f hub.yml logs hub | grep -A1 setup
```

## The data volume

Everything the hub keeps is in `/var/lib/domovoi`:

- `hub.db`, the database, with automatic backups in `backups/` before each migration;
- `secret.key`, which encrypts the secrets you save in the web UI;
- `tls/`, the certificate agents have pinned.

Keep the volume when you upgrade or recreate the container. Without `secret.key`, saved secrets cannot be read and must be entered again. Without `tls/`, every agent has to enroll again. See [Backups](../reference/backup.md).

## Addresses

Until you set them, the setup link and chat links point at `http://127.0.0.1:7420`, and enroll commands at `wss://domovoi:7421`. Set the hub's web and agent addresses in the first run or on the Settings page. To pin them in the deployment instead, set `DOMOVOI_WEB__PUBLIC_URL` and `DOMOVOI_AGENT_SERVER__PUBLIC_URL`; the UI then shows them read-only.

The image sets `DOMOVOI_WEB__BIND=0.0.0.0:7420` and `DOMOVOI_AGENT_SERVER__BIND=0.0.0.0:7421`, so both listeners are reachable through the published ports.

## Watching the Docker host

The hub image has no agent in it. To watch the machine Docker runs on, run the [agent container](container-agent.md) next to it, or [install the agent](binary.md#install-an-agent) on the host.

## Building from source

The `Dockerfile` at the repository root builds the same image from source, with the web UI embedded:

```sh
docker build -t domovoi .
```

## Next

- [Upgrading and uninstalling](upgrading.md)
- [Advanced: config as code](config-as-code.md), to manage the hub from a mounted `hub.toml` or environment variables.
