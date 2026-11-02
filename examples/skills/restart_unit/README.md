# restart_unit

Restarts one systemd unit with `systemctl restart`. Thirty seconds later the agent
re-runs the check that raised the incident, and the action is marked verified or failed.

The agent must run as root, as the packaged unit does.

| Parameter | Values |
| --- | --- |
| `unit` | a unit name such as `nginx.service`; must start with a letter or digit |
