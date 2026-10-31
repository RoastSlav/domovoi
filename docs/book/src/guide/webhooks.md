# Webhooks

Alertmanager, Uptime Kuma, cron jobs and other tools can turn their notifications into Domovoi signals. Send them directly to the hub, or to an agent's local relay when the service cannot reach the hub.

Every request must authenticate. Bodies are limited to 1 MiB and 100 signals; evidence is limited to 32 entries per signal and 1024 characters per text value.

## Hub configuration

Each `[webhooks.<name>]` table creates `POST /hooks/<name>` on the hub's web listener. Names use lowercase letters, digits, `-` and `_`.

```toml
[webhooks.alerts]
kind = "alertmanager"       # "generic", "alertmanager" or "uptime_kuma"
auth = "bearer"             # "bearer" or "hmac"
secret = { env = "DOMOVOI_ALERTS_TOKEN" }
default_host = "nas"        # used when the payload names no host
host_label = "node"         # optional Alertmanager label to use instead
```

For Alertmanager, the host comes from `host_label` when configured. Otherwise Domovoi tries the `host` label, then `instance` with its port removed, then `default_host`. A valid name not already known to the hub becomes an `External` host. A payload with an invalid host name, or no usable host and no `default_host`, is rejected.

Keep the secret in an environment variable or a file, not in `hub.toml`. Check the file with `domovoi check config hub.toml` after editing it.

## HMAC or bearer

With `auth = "hmac"`, sign the exact request body using HMAC-SHA256 and send the lowercase hexadecimal digest in this header:

```text
X-Hub-Signature-256: sha256=<hex digest>
```

With `auth = "bearer"`, send:

```text
Authorization: Bearer <token>
```

HMAC works well for scripts that can compute a signature. Bearer mode suits senders such as Alertmanager that support authorization headers but not request signing. Use HTTPS or a trusted reverse proxy when webhooks cross a network.

## Alertmanager

Alertmanager supports bearer authentication directly:

```yaml
receivers:
  - name: domovoi
    webhook_configs:
      - url: "https://domovoi.example.net/hooks/alerts"
        http_config:
          authorization:
            type: Bearer
            credentials_file: /etc/alertmanager/domovoi-token
```

Each alert becomes one signal. Its check is `alertmanager.<alertname>`, its target is `instance` or `job`, and its severity comes from the `severity` label. The summary annotation is preferred, then description, then the alert name. Firing and resolved alerts use `startsAt` and `endsAt` respectively.

## Uptime Kuma

Create a **Webhook** notification in Uptime Kuma. Set the URL to the hub endpoint, for example `https://domovoi.example.net/hooks/kuma`, and add this additional header:

```json
{"Authorization": "Bearer your-token"}
```

The corresponding webhook needs `kind = "uptime_kuma"` and normally a `default_host`, because Kuma payloads do not name a Domovoi host. Down and up heartbeats become firing and resolved signals with check `uptime_kuma.monitor`; the monitor name is the target. Pending and maintenance heartbeats are ignored.

## Generic JSON

Use `kind = "generic"` for scripts and other integrations. Send one object or an array of objects:

```json
{
  "host": "nas",
  "check": "backup.nightly",
  "target": "photos",
  "status": "firing",
  "severity": "warning",
  "summary": "Backup failed",
  "evidence": {
    "exit_code": 2,
    "destination": "s3"
  },
  "observed_at": "2026-10-31T11:30:00Z"
}
```

`check` and `summary` are required. `status` defaults to `firing`, and `severity` defaults to `warning`; allowed severities are `info`, `warning` and `critical`. Evidence values must be strings, numbers or booleans. Unknown fields are rejected so misspellings do not silently change an alert.

## Testing with curl

For HMAC, sign the exact bytes that curl sends:

```sh
SECRET='your-secret'
BODY='{"host":"nas","check":"backup.nightly","summary":"Backup failed"}'
SIGNATURE=$(printf '%s' "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -hex | cut -d' ' -f2)
curl -i -X POST 'http://127.0.0.1:7420/hooks/alerts' \
  -H "X-Hub-Signature-256: sha256=$SIGNATURE" \
  -H 'Content-Type: application/json' \
  --data-binary "$BODY"
```

For bearer authentication, replace the signature header with `-H "Authorization: Bearer $SECRET"`. An accepted request returns `202`; inspect the resulting signal and incident in Domovoi.

## Agent relay

Services on a guarded host can post to the agent instead. The agent spools their signals and sends them over its existing authenticated connection, so those services need no network route or credentials for the hub.

```toml
[local_webhooks]
bind = "127.0.0.1:7422"
token = { env = "DOMOVOI_LOCAL_WEBHOOK_TOKEN" } # optional, shared by all three paths
allow_non_loopback = false
```

The relay has exactly three endpoints:

- `POST /hooks/generic`
- `POST /hooks/alertmanager`
- `POST /hooks/uptime_kuma`

There are no named relay webhooks and no host lookup. Every signal is pinned to the agent's own host. If the payload names another host, that name is retained as `reported_host` evidence rather than changing the signal's host.

The bind must be loopback unless `allow_non_loopback = true`. Keep the default unless remote senders explicitly need this listener, and configure a token when enabling it. Unlike hub webhooks, the relay accepts unauthenticated requests only when no token is configured.

## Status codes

| Status | Meaning |
| --- | --- |
| `202` | The payload was accepted. Ignored Kuma heartbeats also return this. |
| `400` | The body is not valid JSON for the configured format. |
| `401` | Authentication is missing or invalid; the response body is empty. |
| `404` | The hub webhook name or relay path does not exist. |
| `413` | The body exceeds 1 MiB. |
| `422` | No valid host can be resolved on the hub. |
| `429` | The hub webhook exceeded 60 requests per minute. Failed authentication also consumes tokens. |
| `503` | The hub signal sink or agent spool is unavailable. Retry later. |
