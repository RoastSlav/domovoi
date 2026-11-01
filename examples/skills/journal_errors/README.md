# journal_errors

Shows up to 200 lines of priority `err` or worse that a systemd unit logged in the last
1 to 1440 minutes.

| Parameter | Values |
| --- | --- |
| `unit` | a unit name such as `nginx.service`; must start with a letter or digit |
| `minutes` | 1 to 1440 |
