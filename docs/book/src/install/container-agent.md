# Container-mode agent

On a machine where you cannot or would rather not install packages (Unraid, TrueNAS SCALE, Synology, or any host that runs everything in containers), run the agent as a container. The image is `ghcr.io/roastslav/domovoi-agent`, for `linux/amd64` and `linux/arm64`.

The agent sees the host through read-only mounts under `/host`, and sees Docker through a socket proxy that only allows reads. It never gets the Docker socket itself, which would amount to root on the host.

## With Compose

[`deploy/compose/agent.yml`](https://github.com/RoastSlav/domovoi/blob/main/deploy/compose/agent.yml) runs the agent and the socket proxy:

```sh
curl -fsSLO https://raw.githubusercontent.com/RoastSlav/domovoi/main/deploy/compose/agent.yml
```

In the hub's web UI, open **Hosts**, then **Add host**. Copy the enroll command it shows and run it through Compose, with `docker compose -f agent.yml run --rm agent` in front of it:

```sh
docker compose -f agent.yml run --rm agent agent enroll --hub wss://… --token … --fingerprint … --name …
```

That stores the agent's settings and key in its volume. Then start it:

```sh
docker compose -f agent.yml up -d
```

## What it can see

| Mount | Used for |
| --- | --- |
| `/proc` → `/host/proc`, and `pid: host` | load, memory and processes |
| `/sys` → `/host/sys` | devices and sensors |
| `/etc/os-release` → `/host/etc/os-release` | the OS name and version |
| each filesystem to watch → `/host/<same path>` | disk usage |
| the socket proxy | containers, their health and restarts |

`DOMOVOI_AGENT__HOST_ROOT=/host` tells the agent where the host is mounted.

Some checks need the host itself and do not work from a container:

- **systemd and the journal.** The image has no `systemctl` or `journalctl`, so those checks report "journalctl is not installed". Install the agent on the host if you need unit and log checks.
- **SMART.** The agent runs the `smartctl` that ships in the image, but the container also needs the disks. Either list them under `devices:` with the `SYS_RAWIO` and `SYS_ADMIN` capabilities, or run the container `privileged: true`, which gives it full access to the host. The commented lines in `agent.yml` show both.

## The socket proxy

[`tecnativa/docker-socket-proxy`](https://github.com/Tecnativa/docker-socket-proxy) sits between the agent and Docker. `agent.yml` allows `CONTAINERS`, `EVENTS`, `INFO` and `IMAGES`, which is everything the container checks read.

Container skills that restart or stop a container also need `POST: 1`. That lets anything that can reach the proxy change containers, so leave it off unless you use those skills, and keep the proxy on a network only the agent shares.

## Unraid

[`deploy/unraid/domovoi-agent.xml`](https://github.com/RoastSlav/domovoi/blob/main/deploy/unraid/domovoi-agent.xml) is a template with the same mounts and variables:

1. Copy it to `/boot/config/plugins/dockerMan/templates-user/` on the flash drive.
2. Install `tecnativa/docker-socket-proxy` from Community Applications, with `CONTAINERS`, `EVENTS`, `INFO` and `IMAGES` set to 1, and name it `docker-socket-proxy`. Put both containers on the same custom network.
3. Add the agent from **Docker**, **Add Container**, and pick the `domovoi-agent` template. Under the array path, it watches `/mnt/user`.
4. Before the first start, open the container's console and run the enroll command from **Add host**. Then restart the container.

## TrueNAS SCALE

Install it as a custom app from `agent.yml` (**Apps**, **Discover Apps**, **Install via YAML**), or as two custom apps with the same image, mounts and variables. Use a host path under your pool for `/var/lib/domovoi-agent`, so the enrollment survives app updates. Run the enroll command from the app's shell.

## Synology

In Container Manager, create a project from `agent.yml`. DSM keeps disk health to itself, so leave SMART off and rely on DSM's own alerts for the disks. Run the enroll command from the container's terminal.

## Next

- [Agents and enrollment](../guide/agents.md)
- [Advanced: config as code](config-as-code.md), to give the agent a mounted `agent.toml` instead.
