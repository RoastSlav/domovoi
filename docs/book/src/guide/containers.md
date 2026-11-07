# Docker and Podman

The agent reads containers through the Docker Engine API for four checks (`container.expected_down`, `container.unhealthy`, `container.restart_loop`, `container.new`) and the container read tools. Docker, rootless Docker, Podman and rootless Podman all answer that API.

## What happens by default

Every time the agent starts it looks for an engine on these sockets, in order, and uses the first that accepts a connection:

1. `/var/run/docker.sock`
2. `$XDG_RUNTIME_DIR/docker.sock` (rootless Docker)
3. `/run/podman/podman.sock`
4. `$XDG_RUNTIME_DIR/podman/podman.sock` (rootless Podman)

Nothing is remembered between starts. If you install an engine later, restart the agent.

The host's page in the web UI, on the **Checks** tab, says what was found and shows a **Containers** switch:

- **Automatic** uses the engine when one was found.
- **Always on** needs a found engine.
- **Off** stops the four checks and makes the container tools answer that the runtime is switched off for the host.

The choice is sent to the agent at once, or when it reconnects.

## Fixing it in `agent.toml`

```toml
[docker]
enabled = true      # or false, or "auto" (the default)
endpoint = "unix:///run/podman/podman.sock"   # optional
```

`true` and `false` are the host owner's word: the web UI shows them as fixed and cannot change them. `endpoint` takes a `unix://` socket or a `tcp://host:port` address, such as a socket proxy. It also works for other engines that answer the Docker Engine API.

## Podman needs its socket

Podman only answers the API while its socket is running:

```sh
sudo systemctl enable --now podman.socket
```

For rootless Podman, run it as the user that runs the containers:

```sh
systemctl --user enable --now podman.socket
```

## The agent must be allowed to use the socket

Access to an engine's socket is root on the host. The agent only reads, and removes environment values from what it sends, but its user still needs access. If the socket exists and the agent's user may not use it, the agent says so in its log and the host page names the socket. Add the agent's user to the group that owns the socket, then restart the agent.

To keep the agent away from the socket itself, run the container agent behind a read-only socket proxy and set `endpoint`: see [Container agent](../install/container-agent.md).

## Turning it off for good

Set `enabled = false`. The agent then looks for nothing and the web UI cannot turn it on.
