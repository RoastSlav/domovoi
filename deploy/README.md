# Deploying Domovoi

Ready-made files for the ways Domovoi is installed. The install guides in the book
walk through each one; this is the index.

| Path | What it is | Guide |
| --- | --- | --- |
| [`../install.sh`](../install.sh) | Installs the release binary, the user, directories and systemd unit for a hub or an agent | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/domovoi-hub.service`](systemd/domovoi-hub.service) | The hub's unit, sandboxed, running as the `domovoi` user | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/domovoi-agent.service`](systemd/domovoi-agent.service) | The agent's unit, running as root | [Binary install](../docs/book/src/install/binary.md) |
| [`systemd/single-box.conf`](systemd/single-box.conf) | A drop-in that runs the hub and an agent for the same machine in one process | [Binary install](../docs/book/src/install/binary.md#one-machine) |
| [`compose/hub.yml`](compose/hub.yml) | The hub in Docker Compose | [Docker](../docs/book/src/install/docker.md) |
| [`compose/agent.yml`](compose/agent.yml) | The agent in Docker Compose, with a read-only Docker socket proxy | [Container agent](../docs/book/src/install/container-agent.md) |
| [`unraid/domovoi-agent.xml`](unraid/domovoi-agent.xml) | An Unraid template for the agent | [Container agent](../docs/book/src/install/container-agent.md#unraid) |
| [`test/installer.sh`](test/installer.sh) | CI's test of `install.sh` against a fake release | |

None of these needs a config file: the hub is set up from its web UI, and an agent is
set up by its enroll command. To manage settings as files instead, see
[config as code](../docs/book/src/install/config-as-code.md).
