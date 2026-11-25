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

The agent stops retrying, with an error that says what to do, when the hub answers `reject` with `revoked` or `unknown_agent` (enroll the agent again). When the hub answers `incompatible`, the agent stays up idle: it does not reconnect, logs hourly, and writes `link-status.json`, which `domovoi agent status` prints. Upgrade the agent, then restart it. Any other failure, including a hub presenting a different certificate, is retried.

## Checks and signals

- Each enabled check runs at its `[checks.<key>] interval`, or its default cadence. At most 4 run at once.
- A firing observation becomes a `firing` signal on every run; the hub folds repeats into one incident.
- A target that was firing and is now fine, or is no longer reported, gets one `resolved` signal.
- A check that fails to run changes nothing, so its incidents stay as they are.
- Digest facts go to the hub once an hour, the latest value per key.

## Config file and account

`agent.toml` is written with mode `0644` and owned by root. The agent runs as the unprivileged `domovoi-agent` account on the three-identities layout and reaches the file through the group of `/etc/domovoi` (mode 0750), so it must be readable by that group. An older `0640` file is normalised by the identity migration, and `domovoi agent selftest` reports it on a migrated host.

## Helper probe

The agent asks the root helper about itself at start and then every 60 seconds, and reports the answer to the hub as `helper_status`:

| Value | Meaning |
| --- | --- |
| `Absent` | No helper socket exists on this host. The agent works as before. |
| `Ok` | The helper answered `health` and `groups`. The value lists the enabled read groups, such as `docker.read`. |
| `Error` | The socket exists but the helper did not answer, answered with a refusal, or its start check failed. The reason is the token, such as `helper_state_permissions`, or `unreachable`. |

A helper that starts after the agent turns `Absent` into `Ok` within one probe interval. The probe sends nothing but `health` and `groups`. Nothing falls back to doing a privileged operation inside the agent when the helper is absent or in error.

## Skill runner probe

On an agent that does not run as root, skills run in a separate process, the skill runner, reached through `/run/domovoi-skill-runner.sock`. The agent asks it for `health` at start and then every 60 seconds, and reports the answer to the hub as `skill_runner_status`. It probes again at once after a transport error.

| Value | Meaning |
| --- | --- |
| `Absent` | No runner socket exists on this host. Skills are unavailable. |
| `Ok` | The runner answered, speaks the same protocol, and serves the same `skills_dir` and `packages_dir` as `agent.toml`. The value carries its version. |
| `Unsupported` | The runner refuses every request because its own start check failed. The reason is `runner_groups_extra` or `runner_privileges`. |
| `Error` | The runner is there but cannot be used. The reason is `runner_socket_permission`, `runner_protocol_mismatch`, `runner_roots_differ` or `runner_unreachable`. |

