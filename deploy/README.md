# Deployment files

There are no supported deployment artifacts yet. The current pre-alpha binary does not
start a production hub or agent, and the repository does not publish release archives or
container images. Do not use a source checkout as a server overseer.

The first supported release will add:

- `systemd/`: hardened units for the hub and the agent.
- `compose/`: Docker Compose examples for the hub and for a container-mode agent with a Docker socket proxy.
- `unraid/`: Community Applications template for the agent.

Until then, [the development quick start](../docs/book/src/quick-start.md) is the supported
way to preview the UI with seeded data.
