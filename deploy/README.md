# Deploying Domovoi

Ready-made files for the ways Domovoi is installed. The install guides in the book
walk through each one; this is the index.

| Path | What it is | Guide |
| --- | --- | --- |
| [`../install.sh`](../install.sh) | Installs the release binary, the users, directories and systemd units for a hub or an agent. An agent on a systemd host with no hub gets three identities (`domovoi-agent`, `domovoi-skill` and the root helper); `--migrate-identity` moves an agent that runs as root | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/domovoi-hub.service`](systemd/domovoi-hub.service) | The hub's unit, sandboxed, running as the `domovoi` user | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/domovoi-agent.service`](systemd/domovoi-agent.service) | The agent's unit, running as the unprivileged `domovoi-agent` account | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/domovoi-helper.socket`](systemd/domovoi-helper.socket) | The helper's socket (`/run/domovoi-helper.sock`, `root:domovoi-agent`, mode 0660), one helper process per connection | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/domovoi-helper@.service`](systemd/domovoi-helper@.service) | The root helper's sandboxed template unit, started once per connection by the socket | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/single-box.conf`](systemd/single-box.conf) | A drop-in that runs the hub and an agent for the same machine in one process | [Binary install](../docs/book/src/install/binary.md#one-machine) |
| [`compose/hub.yml`](compose/hub.yml) | The hub in Docker Compose | [Docker](../docs/book/src/install/docker.md) |
| [`compose/agent.yml`](compose/agent.yml) | The agent in Docker Compose, with a read-only Docker socket proxy | [Container agent](../docs/book/src/install/container-agent.md) |
| [`unraid/domovoi-agent.xml`](unraid/domovoi-agent.xml) | An Unraid template for the agent | [Container agent](../docs/book/src/install/container-agent.md#unraid) |
| [`test/installer.sh`](test/installer.sh) | CI's test of `install.sh` against a fake release, in a container without systemd | |
| [`test/identities.sh`](test/identities.sh) | The real-account test of the three identities, the helper unit, the helper's read operations (SMART, kernel log, listening sockets, Docker), the self-test and the migration; needs a disposable root machine with systemd as PID 1 | [Privileged identity test](../TESTING.md#privileged-identity-test) |

None of these needs a config file: the hub is set up from its web UI, and an agent is
set up by its enroll command. The one file `install.sh` writes is the helper's default
`/etc/domovoi/helper.toml`, only when it is absent. To manage settings as files instead, see
[config as code](../docs/book/src/install/config-as-code.md).
