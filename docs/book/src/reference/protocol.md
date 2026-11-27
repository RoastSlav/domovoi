# Agent protocol

Hub and agents talk over a single WebSocket connection that the agent opens to the hub's agent server (default port 7421). The connection uses TLS with the hub's self-signed certificate, which the agent pins at enrollment. Every frame is one JSON text message.

The protocol is a public contract. The JSON below is pinned by snapshot tests: any change that is not purely additive needs a new major version.

## Envelope

```json
{"v": 1, "id": "<uuidv7>", "kind": "<kind>", "reply_to": "<uuidv7>", "payload": {}}
```

| Field | Meaning |
| --- | --- |
| `v` | The sender's protocol major version. A frame with a different major is refused. |
| `id` | A fresh UUIDv7 for every frame. |
| `kind` | Selects the payload schema; see the table below. |
| `reply_to` | Present only on responses: the `id` of the request answered. `pong`, `enrolled`, `challenge`, `welcome`, `reject`, `signals_ack` and every `*_result` set it. |
| `payload` | The message body. |

Unknown fields in the envelope or a payload are ignored, so newer peers can add fields. A frame with an unknown `kind` is not an error: the receiver replies `unsupported` with that kind. One exception: the `call` object in `run_tool` is checked strictly, because the agent re-applies the same argument bounds as the hub, so an unknown or out-of-range argument is rejected.

## Versioning

The current version is **1.2**. Peers talk only when their protocol `major.minor` are equal: patch releases never matter, and there is no backward or forward window. An agent whose reported version differs in either number is refused with `reject` and the reason `incompatible`.

The envelope's `v` is the sender's protocol major. A frame whose `v` is not this build's major is not decoded: the receiver refuses it as an unsupported major before reading the payload, so that peer is never authenticated, never recorded, and gets no reject frame. A `hello` payload can still claim another major or minor inside a decodable frame; that claim is unauthenticated until the challenge signature verifies, and only then is the exact rule applied.

The hub states the rule only once the agent's signature has verified, so a client that does not hold the agent's key cannot leave anything on the Hosts page. It answers `incompatible`. An agent from this release treats that as an idle state rather than a final one: it stays up without reconnecting, logs hourly, and `domovoi agent status` shows what happened until the agent is upgraded and restarted. Agents older than this release exit and are restarted by systemd until they are upgraded.

There are no release-number floors. The wire variant `agent_too_old` can be decoded but the hub never sends it.

## Limits

- A frame is at most **1 MiB** (1,048,576 bytes). Larger frames are refused unread, and senders refuse to produce them.
- Every request carries a `deadline` (RFC 3339, UTC). The hub stops waiting after it.

## Encodings

- Timestamps are RFC 3339 in UTC, such as `2023-11-14T22:13:20Z`.
- Ids are UUIDv7 strings.
- Public keys, nonces and signatures are standard padded base64 of exactly 32, 32 and 64 bytes.
- Digests (`hub_cert_sha256`, `argv_hash`, `plan_hash`, `manifest_hash`) are 64 lowercase hex characters.
- Results use `{"ok": {..}}` or `{"err": {"code": .., "message": ..}}`. A code a peer does not know decodes as `unknown`.

## Enrollment

1. On the hub, `domovoi hub token create <host>` prints a single-use token, valid for 15 minutes unless `--ttl` says otherwise.
2. On the host, `domovoi agent enroll --hub <url> --token <token> --fingerprint <sha256>` generates an ed25519 key pair and sends `enroll` with the token and the public key.
3. The hub replies `enrolled` with the agent id, host id and its certificate fingerprint, or `reject` with `bad_token`. The agent checks that the fingerprint matches the certificate it saw on this connection, then pins it.

## Handshake

1. The agent connects and sends `hello`.
2. The hub replies `challenge` with a random nonce, or `reject` (`unknown_agent`, `revoked`) before any challenge.
3. The agent signs the challenge message and sends `challenge_response`.
4. The hub verifies the signature against the enrolled public key and replies `welcome`, or `reject` with `bad_signature`. Only after the signature verifies does it record what the hello said and apply the exact protocol rule: a differing major or minor is answered with `incompatible`. There is no release check.

After `welcome` either side may send any message.

### Challenge message

The agent signs these bytes, concatenated:

