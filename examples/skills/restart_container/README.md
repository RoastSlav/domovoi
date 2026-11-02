# restart_container

Restarts one Docker container with `docker restart`. Thirty seconds later the agent
re-runs the check that raised the incident, and the action is marked verified or failed.

Needs the `docker` command and access to the Docker socket.

| Parameter | Values |
| --- | --- |
| `container` | a container name: letters, digits, `_`, `.` and `-`, starting with a letter or digit |
