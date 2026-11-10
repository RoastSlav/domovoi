# 30 · Security: cap unauthenticated agent connections

## Background (verified on `main`)

- The agent listener binds `0.0.0.0:7421` by default.
- `crates/domovoi-hub/src/agent_server/mod.rs` `serve()` (`:163-176`) spawns a task for
  every accepted TCP connection, with no limit.
- `conn::accept` (`agent_server/conn.rs:87-108`) gives the TLS and WebSocket upgrade
  `FIRST_FRAME_TIMEOUT` (10 s), and the handshake has its own 10 s steps
  (`handshake.rs:25-28`). So each connection is short-lived, but thousands can be open
  at once, each holding a TLS session and a task.
- Frame size and frame rate limits apply after the upgrade (`agent_server/ws.rs`).

## Steps

1. Add to `Inner` (`agent_server/mod.rs`): `pending: Arc<tokio::sync::Semaphore>` with
   `MAX_UNAUTHENTICATED = 64` permits, and a per-IP counter
   `Mutex<HashMap<IpAddr, u32>>` with `MAX_PER_IP = 8`.
2. In `serve()`, before spawning: `try_acquire_owned` a permit and bump the IP count; if
   either is over, drop the `TcpStream` at once and log at `debug` (rate-limit that log
   line: at most once a minute, with a count).
3. Release both when the handshake ends (enrolled, rejected, timed out or turned into a
   session). Do it with a small guard struct whose `Drop` decrements, in a new
   `agent_server/admission.rs`, so every exit path releases. A live session does not
   hold a permit.
4. Do not count the in-memory local link (`agent_server/memory.rs`).
5. Book: `docs/book/src/guide/hardening.md`: one line on the limits and that the agent
   port should still be firewalled to the hosts that need it.

## Tests

- Unit tests in `admission.rs`: the ninth connection from one IP is refused; a dropped
  guard frees the slot; the 65th connection overall is refused.
- `crates/domovoi-hub/tests/hub/agent.rs`: an agent still connects while 8 idle
  connections from another address are open.

## Done when

- At most 64 connections are in the handshake at once, and at most 8 from one address.