| Part | Bytes |
| --- | --- |
| `domovoi-agent-auth-v1` followed by a zero byte | 22 |
| The nonce from `challenge` | 32 |
| The agent id (UUID bytes, big-endian) | 16 |
| The session hint, UTF-8 | rest |

The session hint binds the signature to the connection it was made on. Over TLS it is the connection's RFC 9266 channel binding (the `EXPORTER-Channel-Binding` exporter, 32 bytes, lowercase hex), which the hub computes on its side.

## Message kinds

| Kind | Direction | Purpose |
| --- | --- | --- |
| `enroll` | agent → hub | Exchange a single-use enrollment token and a new public key for an identity. Sent once, on the first connection. |
| `enrolled` | hub → agent | The new agent and host ids, and the hub certificate fingerprint the agent pins. |
| `hello` | agent → hub | First frame of every later connection: agent id, protocol version, host facts, capabilities and skill catalog. |
| `challenge` | hub → agent | 32 random bytes the agent must sign. |
| `challenge_response` | agent → hub | The ed25519 signature over the challenge message. |
| `welcome` | hub → agent | The session is open: session id, hub version and hub-managed settings. |
| `reject` | hub → agent | The connection is refused; the hub closes it. |
| `ping` | both | Liveness probe. |
| `pong` | both | Reply to `ping`. |
| `signals` | agent → hub | A numbered batch of signals from checks or relayed local webhooks. |
| `signals_ack` | hub → agent | Every batch up to `up_to_seq` is stored; the agent drops them from its spool. |
| `digest_facts` | agent → hub | Facts for the daily digest. |
| `run_tool` | hub → agent | Run one read tool with typed, bounded arguments before a deadline. |
| `tool_result` | agent → hub | The tool's redacted output, or why it failed. |
| `run_skill` | hub → agent | Run an approved step's skill: execution id, skill, args, plan hash, deadline. |
| `skill_result` | agent → hub | Exit status, redacted and capped output, duration and argv hash, or why it did not run. |
| `run_capability` | hub → agent | Run one capability contract on the host (a helper-backed read or write, a package install, a restart, a block): contract, version, typed arguments, execution id, plan hash, authorization and deadline. |
| `capability_result` | agent → hub | The capability's typed outcome, or why it failed. |
| `refused` | agent → hub | The agent declined a request before running it: a stable `code`, a short `detail`, and `retry_after` or `scope` for a rate limit. A refusal ends that execution and never ends the session. |
| `run_check` | hub → agent | Re-run one check now. |
| `check_result` | agent → hub | One fresh observation per target. |
| `config_update` | hub → agent | New hub-managed settings, replacing the previous ones. |
| `agent_status` | agent → hub | What changed on the host since `hello`: the helper's status and public key, the skill runner's status, whether the agent runs as root or in a container, and the hash of its unattended ceiling. The hub answers nothing and keeps the values for the session. |
| `get_logs` | hub → agent | Ask for the agent's own recent log lines: at most `lines` of them, from `min_level` up, after `offset` lines already received, before a deadline. |
| `logs` | agent → hub | One chunk of those lines, oldest first. The hub asks again with a larger `offset` until it has the window. |
| `get_catalog` | hub → agent | Ask for one page (`page`, `page_size` of at most 100) of the agent's capability catalogue. |
| `catalog_page` | agent → hub | The contracts on that page, each with its id and version, and `next_page` when there are more. |
| `action_status` | hub → agent | Ask the agent's journal what happened to up to 64 executions, naming the journal epoch the hub believes it is asking about. |
| `action_status_answers` | agent → hub | The journal's actual epoch and one answer per requested execution, in request order. |
| `attest_safety` | hub → agent | A challenge: 32 random bytes the helper must sign into a safety report. |
| `safety_attestation` | agent → hub | The helper's public key, the report bytes and the signature, relayed unchanged. |
| `transfer_begin` | hub → agent | Start a file transfer for a package archive or signature. |
| `transfer_chunk` | hub → agent | One piece of a transfer. |
| `transfer_end` | hub → agent | End of a transfer. The agent answers with `transfer_result`. |
| `transfer_result` | agent → hub | Whether the transfer was stored, or why it was refused. Sent as the reply to `transfer_end`. |
| `error` | both | A structured error, as a reply or on its own. |
| `unsupported` | both | Reply to a frame whose kind the receiver does not know. |

## Safety attestation

On a host with a helper, the hub can ask for a signed report of what the helper enforces. The agent is only a relay: it cannot make, change or sign a report.