The reasons and what to do about each are in [Troubleshooting](../troubleshooting.md#skills-are-unavailable-the-skill-runner). The runner takes both directories from its unit, not from `agent.toml`, so a changed `skills_dir` or `packages_dir` shows up as `runner_roots_differ` until the installer renders the unit again.

An agent running as root does not probe the runner and runs skills itself, as before.

The agent also decides whether the privilege boundary is in effect when it starts. It is only when the agent runs as `domovoi-agent` with exactly its own group and `systemd-journal`. A root agent, a container agent and the single-process mode are labelled "not in effect", and package results say so.

## State directory

Everything lives under `[agent] state_dir` (default `/var/lib/domovoi-agent`, owned by `domovoi-agent` with mode 0700 on the three-identities layout). Every file is written to a temporary file and renamed, so a crash never leaves half a file. Files are mode `0600`.

| File | Contents |
| --- | --- |
| `identity.json` | Agent id, host id, the ed25519 signing key and the pinned hub fingerprint. |
| `checks.json` | Each check's own state (baselines, cursors, counters) and the targets last seen firing. |
| `spool.jsonl` | Signal batches the hub has not acknowledged, one per line. |
| `spool.json` | The next batch number and how many signals were dropped. |

## Spool

Every batch of signals is written to the spool before it is sent. The hub acknowledges batches with `signals_ack`, and acknowledged batches are removed. After a reconnect, every unacknowledged batch is sent again, oldest first. Batch numbers keep rising across restarts.

Check state and the memory of what was firing are saved only once the run's signals are on disk in the spool. If the spool cannot be written, nothing from that run is remembered: an error is logged and the run repeats at the next interval, producing the same signals again instead of losing them.

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

The approved skill then goes to the runner, which has the final say:

- The agent refuses a `run_as` other than `domovoi-skill` before it connects, and does not connect at all while the runner status is not `Ok`. The failure message starts with `skill_runner_unavailable` or `run_as_unsupported`.
- One request travels on one connection and carries the skill name, the validated arguments, the skill's implementation hash, and caps for time and output. It carries no path, environment or user.
- The runner finds the skill folder itself, recomputes the hash and refuses on a difference, validates the arguments again and runs the entry as `domovoi-skill`. It sends back one reply: the exit code or signal, both streams, and whether it was cut off or timed out. The agent redacts and caps the reply again.
- Closing the connection before the reply cancels the run: the runner stops the whole process group and exits. That is what happens when the agent restarts or the hub's deadline passes, and the outcome is then "result unknown".
- At most 8 skills run at once.

## Packages

The hub can send the agent a signed package: an archive of skills. The agent checks the signature, then installs or removes it. Installing a package never enables it or runs anything. Package skills are not added to the skill catalogue, and they cannot run yet; enabling them is a later step.

Packages are off unless `agent.toml` has a `[packages]` section. Without it the agent installs nothing and advertises no package capabilities. Listing installed packages works whenever the section is present.

| Setting | Default | Meaning |
| --- | --- | --- |
| `[agent] packages_dir` | `/var/lib/domovoi-packages` | Where installed packages live. |
| `[packages] allow_install` | `false` | Whether the hub may install or remove packages. |
| `[packages] revoked` | empty | Key ids that are refused. |
| `[[packages.trusted_keys]]` | empty | Keys that may sign packages: `id`, `public_key` (base64) and `label`. |

### Trust

- Only keys listed in `agent.toml` are trusted. The key `id` is the first 16 hex characters of the SHA-256 of the 32-byte ed25519 public key.
- The hub cannot add, replace or remove keys. Only the owner, editing `agent.toml`, can.
- A revoked key is refused even when it is also listed as trusted.
- Keys are read when the agent starts. Changing them needs a restart.
- A valid signature is not permission. Each install and removal also needs a person's approval in the hub. A policy approval is refused.
- A root agent installs with `privilege_boundary` set to `not_in_effect`, because the package is not confined to a lower-privilege user. `packages_dir` must be owned by the agent's user and not writable by group or others.

### Layout

```text
packages_dir/
  .packages.toml          registry, mode 0600
  .staging/               wiped at start
  <name>/<version>/       package files
```

- Directories are `0755`, files `0644`, and owner-executable files `0755`.
- The registry records, for each package, the signing key id, the archive digest and the time of install.
- Each package has one installed version.
- An archive is `<name>-<version>.tar.zst`. It has one top-level `<name>/` directory holding `package.toml` and `skills/<skill>/`. Links, paths that escape the package, and archives over the limits are refused.
- The signature is a detached `<name>-<version>.sig.toml`. It is an ed25519 signature over this text: `domovoi-package-v1`, the package name, the version, the archive SHA-256 and the key id, one per line.

### Limits

| Limit | Default |
| --- | --- |
| Compressed archive | 20 MiB |
| Uncompressed total | 100 MiB |
| One file | 20 MiB |
| Entries | 2000 |
| Path depth | 8 |
| Path length | 255 bytes |
| Extraction time | 60 s |

The limits are set under `[packages.limits]`.

### Crash recovery

The registry write is the commit point. An install is complete only when it is recorded there. At start-up the agent removes version directories the registry does not list, and wipes `.staging/`. Retrying an install after a crash gives the same result. A corrupt registry fences the packages until the owner repairs or removes it.

### Downgrade

Older agent binaries reject a config that has `[packages]` or `packages_dir`. Before downgrading, remove both from `agent.toml`. Installed packages stay on disk.

The wire messages are in [Agent protocol](protocol.md#package-transfers).
