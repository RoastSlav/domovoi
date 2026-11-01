# Docker

Coming in v0.1: running the hub from `ghcr.io/RoastSlav/domovoi` with Docker or Compose.

## After starting the container

The hub needs no config file. It prints a setup link once in its log (`docker logs <container>`). Open it, create the admin account, and follow the guided first run. Settings live in the data volume: keep `/var/lib/domovoi` on a volume, including its `secret.key`.

To manage the hub from files or environment variables instead, see [Advanced: config as code](config-as-code.md).