1. **Challenge.** The hub sends `attest_safety` with a `nonce` of exactly 32 random bytes (base64). The nonce is issued for one host, valid for under 60 seconds and usable once. The hub refuses to run two exchanges for one host at the same time.
2. **Answer.** The agent passes the nonce to the helper and sends back `safety_attestation`: `public_key` (32 raw bytes, base64), `report` (the report bytes, base64, at most 64 KiB) and `signature` (64 bytes, base64). The agent does not parse, re-encode or filter any of the three.
3. **Verification.** The hub checks that `public_key` is the key the owner pinned, then verifies the signature over `"domovoi-safety-attest-v1
"` followed by the report bytes, then parses those same bytes strictly (a report that does not re-render to the identical text is refused) and checks the nonce, the hashes and the generation. Only then is the report accepted. See [How a safety report is trusted](../security.md#helper-attestation).
4. **Error reply.** When the helper cannot sign, the agent answers with an `error` whose `code` is the helper's own token: `uninitialized`, `key_missing`, `key_corrupt`, `helper_state_permissions` or `lock_unavailable`. A helper that is absent, unreachable or not serving the operation is `helper_unavailable`. These are unsigned hints. The hub records and shows them but gives them no weight: the host stays unverified (`attestation_missing`).

`hello.in_container` and `agent_status.in_container` (wire 1.2) say whether the agent runs in a container. A frame that omits them reads as `false`.

`hello.helper_identity` and `agent_status.helper_identity` carry `{ "public_key": <base64 of 32 bytes> }` while the helper has a key, and absent otherwise. The agent never sends a fingerprint or key id; the hub computes the fingerprint (SHA-256 of the 32 key bytes) itself.

`run_capability` carries `expected_safety`, a closed tagged value the helper compares against what it computes:

- `{"state": "verified", "fingerprint": ..., "boot_id": ..., "generation": ..., "config_hash": ..., "route_state_hash": ...}`: the full identity of the verified report the request was authorized against.
- `{"state": "unverified", "reason": <one of nine tokens>}`: only for a human-approved run. The nine are `key_not_pinned`, `key_mismatch`, `attestation_missing`, `attestation_stale`, `signature_invalid`, `report_incomplete`, `generation_regressed`, `path_uncovered` and `confirmation_stale`.

The form is strict. A missing tag, an unknown tag, an unknown reason, an extra or repeated field, a field that belongs to the other tag, upper-case hex and a wrong length are each refused. It is required on every containment request, and the helper refuses one without it. The wire version stays 1.2.

## Containment

`firewall.block_source` (version 1) and `firewall.list_blocks` (version 1) travel as ordinary `run_capability` requests. They use no frame or field of their own; the replies are the existing `capability_result`, `refused` and `error` messages and the `action_status` answers.

A block is three executions, one per phase, each with its own execution id and its own entry in the agent's journal. The `args` hold exactly six keys, all strings:

| Key | Meaning |
| --- | --- |
| `phase` | `apply`, `confirm` or `rollback`. |
| `source` | The canonical network text, with the prefix length always written. Anything else is refused. |
| `ttl_s` | The lifetime after `confirm`, in seconds. Identical in all three phases. |
| `block` | The execution id of the `apply`. In `apply` it equals the request's own execution id. |
| `prefix_len_v4` | The target's prefix length when it is IPv4, else `32`. |
| `prefix_len_v6` | The target's prefix length when it is IPv6, else `128`. |

`target` is `ipnet:` followed by the canonical network. The three phases of one block carry the same `plan_hash`, the same `authorization_kind` and the same `expected_safety`, so one approval covers one block. The plan hash is a separate family (`plan:containment-v1`) over the contract, the source, the lifetime, the host, the host key and the safety assurance. It does not include the phase or the execution id. `expected_safety` is required on every phase of `firewall.block_source`; `firewall.list_blocks` is a read and carries none, and needs a `human` authorization.

The agent accepts a `policy` authorization only with a verified `expected_safety`; with `unverified` it refuses with `policy_requires_verified_safety`. A request without `expected_safety` is refused with the open code `malformed_safety`. `confirm` and `rollback` need a finished `apply` under the same `block` in the agent's journal, or they are refused with `unknown_block`. Only an `apply` counts toward the hourly limit.

Refusals and failures travel as `"<token>: <detail>"` in `Refused.detail` and `Failed.message`, with the token spellings of [Firewall operations](helper.md#firewall-operations): for example `exclusion_overlap`, `ttl_above_maximum`, `containment_blocked`, `table_drift`, `already_present`, `rolled_back`, `expired` and `containment_lost`. A reason that has no refusal code of its own travels as an open code. The hub sends `confirm` within 20 seconds of the `apply` result, never retries a phase after sending it, and settles an unanswered phase by asking the agent's journal.

## Agent safety on requests

The agent never runs anything on the hub's word alone. For `run_skill` it looks up the skill in its own manifests, checks the skill's risk against its local policy, validates `args` against the manifest's parameters and recomputes the plan hash. Any mismatch is answered with a `skill_result` error (`not_found`, `risk_not_allowed`, `invalid_args` or `plan_mismatch`), and nothing runs. The hub cannot send scripts or commands, only skill names and argument values.

## Package transfers

The hub sends a package in three steps: `transfer_begin`, one or more `transfer_chunk` frames, then `transfer_end`. The `kind` in `transfer_begin` is one of:

- `package_artifact`: the archive bytes (`<name>-<version>.tar.zst`).
- `package_manifest`: the detached signature file (`<name>-<version>.sig.toml`) bytes.

Rules:

- A chunk is at most 64 KiB. Chunk `seq` numbers start at 0 and have no gaps.
- Each transfer gets exactly one `transfer_result`, sent as the reply to its `transfer_end`.
- Use a fresh transfer UUID for every attempt. A `transfer_begin` for an id that is still arriving is ignored and the first transfer carries on, because `transfer_end` carries only the id.
- The agent keeps at most 8 records of arriving or failed transfers. A failed transfer past that bound is not remembered, and its `transfer_end` is answered as `aborted` ("unknown transfer").
- At most 2 transfers are active and 8 are staged. An active transfer is dropped after 60 s without a chunk, or 10 minutes in total. A staged transfer that is not used is dropped after 15 minutes.

The agent advertises these capabilities when `[packages]` is set in its config. A refused call returns a failed capability result with code `refused`, and the message is `<reason_code>: <detail>`. Reason codes include `untrusted_key`, `revoked_key`, `bad_signature`, `digest_mismatch`, `identity_mismatch`, `archive_invalid`, `path_escape`, `link_entry`, `version_conflict`, `no_space`, `packages_dir_unsafe` and `store_corrupt`.

| Capability | Arguments | Outputs |
| --- | --- | --- |
| `agent.package.install` | `package`, `version`, `artifact_transfer`, `signature_transfer` | `package`, `version`, `key_id`, `archive_sha256`, `installed_at`, `skills`, `replaced_version`, `already_installed`, `signer_updated`, `enabled` (always `"false"`), `privilege_boundary` |
| `agent.package.uninstall` | `package`, `version` | `package`, `version`, `removed`, `privilege_boundary` |
| `agent.package.enable` | `package`, `version` | `package`, `version`, `enabled`, `changed`, `tree_sha256` |
| `agent.package.disable` | `package`, `version` | `package`, `version`, `enabled`, `changed`, `tree_sha256` |
| `agent.package.list` | none | `count`, `install_allowed`, `privilege_boundary`, `packages` (a JSON array) |

`artifact_transfer` and `signature_transfer` refer to the archive and signature transfers sent before the call.

### How the hub uses these frames

For one install, on one host, the hub:

1. Sends the archive: `transfer_begin` with kind `package_artifact`, then `transfer_chunk` frames of at most 64 KiB each (the last one may be shorter), then `transfer_end`. It waits for that transfer's `transfer_result` before going on.
2. Sends the signature file the same way with kind `package_manifest`, under a new transfer id.
3. Sends `run_capability` for `agent.package.install` (or `agent.package.uninstall`, which has no transfers), naming the two transfer ids and the execution id of this attempt.
4. Waits for the capability result.

Each step has a deadline. A transfer's deadline is 60 s plus its size at 256 KiB/s, capped at 15 minutes. The install call has 300 s plus 30 s of slack, uninstall 60 s, and list 30 s. A transfer that gets no `transfer_result` by its deadline is treated as not stored.

Each transfer id is fresh for every attempt, and each transfer gets exactly one `transfer_result`. If the connection changes during a transfer, the hub stops and does not send the install call. If the hub has sent `run_capability` and then loses the answer, it does not resend it. It asks the agent's journal instead.

## Examples

Each example is a complete frame with fixed ids and times. The frames match the snapshots that pin the wire format.

### `enroll`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "enroll",
  "payload": {
    "agent_version": "0.1.0",
    "facts": {
      "arch": "x86_64",
      "boot_id": "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
      "boot_time": "2023-11-03T08:26:40Z",
      "cpu_count": 4,
      "hostname": "nas",
      "kernel": "6.12.0-1-amd64",
      "mem_total_bytes": 17179869184,
      "os": "Debian GNU/Linux 13"
    },
    "host_name": "nas",
    "public_key": "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
    "token": "dmv_enroll_0123456789abcdef"
  },
  "v": 1
}
```

### `enrolled`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "enrolled",
  "payload": {
    "agent_id": "01900000-0000-7000-8000-0000000000a1",
    "host_id": "01900000-0000-7000-8000-0000000000b1",
    "hub_cert_sha256": "0909090909090909090909090909090909090909090909090909090909090909"
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `hello`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "hello",
  "payload": {
    "agent_id": "01900000-0000-7000-8000-0000000000a1",
    "agent_version": "0.1.0",
    "capabilities": [
      "docker",
      "journal"
    ],
    "catalog_hash": null,
    "ceiling_hash": null,
    "ceiling_summary": null,
    "checks": [
      {
        "enabled": true,
        "interval_seconds": 300,
        "key": "disk.free",
        "thresholds": {
          "warn_percent": 85
        }
      }
    ],
    "docker": {
      "available": true,
      "enabled": true,
      "endpoint": "unix:///run/podman/podman.sock",
      "engine": "Podman",
      "locked": false,
      "problem": null
    },
    "facts": {
      "arch": "x86_64",
      "boot_id": "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
      "boot_time": "2023-11-03T08:26:40Z",
      "cpu_count": 4,
      "hostname": "nas",
      "kernel": "6.12.0-1-amd64",
      "mem_total_bytes": 17179869184,
      "os": "Debian GNU/Linux 13"
    },
    "features": [],
    "helper_identity": null,
    "helper_status": {
      "state": "absent"
    },
    "in_container": true,
    "journal_epoch": "01900000-0000-7000-8000-0000000000f0",
    "protocol": {
      "major": 1,
      "minor": 2
    },
    "runs_as_root": true,
    "skill_runner_status": {
      "state": "absent"
    },
    "skills": [
      {
        "applies_to": [
          "container.restart_loop"
        ],
        "description": "Restart one container.",
        "entry_script_digest": "fc07a55ac3f7c7d7ae2b7117459aa0b7e27421c964467ecd6f59c5c24bd1ad0b",
        "manifest_hash": "abababababababababababababababababababababababababababababababab",
        "name": "restart_container",
        "params": [
          {
            "description": "Container name.",
            "kind": {
              "max_len": 64,
              "pattern": "^[a-z0-9_.-]+$",
              "type": "string"
            },
            "name": "container",
            "required": true
          }
        ],
        "readme": "# Restart container\n\nRestarts one container managed by compose.",
        "risk": "write",
        "timeout": "1m",
        "verify": null
      }
    ]
  },
  "v": 1
}
```

### `challenge`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "challenge",
  "payload": {
    "nonce": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `challenge_response`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "challenge_response",
  "payload": {
    "signature": "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg=="
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `welcome`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "welcome",
  "payload": {
    "config": {
      "check_overrides": {
        "disk.free": {
          "enabled": true,
          "interval_seconds": 300,
          "thresholds": {
            "critical_percent": 95
          }
        }
      },
      "docker": false,
      "heartbeat_seconds": 30
    },
    "hub_version": "0.1.0",
    "session_id": "01900000-0000-7000-8000-0000000000d1"
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `reject`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "reject",
  "payload": {
    "reason": {
      "code": "incompatible",
      "hub": {
        "major": 1,
        "minor": 2
      }
    }
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `ping`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "ping",
  "payload": {
    "at": "2023-11-14T22:13:20Z"
  },
  "v": 1
}
```

### `pong`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "pong",
  "payload": {
    "at": "2023-11-14T22:13:20Z"
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `signals`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "signals",
  "payload": {
    "batch_seq": 42,
    "signals": [
      {
        "check": "disk.free",
        "class": "normal",
        "evidence": {
          "used_percent": 97.5
        },
        "host": "01900000-0000-7000-8000-0000000000b1",
        "id": "01900000-0000-7000-8000-0000000000c1",
        "observed_at": "2023-11-14T22:13:20Z",
        "severity": "critical",
        "source": {
          "type": "built_in"
        },
        "status": "firing",
        "summary": "/srv is 97% full",
        "target": "/srv"
      }
    ]
  },
  "v": 1
}
```

### `signals_ack`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "signals_ack",
  "payload": {
    "up_to_seq": 42
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `digest_facts`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "digest_facts",
  "payload": {
    "facts": [
      {
        "host": "01900000-0000-7000-8000-0000000000b1",
        "key": "zfs.tank.used_pct",
        "label": "tank used",
        "observed_at": "2023-11-14T22:13:20Z",
        "unit": "%",
        "value": 61.0
      }
    ]
  },
  "v": 1
}
```

### `run_tool`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "run_tool",
  "payload": {
    "call": {
      "container": "postgres",
      "lines": 50,
      "tool": "docker_logs"
    },
    "deadline": "2023-11-14T22:14:50Z"
  },
  "v": 1
}
```

### `tool_result`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "tool_result",
  "payload": {
    "outcome": {
      "ok": {
        "text": "ready to accept connections",
        "truncated": false
      }
    }
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `get_logs`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "get_logs",
  "payload": {
    "deadline": "2023-11-14T22:14:50Z",
    "lines": 50,
    "min_level": "info",
    "offset": 0
  },
  "v": 1
}
```

### `logs`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "logs",
  "payload": {
    "has_more": false,
    "lines": [
      {
        "at": "2023-11-14T22:13:20Z",
        "level": "warn",
        "message": "/srv is 97% full",
        "target": "agent.checks"
      },
      {
        "at": "2023-11-14T22:13:20Z",
        "level": "error",
        "message": "agent not connected; retrying in 4s",
        "target": "agent.link"
      }
    ]
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `run_skill`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "run_skill",
  "payload": {
    "action_id": "01900000-0000-7000-8000-0000000000e1",
    "args": {
      "container": "postgres"
    },
    "deadline": "2023-11-14T22:14:50Z",
    "plan_hash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
    "skill": "restart_container"
  },
  "v": 1
}
```

### `skill_result`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "skill_result",
  "payload": {
    "outcome": {
      "err": {
        "code": "plan_mismatch",
        "message": "manifest changed since approval"
      }
    }
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

A successful run:

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "skill_result",
  "payload": {
    "outcome": {
      "ok": {
        "argv_hash": "0303030303030303030303030303030303030303030303030303030303030303",
        "duration_ms": 1250,
        "exit_code": 0,
        "stderr": "",
        "stdout": "postgres\n",
        "truncated": false
      }
    }
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `run_check`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "run_check",
  "payload": {
    "check": "disk.free",
    "deadline": "2023-11-14T22:14:50Z",
    "target": "/srv"
  },
  "v": 1
}
```

### `check_result`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "check_result",
  "payload": {
    "observations": [
      {
        "evidence": {
          "used_percent": 97.5
        },
        "observed_at": "2023-11-14T22:13:20Z",
        "status": "firing",
        "summary": "/srv is 97% full",
        "target": "/srv"
      }
    ]
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `config_update`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "config_update",
  "payload": {
    "config": {
      "check_overrides": {
        "disk.free": {
          "enabled": true,
          "interval_seconds": 300,
          "thresholds": {
            "critical_percent": 95
          }
        }
      },
      "docker": false,
      "heartbeat_seconds": 30
    }
  },
  "v": 1
}
```

### `error`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "error",
  "payload": {
    "code": "invalid_payload",
    "message": "signals: missing field `batch_seq`"
  },
  "v": 1
}
```

### `unsupported`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "unsupported",
  "payload": {
    "type": "future_kind"
  },
  "v": 1
}
```

### `agent_status`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "agent_status",
  "payload": {
    "ceiling_hash": null,
    "ceiling_summary": null,
    "helper_identity": null,
    "helper_status": {
      "state": "absent"
    },
    "in_container": true,
    "runs_as_root": true,
    "skill_runner_status": {
      "state": "absent"
    }
  },
  "v": 1
}
```

### `run_capability`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "run_capability",
  "payload": {
    "args": {
      "container": "plex"
    },
    "authorization_kind": "human",
    "contract": "container.restart",
    "execution_id": "01900000-0000-7000-8000-0000000000f1",
    "expected_safety": null,
    "expires_at": "2023-11-14T22:14:50Z",
    "plan_hash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
    "target": "container:plex",
    "version": 1
  },
  "v": 1
}
```

A containment request. `expected_safety` is required:

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "run_capability",
  "payload": {
    "args": {
      "source": "203.0.113.9/32"
    },
    "authorization_kind": "human",
    "contract": "firewall.block_source",
    "execution_id": "01900000-0000-7000-8000-0000000000f1",
    "expected_safety": {
      "boot_id": "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
      "config_hash": "0101010101010101010101010101010101010101010101010101010101010101",
      "fingerprint": "abababababababababababababababababababababababababababababababab",
      "generation": 7,
      "route_state_hash": "0202020202020202020202020202020202020202020202020202020202020202",
      "state": "verified"
    },
    "expires_at": "2023-11-14T22:14:50Z",
    "plan_hash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
    "target": "host:nas",
    "version": 1
  },
  "v": 1
}
```

The same request authorized by a person while the helper is unverified:

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "run_capability",
  "payload": {
    "args": {
      "source": "203.0.113.9/32"
    },
    "authorization_kind": "human",
    "contract": "firewall.block_source",
    "execution_id": "01900000-0000-7000-8000-0000000000f1",
    "expected_safety": {
      "reason": "key_not_pinned",
      "state": "unverified"
    },
    "expires_at": "2023-11-14T22:14:50Z",
    "plan_hash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
    "target": "host:nas",
    "version": 1
  },
  "v": 1
}
```

### `capability_result`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "capability_result",
  "payload": {
    "outcome": {
      "succeeded": {
        "outputs": {
          "previous_state": "running"
        }
      }
    }
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `refused`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "refused",
  "payload": {
    "code": "rate_limited",
    "detail": "too many mutations this hour",
    "retry_after": 30,
    "scope": "global"
  },
  "v": 1
}
```

### `action_status`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "action_status",
  "payload": {
    "execution_ids": [
      "01900000-0000-7000-8000-0000000000f1"
    ],
    "expected_epoch": "01900000-0000-7000-8000-0000000000f0"
  },
  "v": 1
}
```

### `action_status_answers`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "action_status_answers",
  "payload": {
    "answers": [
      {
        "execution_id": "01900000-0000-7000-8000-0000000000f1",
        "state": {
          "finished": {
            "result": {
              "outcome": {
                "succeeded": {
                  "outputs": {}
                }
              }
            }
          }
        }
      }
    ],
    "epoch": "01900000-0000-7000-8000-0000000000f0"
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `get_catalog`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "get_catalog",
  "payload": {
    "page": 0,
    "page_size": 50
  },
  "v": 1
}
```

### `catalog_page`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "catalog_page",
  "payload": {
    "entries": [
      {
        "contract": "container.restart",
        "descriptor": {
          "family": "container",
          "verb": "restart"
        },
        "version": 1
      }
    ],
    "next_page": null
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `transfer_begin`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "transfer_begin",
  "payload": {
    "kind": "package_artifact",
    "sha256": "0404040404040404040404040404040404040404040404040404040404040404",
    "size": 3,
    "transfer_id": "01900000-0000-7000-8000-0000000000f1"
  },
  "v": 1
}
```

### `transfer_chunk`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "transfer_chunk",
  "payload": {
    "bytes": "AQID",
    "seq": 0,
    "transfer_id": "01900000-0000-7000-8000-0000000000f1"
  },
  "v": 1
}
```

### `transfer_end`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "transfer_end",
  "payload": {
    "transfer_id": "01900000-0000-7000-8000-0000000000f1"
  },
  "v": 1
}
```

### `transfer_result`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "transfer_result",
  "payload": {
    "outcome": "stored",
    "transfer_id": "01900000-0000-7000-8000-0000000000f1"
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```

### `attest_safety`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "attest_safety",
  "payload": {
    "nonce": "BQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU="
  },
  "v": 1
}
```

### `safety_attestation`

```json
{
  "id": "01900000-0000-7000-8000-000000000001",
  "kind": "safety_attestation",
  "payload": {
    "public_key": "BgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgYGBgY=",
    "report": "ZG9tb3ZvaS1hdHRlc3RhdGlvbg==",
    "signature": "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBw=="
  },
  "reply_to": "01900000-0000-7000-8000-000000000002",
  "v": 1
}
```
