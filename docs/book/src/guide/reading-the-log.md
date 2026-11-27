# Reading the hub and agent log

The hub and the agent write one line per notable event to standard error (the journal, under systemd; `docker logs`, in a container). Every line is `key=value` text, so you can search it. `--log-format json` writes the same fields as JSON, and `--log-level domovoi_hub=debug` turns up one part. See the [command line](../reference/cli.md) for the flags.

Each line should answer who did it, what it was trying to do, what happened and why. If one does not, that is a bug worth reporting.

## What starts a run

| Line | Meaning |
| --- | --- |
| `domovoi hub starting` | One line with `version`, `commit`, `config_file`, `data_dir`, `web_bind`, `agent_bind`, `public_url`, `tls_fingerprint`, `settings_revision`, `settings_from_file_or_env` and `user`. `settings_from_file_or_env` lists setting names only, as `key(file)` or `key(env)`; those cannot be edited in the web UI. Values are never printed. |
| `ready: listening for the web UI and for agents` | The sockets are open. |
| `domovoi agent starting` | The agent's build, host name and id, `hub_url`, `config_file`, `state_dir`, `user` and how many checks are on. |

## Requests and the request id

Every API response carries an `x-request-id` header. Every line the hub logs while serving that request shows the same id in front of the message, as `request{request_id=... method=POST path=/api/v1/...}`, and the request ends with one `request` line with `status`, `user` and `duration_ms`. When someone reports "it said internal error", ask for the id (browser developer tools, Network tab, the response headers) and search the log for it: the lines with that id are everything the hub did and logged for that click.

Successful requests are logged at `debug`, so the default log is not one line per page poll. Refusals (`401`, `403`, `429`) are `warn`. `400`, `404`, `409` and the expected `502` (a chat message that was not delivered) and `503` (no model is configured) are `info`. Only an unexpected `500` is `error`.

## Sign-in and approvals

| Line | Meaning |
| --- | --- |
| `failed login ip=... username="..." reason="wrong password"` | The name exists, the password did not match. |
| `failed login ... reason="unknown user"` | No account has that name. The browser sees the same answer in both cases. |
| `too many failed logins; ... locked out` | Written once, when the lockout starts, with `lockout_secs`. |
| `failed password confirmation ip=... username=...` | A change that needs your password was refused; the request line in front names the route. |
| `audit row recorded seq=... kind=... actor=... subject=...` | Every audit row (settings changes, rules, policy, packages, restores, approvals, skill changes, backups) leaves one pointer line. Look the row up by `seq` in the [audit log](audit.md). The details are not in the log, because they can hold values that must not be there. |

## Webhooks

`webhook request rejected hook="..." source=... status=... reason="..." suppressed=N` explains each refusal: `401` (no signature or bearer header, or it does not match), `413` (body too large), `400` (the body is not a valid payload of the configured kind), `404` (no hook with that name), `422` (no usable host), `429` (rate limit). One line per hook and status every 10 seconds; `suppressed` counts the repeats held back. The secret and the body are never printed.

## Agents

| Line | Meaning |
| --- | --- |
| `agent enrolled host=... host_id=... agent_id=...` | A new agent finished enrolling. `enrollment invite created` names the person who made the invite. |
| `agent connected host=... host_id=... session=... peer=... agent_version=...` | A session started. |
| `agent handshake failed peer=... host=... host_id=... agent_id=... reason="..."` | An agent was refused. The reason is a sentence: the enrollment token is unknown, expired or already used (the hub keeps only a hash of tokens and cannot tell these apart); the agent was revoked or replaced; the hub does not know the agent id; the signature did not verify; the protocol versions differ. `host` and `host_id` are `-` when the hub could not tell which host it was. |
| `agent disconnected after 3h12m` | The agent went away. Informational; `reason` says whether it closed cleanly or just vanished (stopped, killed, network lost). |
| `agent stopped answering after ...` | It missed three heartbeats in a row. |
| `agent connection failed after ...` / `agent session ended on a protocol error after ...` | A TLS failure or a message the hub could not accept. `reason` has the library text with its links removed. |
| `could not accept an agent connection ... suppressed=N` | The agent port could not take a connection. Written once, then once every 10 seconds with a count, and the hub pauses 0.1 to 1 second between failures. "Too many open files" means the open-file limit is too low. |
| `refused an agent connection: too many connections have not finished the handshake ... peer=... pending_from_peer=N limit_per_peer=8 limit_total=64 suppressed=N` | The agent port holds at most 64 connections (8 per address) that have not yet finished TLS and the handshake, and drops one that has not within 5 seconds. A scanner or a stuck client opened more; the connections are closed at once. One line per 10 seconds. It is only a problem if real agents are refused. |
| `the open-file limit is low soft=... hard=... recommended=4096` | Written once at start when the process may open fewer than 4096 files. Every connection is one descriptor; raise it with `LimitNOFILE=65536` (the shipped unit does) or `--ulimit nofile=65536:65536`. The banner also carries `open_files_soft_hard=`. |
| `refused a web connection: too many open connections ... peer=... open_from_peer=N limit_per_peer=128 limit_total=512 suppressed=N` | The web port holds at most 512 connections (128 per address) and closes one that has not sent a complete request header within 10 seconds, a keep-alive connection waiting for its next request included. Behind a reverse proxy every browser comes from the proxy's address, so the per-address limit applies to the proxy as a whole. One line per 10 seconds. |

On the agent side:

| Line | Meaning |
| --- | --- |
| `could not reach the hub hub=... attempt=N next_retry_in_secs=... next_retry_at=... suppressed=N reason="..."` | The first failure of an outage is logged at once. While it lasts, one line every 5 minutes with the number of attempts held back. |
| `connected to the hub session=... hub=... hub_version=... offline_for=...` | The link is up; `offline_for` says how long it was down and `failed_attempts` how many tries it took. |
| `the hub link dropped; reconnecting session=... uptime=... reason="..."` | The open session ended. When the hub closed it (a restart or upgrade) the line is `the hub link closed` at `info`. |

## Settings, chat and backups

| Line | Meaning |
| --- | --- |
| `settings saved revision=N actor=... keys=a,b restart=true` | Who changed which settings (names only, never values). Then either `the hub is back with the new settings revision=N`, or `the new settings could not be applied; the previous settings are restored` / `the hub did not start with the new settings; rolling back` with the reason. |
| `chat gateway stopped; chat on this platform is off until the hub is restarted, it does not retry` | `error`, once. The gateway gave up for good (usually a refused bot token or a missing intent). Fix it and restart the hub. The close code that Discord sent is not available to the hub; check the token and the Message Content intent. |
| `nightly backup written path=... size_bytes=...` | A backup succeeded. `nightly backup failed reason=...` is the failure, and it raises an advisory. |
| `a model's security verdict or wording was refused model_verdict=EXPECTED rule=... reason="..."` | The guard replaced what the model said about a security event. `rule` is `expected_is_a_persons_decision`, `verdict_needs_a_tool_result` or `claims_without_evidence`; `verdict_now` is what the incident shows instead. |

## The setup link

When no account exists, the hub prints the one-time setup link to standard output so the person at the terminal can open it. Under systemd the journal keeps standard output too, so the link stays in the journal after it is used (it stops working once the account exists or after 24 hours). The structured log records only the first eight characters of the token, as `token_prefix`, so you can tell which link a line is about. After setup, clear the journal entry if the host is shared (`journalctl --vacuum-time`), or start the hub once from a terminal.
