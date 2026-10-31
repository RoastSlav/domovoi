# Agent runtime

The agent runs on every guarded host. It keeps one outbound connection to the hub, runs the built-in checks, serves the hub's read tools and approved skills, and keeps signals on disk while the hub is out of reach. The wire format is in [Agent protocol](protocol.md).

## Trust

- **The hub's certificate is pinned.** The hub uses a self-signed certificate. At enrollment the agent records the SHA-256 of the certificate the hub presents, and from then on it accepts exactly that certificate. Names and certificate authorities play no part. The TLS handshake signature is still verified, so an impostor would also need the hub's private key.
- **Expected fingerprint.** When you pass the fingerprint printed by the hub to enrollment, the agent refuses any other certificate before the enrollment token leaves the host.
- **The agent proves its key on every connection.** It signs the hub's challenge together with the TLS channel binding of that connection (the RFC 9266 exporter, `EXPORTER-Channel-Binding`, as hex). That signature is useless on any other connection.
- **TLS 1.3 only.**

## Connection

| Behaviour | Value |
| --- | --- |
| Connect timeout (TCP, TLS and WebSocket upgrade) | 10 s |
| Handshake timeout | 30 s |
| Reconnect delay | 1 s, doubling to 60 s, each ±20 % jitter |
| Delay reset | after a connection that stayed up 60 s |
| Heartbeat | a `ping` every `heartbeat_seconds` from `welcome` (10 s if unset) |
| Dead connection | three `ping`s without a `pong` |

The agent stops retrying, with an error that says what to do, when the hub answers `reject` with `revoked` or `unknown_agent` (enroll the agent again) or `incompatible` (upgrade the older side). Any other failure, including a hub presenting a different certificate, is retried.

## Checks and signals

- Each enabled check runs at its `[checks.<key>] interval`, or its default cadence. At most 4 run at once.
- A firing observation becomes a `firing` signal on every run; the hub folds repeats into one incident.
- A target that was firing and is now fine, or is no longer reported, gets one `resolved` signal.
- A check that fails to run changes nothing, so its incidents stay as they are.
- Digest facts go to the hub once an hour, the latest value per key.

## State directory

Everything lives under `[agent] state_dir` (default `/var/lib/domovoi-agent`). Every file is written to a temporary file and renamed, so a crash never leaves half a file. Files are mode `0600`.

| File | Contents |
| --- | --- |
| `identity.json` | Agent id, host id, the ed25519 signing key and the pinned hub fingerprint. |
| `checks.json` | Each check's own state (baselines, cursors, counters) and the targets last seen firing. |
| `spool.jsonl` | Signal batches the hub has not acknowledged, one per line. |
| `spool.json` | The next batch number and how many signals were dropped. |

## Spool

Every batch of signals is written to the spool before it is sent. The hub acknowledges batches with `signals_ack`, and acknowledged batches are removed. After a reconnect, every unacknowledged batch is sent again, oldest first. Batch numbers keep rising across restarts.

The spool holds at most 10 000 batches or 8 MiB. When it is full:

1. The oldest firing signals with `info` severity are dropped first.
2. Then whole batches, oldest first.

Resolved signals are kept until whole batches go, since losing one would leave an incident open. Dropped signals are counted in the `spool.dropped` digest fact.

## Requests from the hub

Requests run beside the link, at most 4 at once. If 32 more are waiting, new ones are answered as busy. Each request stops at its deadline.

### Read tools

Tool output is redacted, then capped at 64 KiB.

| Tool | Source |
| --- | --- |
| `docker_ps`, `docker_logs`, `docker_inspect` | The Docker Engine API. Inspect output lists environment variable names only, never their values. |
| `docker_events` | The last 24 hours of container events kept by `container.expected_down`. |
| `host_overview` | Uptime, load, memory, swap and disks from `/proc` and `statvfs`. |
| `journal`, `smart_report` | `journalctl` and `smartctl` through the fixed command templates. |
| `listening_ports` | `/proc/1/net/*`, loopback excluded. |
| `run_read_skill` | A skill from the local catalogue whose risk is `read`; any other risk is refused. |

`incident_history` and `notes_search` are answered by the hub, so the agent replies `unknown_tool`.

### Skills

Before an approved skill reaches the runner, the agent checks, in order:

1. The skill is in its own catalogue (`[agent] skills_dir`); otherwise `not_found`.
2. Its risk is in `[policy] allow_risk`; otherwise `risk_not_allowed`.
3. The arguments pass the local manifest; otherwise `invalid_args`.
4. The plan hash recomputed from the local manifest, this host and the validated arguments equals the approved one; otherwise `plan_mismatch`. Editing a skill after approval therefore needs a new approval.

Logs name the skill and the step that refused it, never argument values.
