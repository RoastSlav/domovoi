# disk_usage_top

Shows the 20 largest directories directly under `/var/log`, `/var/lib/docker` or
`/home`, without crossing into other filesystems. Useful for a `disk.free` incident.

| Parameter | Values |
| --- | --- |
| `path` | `/var/log`, `/var/lib/docker` or `/home` |
