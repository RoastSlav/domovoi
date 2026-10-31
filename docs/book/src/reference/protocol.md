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

The current version is **1.0**. The hub accepts an agent with the same major version and a minor version equal to its own or one behind. An agent newer than the hub, more than one minor behind, or on another major is refused with `reject` and the reason `incompatible`.

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

1. On the hub, `domovoi hub token create <host>` prints a single-use token valid for 15 minutes.
2. On the host, `domovoi agent enroll --hub <url> --token <token>` generates an ed25519 key pair and sends `enroll` with the token and the public key.
3. The hub replies `enrolled` with the agent id, host id and its certificate fingerprint, or `reject` with `bad_token`. The agent checks that the fingerprint matches the certificate it saw on this connection, then pins it.

## Handshake

1. The agent connects and sends `hello`.
2. The hub replies `challenge` with a random nonce, or `reject` (`unknown_agent`, `revoked`, `incompatible`).
3. The agent signs the challenge message and sends `challenge_response`.
4. The hub verifies the signature against the enrolled public key and replies `welcome`, or `reject` with `bad_signature`.

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
| `run_skill` | hub → agent | Run an approved action's skill: action id, skill, args, plan hash, deadline. |
| `skill_result` | agent → hub | Exit status, redacted and capped output, duration and argv hash, or why it did not run. |
| `run_check` | hub → agent | Re-run one check now. |
| `check_result` | agent → hub | One fresh observation per target. |
| `config_update` | hub → agent | New hub-managed settings, replacing the previous ones. |
| `error` | both | A structured error, as a reply or on its own. |
| `unsupported` | both | Reply to a frame whose kind the receiver does not know. |

## Agent safety on requests

The agent never runs anything on the hub's word alone. For `run_skill` it looks up the skill in its own manifests, checks the skill's risk against its local policy, validates `args` against the manifest's parameters and recomputes the plan hash. Any mismatch is answered with a `skill_result` error (`not_found`, `risk_not_allowed`, `invalid_args` or `plan_mismatch`), and nothing runs. The hub cannot send scripts or commands, only skill names and argument values.

## Examples

Each example is a complete frame with fixed ids and times.

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
    "protocol": {
      "major": 1,
      "minor": 0
    },
    "skills": [
      {
        "applies_to": [
          "container.restart_loop"
        ],
        "description": "Restart one container.",
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
        "risk": "write"
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
        "disk.usage": {
          "enabled": true,
          "interval_seconds": 300,
          "thresholds": {
            "critical_percent": 95
          }
        }
      },
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
        "minor": 0
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
        "check": "disk.usage",
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
    "check": "disk.usage",
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
        "disk.usage": {
          "enabled": true,
          "interval_seconds": 300,
          "thresholds": {
            "critical_percent": 95
          }
        }
      },
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
    "kind": "future_kind"
  },
  "v": 1
}
```
