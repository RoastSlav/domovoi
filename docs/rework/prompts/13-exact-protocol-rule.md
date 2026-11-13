# 13 — Exact protocol rule, durable Incompatible state

| | |
|---|---|
| Branch | `exact-protocol-rule` |
| Milestone | F3 |
| Depends on (must be merged) | none |
| Can run in parallel with | 08, 09, 10, 11, 12 |
| Produces (shared contracts) | `agents.reported_protocol`, `reported_release`, `last_authenticated_at`, `incompatible_since`; `HostRepo::{agent_report, record_agent_report}`; `domovoi_hub::agent_server::{ProtocolState, protocol_state}`; `LinkError::Incompatible` (core); agent link-status file |
| Consumes | existing `RejectReason::Incompatible { hub }`; existing `AuditLog` port |
| Launch gate | none |
| Suggested model | medium |

## 1. Objective

Replace the protocol compatibility window and the two release floors with one rule: **an agent and a hub
talk only when their protocol `major.minor` are equal**. Patch releases never matter. Make the hub
decide this only **after** the agent proves its key, record what the agent reported on its `agents`
row (only when the values change, with an audit entry), and derive the host's `Incompatible` or
"expected incompatible" state from that row so it survives a hub restart. Make the agent stay up in an
idle state after an `Incompatible` reject instead of exiting (and being restarted by systemd every
5 s). **No wire change: the protocol stays 1.1.**

This prompt runs in parallel with the unit-of-work prompts (08 to 12). **Assume they are not merged.**
Write the report with the existing store APIs; a later prompt moves it into a `UnitOfWork`
transaction. If they are merged when you run, still use the APIs described here, and say so in the
report.

## 2. Architectural context

- **Decision D-20 / RW-17.** Exact major.minor match; no backward or forward window. Same-protocol
  patch releases interoperate. A mismatched agent is `Incompatible`, not "offline", and keeps its
  history. Operations on it are `Unavailable` with reason `agent_incompatible`. Upgrades are
  coordinated: hub first, then agents. There is no agent self-update.
- **Why the order changes.** Today the hub compares protocols *before* the challenge
  (`handshake.rs:165`), so anyone who knows an agent id gets a refusal, and nothing about it can be
  trusted enough to store. After this change the hub stores something about a host only after the
  signature over a fresh challenge verified (INV-10). An unauthenticated client with a known agent id
  changes nothing.
- **No new reject variant.** `RejectReason::Incompatible { hub }` already exists and is *final* on every
  released agent. A reject after the challenge is already handled by agents (the `AgentTooOld` path
  uses one).
- **Release floors are not a compatibility dimension.** `MIN_AGENT_VERSION` and `MIN_HUB_VERSION` and the
  hub's in-memory `record_refusal` go away. The wire variant `RejectReason::AgentTooOld` stays in the
  enum (removing it is a wire change; old frames and snapshots must still decode) but the hub never
  sends it again.
- **Durable, derived state.** The `agents` row is the source of truth. The UI state is *derived*:
  - reported protocol equals the hub's → compatible;
  - reported protocol differs and the hub itself rejected the agent (`incompatible_since` set) →
    `Incompatible`;
  - reported protocol differs but the hub never rejected it (`incompatible_since` is NULL; the agent
    was compatible with an older hub and this hub was upgraded) → "expected incompatible (last
    reported X, hub requires Y)";
  - never authenticated → unknown.
- **Agent side.** From this release on, an agent that is rejected as `Incompatible` stays up in an *idle
  incompatible state*: it does not reconnect, it logs on entry and hourly, and `domovoi agent status`
  says so. Upgrading and restarting the agent ends the state. Agents released before this change exit
  and are restarted by systemd every 5 s until upgraded; the hub must therefore write only on change
  and must not flood logs.
- **Owner of the contract.** This prompt owns `Incompatible`. Prompt 14 (`recovery-order`) defines
  `HostAvailability` and consumes `protocol_state`; this prompt only has to make work on such hosts
  fail with the reason `agent_incompatible`.

## 3. Repository context

All paths are relative to the repository root `C:\Programming\domovoi`. Verify each before use.

**Protocol crate**
- `crates/domovoi-proto/src/version.rs:17` `PROTOCOL_VERSION = 1.1`.
- `version.rs:22-26` the window rule being replaced:
  ```rust
  pub fn compatible(agent: ProtocolVersion, hub: ProtocolVersion) -> bool {
      agent.major == hub.major
          && agent.minor <= hub.minor
          && agent.minor.saturating_add(1) >= hub.minor
  }
  ```
- `version.rs:44-66` `MIN_AGENT_VERSION`, `MIN_HUB_VERSION`, `AGENT_TOO_OLD_REASON_SINCE`; `at_least`,
  `parse_release`, `Release`, `is_release_tag` are also here. `at_least`'s only users are the two
  floors and `refusal_reason`; `parse_release`/`Release`/`is_release_tag` have other users (the
  update checker, `domovoi upgrade`); do not remove those.
- `crates/domovoi-proto/src/lib.rs:23-26` re-exports all of the above.
- `crates/domovoi-proto/src/messages/handshake.rs:226-248` `RejectReason { UnknownAgent, Revoked,
  Incompatible { hub }, AgentTooOld { minimum }, BadSignature, BadToken, #[serde(other)] Unknown }`.
- `crates/domovoi-proto/src/envelope.rs:82` frames whose envelope `v` differs from this build's major
  fail to decode (`UnsupportedMajor`). So an agent on another **major** cannot even deliver a `hello`;
  it is not authenticated and not recorded. That is existing behaviour; do not change it.
- Tests: `crates/domovoi-proto/tests/protocol/version.rs` (`compatibility_matrix`,
  `current_version_is_one_one`), snapshots under `crates/domovoi-proto/tests/protocol/snapshots/`
  (`protocol__snapshots__hello.snap`, `..._reject.snap`, `..._reject_agent_too_old.snap`, ...) driven by
  `tests/protocol/snapshots.rs` and `samples.rs`.

**Hub handshake** (`crates/domovoi-hub/src/agent_server/handshake.rs`)
- `:136-148` `refusal_reason(agent_version, minimum)` (floor helper; delete).
- `:150-224` `authenticate`. Current order: lookup (`UnknownAgent`, `Revoked`) → `compatible(...)` check at
  `:165` → challenge → verify signature `:190` → floor check `:194-213` with `record_refusal` → `agent.version`
  update `:214-217` → `welcome`. Excerpt of the part that moves:
  ```rust
  if !compatible(hello.protocol, PROTOCOL_VERSION) {
      let reason = RejectReason::Incompatible { hub: PROTOCOL_VERSION };
      return Err(reject(transport, reason, id).await);
  }
  let nonce = Nonce(rand::random());
  ```
- `reject()` at `:81-90` sends the reject and returns `ConnError::Rejected(reason)`.
- `crates/domovoi-hub/src/agent_server/registry.rs:29-40` `struct Refusal`; `:79` `refused` map; `:87`
  `refusal_lock`; `:123` `self.refusal_lock().remove(&host)` inside `register`; `:142` `record_refusal`;
  `:147` `refusal`.
- `crates/domovoi-hub/src/agent_server/mod.rs:42` `pub use registry::{..., Refusal}`; `:55-70`
  `AgentServerDeps { clock, hosts, facts, sink, probes, hub_host }`; `:146-152` `host_refusal`.
- `crates/domovoi-hub/src/agent_server/link.rs:24-48` `WsAgentLink { registry, clock }`, `new` at `:29`,
  `request` at `:34` calls `self.registry.dispatch(host, message)?`, which returns
  `LinkError::NotConnected` at `registry.rs:169` when there is no live session.
- `AgentServerDeps` is constructed in: `crates/domovoi-hub/src/runtime/mod.rs:204`,
  `crates/domovoi-hub/src/admin/mod.rs:109`, `crates/domovoi-hub/examples/dev_hub.rs:165`,
  `crates/domovoi-hub/tests/hub/harness.rs:126`, `tests/hub/enrollment.rs:31`, `tests/hub/api_harness.rs:345`,
  `crates/domovoi/tests/agent_hub.rs:83`.

**Hub API** (`crates/domovoi-hub/src/api/`)
- `mod.rs:56` imports `Refusal`; `:83` trait `AgentAdmin::refusal`; `:121` its impl for `AgentServer`.
- `hosts.rs:44-48` `refused_text`; `:68-83` `host_dto` sets `refused: state.deps.agents.refusal(host.id).map(refused_text)`.
- `dto_admin.rs:67-70` `HostDto.refused: Option<String>`.
- `examples/dev_hub.rs:33,135` and `tests/hub/api_harness.rs:24,110,156,195,373,455`, `tests/hub/api_admin.rs:5,7,263`
  implement or use `refusal`.
- `web/src/api/openapi.json` and `web/src/api/schema.d.ts` are generated; never hand-edit.

**Store and core**
- `crates/domovoi-store/migrations/` holds `0001_init.sql` to `0005_ack_rule_creator.sql`.
  `agents` is defined in `0001_init.sql:15-24` (STRICT; `agent_id`, `host_id`, `public_key`, `version`,
  `enrolled_at`, `revoked_at`).
- `crates/domovoi-store/src/repos/hosts.rs`: `AgentRow` `:36-55`, `put_agent` `:120-153`, `get_agent` `:155`,
  `live_agent` `:166`, `revoke_agent` `:179`.
- `crates/domovoi-core/src/ports/repos/hosts.rs:29-` trait `HostRepo`; `crates/domovoi-core/src/host.rs:104-117`
  `AgentIdentity` (**do not add fields to it**: ~12 struct literals in tests build it).
- In-memory fake: `crates/domovoi-testkit/src/repos/hosts.rs` (`impl HostRepo for InMemoryStore`).
  Contract tests: `crates/domovoi-testkit/src/contracts/hosts.rs` (`host_repo_contract`), run against both
  the fake and SQLite.
- Audit: `crates/domovoi-core/src/ports/repos/audit.rs` `AuditEvent { at, actor, kind, subject, detail }` and
  `AuditLog::append`.
- `crates/domovoi-core/src/ports/agent_link.rs:87-100` `enum LinkError { NotConnected, Timeout,
  Rejected(String), Transport(String) }`.

**Agent**
- `crates/domovoi-agent/src/link/handshake.rs:8,66-71` `MIN_HUB_VERSION` check on `welcome.hub_version`
  returning `LinkError::HubTooOld`; `:80-98` `refused()` maps rejects to `LinkError`.
- `crates/domovoi-agent/src/error.rs:51-90` `LinkError::{Incompatible, AgentTooOld, HubTooOld}`; `:93-102`
  `is_final()` matches `Revoked | Incompatible | AgentTooOld | HubTooOld`.
- `crates/domovoi-agent/src/link/mod.rs:146-176` `Link::run`: a final error returns `Err(err)`
  (`:163-167` for a session error, `:171` for a connect error). `LinkParts` at `:98-116`;
  constructed in `src/runtime.rs:198` and `tests/agent/harness.rs:79`.
- `crates/domovoi-agent/src/runtime.rs:228` `link.run(stop.clone())`; the runtime then stops the scheduler
  and returns `result.map_err(AgentError::from)`; `crates/domovoi/src/cmd/agent.rs:55-56` turns that into
  the process error.
- `crates/domovoi-agent/src/fs_atomic.rs` `write_atomic(path, bytes, mode)`, `read_optional`.
- `deploy/systemd/domovoi-agent.service:25-26` `Restart=always`, `RestartSec=5`.
- `crates/domovoi/src/cmd/agent.rs:15-22` `AgentCommand { Run, Enroll }`. **There is no `status`
  subcommand today.**
- Tests: `crates/domovoi-agent/tests/agent/link.rs` (`agent_refused_by_the_hub_for_being_too_old_is_final`
  `:210`, `incompatible_reject_stops_with_upgrade_message` `:236`, `hub_older_than_the_agent_minimum_is_refused`
  `:253`), `tests/agent/harness.rs` (`TestHub`, `hub.accepted()`, `start_link`).

**Hub tests** (`crates/domovoi-hub/tests/hub/`): `enrollment.rs:183` `incompatible_major_version_is_rejected`,
`:204` `agent_older_than_the_hub_minimum_is_refused`, `:239`, `:264`; helpers in `agent.rs`
(`FakeAgent`, `hello_running`, `rejected`, `enroll`, `online`) and `harness.rs` (`Hub::start`, an
`InMemoryStore`, a `FakeClock`).

**Web**: `web/src/features/hosts/{HostMeta,HostCards,HostsTable,hostSort,hostFilter,HostsPage}.tsx|ts`,
`web/src/features/overview/{OverviewPage,OverviewTiles}.tsx`, `web/src/design/{status,tone,fleetVocab}.ts`,
tests `web/src/features/hosts/hostsList.test.tsx:160-175`.

**Book**: `docs/book/src/reference/protocol.md:23-29,53-55` (also wrongly says the current version is
1.0), `docs/book/src/install/upgrading.md:36-38`, `docs/book/src/concepts/hub-and-agent.md:31`,
`docs/book/src/guide/agents.md:34`, `docs/book/src/reference/cli.md`.

## 4. Exact scope

Build, in this order of layers:

1. **Proto.** Exact rule; remove floors; keep wire types.
2. **Core + store + testkit.** Two `HostRepo` methods, an `AgentReport` type, a migration, a contract test.
3. **Hub.** Reorder `authenticate`; persist the report; audit; derive state; remove `Refusal`; fail work on an
   incompatible host with `LinkError::Incompatible`; API fields.
4. **Agent.** Remove `MIN_HUB_VERSION`; idle incompatible state; link-status file; `domovoi agent status`.
5. **Web.** Host states `Incompatible` and "Expected incompatible".
6. **Docs.** Book pages.

Files expected to change (new ones marked **(new)**):
- `crates/domovoi-proto/src/version.rs`, `src/lib.rs`, `src/messages/handshake.rs` (doc comments only),
  `tests/protocol/version.rs`, and a new frames test (see §12).
- `crates/domovoi-core/src/host.rs` (add `AgentReport`, `ReportedProtocol`, `AuthenticatedReport`,
  `ReportOutcome`), `src/lib.rs` re-exports, `src/ports/repos/hosts.rs`, `src/ports/agent_link.rs`
  (`LinkError::Incompatible`).
- `crates/domovoi-store/migrations/0006_agent_protocol_report.sql` **(new)**; `src/repos/hosts.rs`.
- `crates/domovoi-testkit/src/repos/hosts.rs`, `src/contracts/hosts.rs`.
- `crates/domovoi-hub/src/agent_server/{handshake,registry,mod,link}.rs`, `compat.rs` **(new)**.
- `crates/domovoi-hub/src/api/{mod,hosts,dto_admin}.rs`; `src/runtime/mod.rs`, `src/admin/mod.rs`,
  `examples/dev_hub.rs` (deps, `refusal` removal).
- `crates/domovoi-agent/src/{error,link/handshake,link/mod,runtime}.rs`, `link/status.rs` **(new)**.
- `crates/domovoi/src/cmd/agent.rs`, `src/cmd/mod.rs` only if needed for the new subcommand.
- Tests listed in §12; web files listed in §3; book pages in §15; `web/src/api/openapi.json` and
  `schema.d.ts` regenerated.

## 5. Explicit non-goals

- **No wire change.** Do not touch `PROTOCOL_VERSION`, message payloads, envelope, or any snapshot other than
  the new ones this prompt adds. Do not add or remove a `RejectReason` variant.
- No `HostAvailability` type, no startup-order work, no dispatch freshness checks (prompt 14). If
  `HostAvailability` already exists in the code when you run (prompt 14 merged first), also make
  `WsAgentLink::availability` return `Incompatible { reported }` for `ProtocolState::Incompatible`, using
  the same derivation; otherwise prompt 14 wires it. No
  `agent_status`, `run_capability` or any 1.2 field (prompt 20).
- No `UnitOfWork`, no `Actor` enum, no audit hash chain (prompts 08, 09). Use the existing
  `AuditLog::append` and the string actor form `system:agent_server`.
- No operations or steps (M3). "Unavailable(agent_incompatible)" in this prompt means only: link calls to such
  a host fail with `LinkError::Incompatible`, and the API shows the state.
- No agent self-update, no automatic reconnect after an incompatible hub is changed. An idle agent is
  restarted by the operator.
- Do not touch the update checker, `domovoi upgrade`, `parse_release`, `Release`, `is_release_tag`.
- Do not change the TLS, token or enrollment paths.

## 6. Required interfaces and contracts

### 6.1 Proto — **fixed**

```rust
/// Whether a hub at `hub` accepts an agent at `agent`: the same major and the same minor.
/// Patch releases do not enter into it.
#[must_use]
pub fn compatible(agent: ProtocolVersion, hub: ProtocolVersion) -> bool {
    agent.major == hub.major && agent.minor == hub.minor
}
```
Delete `MIN_AGENT_VERSION`, `MIN_HUB_VERSION`, `AGENT_TOO_OLD_REASON_SINCE`, `at_least` and their unit tests
and re-exports. Keep `Release`, `parse_release`, `is_release_tag`. Update the module doc and the
`Release` doc comment (it said peers refuse each other below a minimum). Update the doc comments of
`RejectReason::Incompatible` ("the protocol versions are not equal") and `RejectReason::AgentTooOld`
("No longer sent by the hub; kept so frames from older hubs still decode").

### 6.2 Core types — **fixed names and fields**

In `crates/domovoi-core/src/host.rs`:
```rust
/// A protocol version as an agent reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReportedProtocol { pub major: u16, pub minor: u16 }
// Display "1.1"; FromStr parses "major.minor" (two u16, nothing else).

/// What the hub last recorded about an agent after it proved its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReport {
    pub protocol: ReportedProtocol,          // agents.reported_protocol
    pub release: String,                     // agents.reported_release
    pub last_authenticated_at: Timestamp,    // agents.last_authenticated_at
    pub incompatible_since: Option<Timestamp>, // agents.incompatible_since
}

/// What an authenticated `hello` said, for `record_agent_report`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedReport {
    pub protocol: ReportedProtocol,
    pub release: String,
    /// The hub's verdict under the exact rule for this hello.
    pub compatible: bool,
}

/// The result of recording a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportOutcome {
    /// True when protocol, release or the incompatible flag differed from the stored row.
    pub changed: bool,
    pub previous: Option<AgentReport>,
}
```
Every public item needs a doc comment saying what it is for. Re-export from `crates/domovoi-core/src/lib.rs`
next to `AgentIdentity`.

`HostRepo` additions (**fixed**):
```rust
/// What `agent` last reported after authenticating; `None` when it never has.
async fn agent_report(&self, agent: AgentId) -> Result<Option<AgentReport>, StoreError>;

/// Records `seen` for `agent` at `now`. [`StoreError::NotFound`] when the agent does not exist.
/// Writes only when something changed (see below), and says whether it did.
async fn record_agent_report(
    &self, agent: AgentId, seen: &AuthenticatedReport, now: Timestamp,
) -> Result<ReportOutcome, StoreError>;
```
Write rules inside `record_agent_report`, in **one** transaction (SQLite) or one lock (fake):
1. Read the row. No row: `NotFound`.
2. `incompatible_since` for the new row: `None` when `seen.compatible`; otherwise the stored
   `incompatible_since` if it is set, else `Some(now)`.
3. `changed` = stored protocol/release are NULL or differ from `seen`, **or**
   `stored.incompatible_since.is_some() != new.incompatible_since.is_some()`.
4. If `changed`: write all four columns. If not `changed` but `last_authenticated_at` is NULL or older than
   `REPORT_REFRESH` (1 hour, `pub const REPORT_REFRESH: SignedDuration` in core): write only
   `last_authenticated_at = now` and still return `changed: false`. Otherwise write nothing.
5. Return `ReportOutcome { changed, previous }` where `previous` is the stored report (or `None` if the
   columns were NULL).

`LinkError` addition (**fixed**), in `crates/domovoi-core/src/ports/agent_link.rs`:
```rust
/// The host's agent speaks another protocol version than this hub. Work on the host is
/// unavailable until the agent is upgraded. Reason code: `agent_incompatible`.
#[error("agent_incompatible: the agent speaks another protocol version; upgrade it")]
Incompatible,
```
Run `cargo build --workspace` to find exhaustive matches on `LinkError` and handle the new variant the
same way as `NotConnected` where the code treats "cannot reach the host" (report each such place).

### 6.3 Hub derived state — **fixed**

New `crates/domovoi-hub/src/agent_server/compat.rs`, re-exported from `agent_server/mod.rs`:
```rust
/// How a host's agent stands against this hub's protocol, derived from its stored report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolState {
    /// The agent never authenticated, or has no enrolled agent.
    Unknown,
    /// Its last reported protocol equals the hub's.
    Compatible,
    /// This hub rejected the agent's protocol. Final until the agent is upgraded.
    Incompatible { reported: ProtocolVersion, since: Timestamp },
    /// It was last compatible with an older hub and has not reconnected since this hub's protocol
    /// changed.
    ExpectedIncompatible { reported: ProtocolVersion },
}

/// Pure derivation; `hub` is `PROTOCOL_VERSION` in production.
pub fn protocol_state(report: Option<&AgentReport>, hub: ProtocolVersion) -> ProtocolState;
```
Rules: `None` → `Unknown`. Reported equals `hub` → `Compatible` (even if `incompatible_since` is stale).
Else `incompatible_since = Some(t)` → `Incompatible { reported, since: t }`. Else → `ExpectedIncompatible`.

`AgentServer` gets `pub async fn protocol_state(&self, host: HostId) -> Result<ProtocolState,
StoreError>`: `live_agent(host)` → `agent_report(agent)` → `protocol_state(.., PROTOCOL_VERSION)`; no live
agent → `Unknown`. (Prompt 14 consumes this.)

### 6.4 Hub handshake order — **fixed**

```text
authenticate(hello):
  1. agent = get_agent(hello.agent_id)           -> UnknownAgent reject (not persisted)
  2. agent.revoked_at set                         -> Revoked reject (not persisted)
  3. send challenge; read challenge_response
  4. verify signature                             -> BadSignature reject (not persisted)
  5. compatible = compatible(hello.protocol, PROTOCOL_VERSION)
  6. outcome = hosts.record_agent_report(agent.agent, {hello.protocol, hello.agent_version, compatible}, now)
     - Err(e) -> return Err(e.into())   (the connection fails; nothing is half-recorded)
  7. if outcome.changed: audit.append(kind "agent.protocol_reported", see 6.5). An audit error is logged
     at error level and does NOT stop the handshake (documented limitation: not atomic with step 6
     until the unit-of-work change).
  8. if !compatible: log (warn if outcome.changed, else debug), then reject Incompatible { hub: PROTOCOL_VERSION }
  9. update agents.version if it differs (existing code), send welcome (existing code)
```
Nothing about the protocol is read or stored before step 4. There is no release-floor check.

### 6.5 Audit entry — **fixed**

`AuditEvent { at: now, actor: "system:agent_server", kind: "agent.protocol_reported", subject:
"agent:<agent_id>", detail: json }` with
```json
{ "host": "<host_id>", "reported_protocol": "1.0", "reported_release": "0.9.0",
  "hub_protocol": "1.1", "compatible": false,
  "previous_protocol": "1.1", "previous_release": "0.8.0" }
```
(`previous_*` are `null` when the columns were NULL.) Never put the key, nonce or signature in it.

### 6.6 Hub API — **fixed field names** (replace `refused`)

`HostDto` loses `refused` and gains:
```text
protocol_state:    "compatible" | "incompatible" | "expected_incompatible" | "unknown"
reported_protocol: string | null     // "1.1"
hub_protocol:      string            // "1.1"
reported_release:  string | null
incompatible_since: string | null    // RFC 3339
```
`host_dto` computes them from `state.deps.hosts` (`live_agent` then `agent_report`) with
`protocol_state(.., PROTOCOL_VERSION)`. A connected host is always `"compatible"` regardless of the row.
Remove `AgentAdmin::refusal`, `refused_text`, `Refusal`, `Registry::{record_refusal, refusal,
refused, refusal_lock}`, `AgentServer::host_refusal`, and the `self.refusal_lock().remove(&host)` line in
`register`. Update every implementor and test listed in §3.

### 6.7 Link failure for an incompatible host — **fixed**

`WsAgentLink` gets `hosts: Arc<dyn HostRepo>` (constructor `new(registry, clock, hosts)`). In `request`
(and any other direct `dispatch` use in `link.rs`), when `registry.dispatch` returns
`LinkError::NotConnected`, call a private `async fn not_connected(&self, host) -> LinkError` that reads
`live_agent` and `agent_report` and returns `LinkError::Incompatible` only when `protocol_state` is
`Incompatible` (the hub itself rejected the agent). `ExpectedIncompatible` is a prediction, not a finding:
the agent may have been upgraded together with the hub and not reconnected yet, so it returns
`NotConnected`, as it does for `Unknown` and `Compatible` and on a store read error (log it). A host with a
live session never takes this path.

### 6.8 Agent — **fixed**

- `LinkError::HubTooOld` and the `MIN_HUB_VERSION` check in `link/handshake.rs` are **removed**. The agent
  accepts any `welcome`.
- `LinkError::AgentTooOld` stays (an older hub may still send the wire variant), but both
  `Incompatible` and `AgentTooOld` become **idle** errors, not final ones:
  ```rust
  /// Whether retrying cannot help, so the link stops.
  pub fn is_final(&self) -> bool { matches!(self, Self::Revoked(_)) }
  /// Whether the agent should stay up and stop reconnecting until upgraded and restarted.
  pub fn is_idle(&self) -> bool { matches!(self, Self::Incompatible { .. } | Self::AgentTooOld { .. }) }
  ```
- `Link::run`: on `Err(err) if err.is_idle()` (from a session or a connect), call
  `self.idle(err, &shutdown).await` and return `Ok(())` when `shutdown` fires. `idle`:
  1. writes the status file (below) with `state = "incompatible"`;
  2. logs one `tracing::error!` with the message "this agent speaks protocol X and the hub speaks Y;
     upgrade the agent and restart it; staying up without reconnecting" (use the `LinkError` text);
  3. loops: `select!` on `shutdown.cancelled()`, `clock.sleep_until(next_notice)` where `next_notice =
     now + IDLE_NOTICE_INTERVAL` (`pub const IDLE_NOTICE_INTERVAL: SignedDuration = SignedDuration::from_hours(1)`);
     on each wake re-log at `error` level and rewrite the status file with `last_notice_at`; and drains
     (discards) `self.outbox` so the scheduler never blocks on it. It never calls `connector.connect()`.
  `Link::run` still returns `Err` for `Revoked`.
- The scheduler and the spool keep running while idle (the spool is already bounded:
  `crates/domovoi-agent/src/spool.rs:26-28`).

Status file, new `crates/domovoi-agent/src/link/status.rs`:
```rust
/// `link-status.json` under the state directory: what `domovoi agent status` reads.
pub const STATUS_FILE: &str = "link-status.json";

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct LinkStatus {
    /// "connected" | "reconnecting" | "incompatible"
    pub state: String,
    pub updated_at: Timestamp,
    pub connected_since: Option<Timestamp>,
    /// For "incompatible": the protocol the hub said it speaks and this agent's.
    pub hub_protocol: Option<String>,
    pub agent_protocol: Option<String>,
    pub last_notice_at: Option<Timestamp>,
    pub detail: Option<String>,
}
pub fn write(dir: &Path, status: &LinkStatus) -> Result<(), StateError>   // write_atomic, mode 0o644
pub fn read(dir: &Path) -> Result<Option<LinkStatus>, StateError>
```
`LinkParts` gets `pub status_dir: Option<PathBuf>`; `runtime.rs` sets `Some(state_dir)`, the test harness may
set `None` or a tempdir. Write "connected" after a `welcome`, "reconnecting" when a session drops with a
non-final error. A write failure is logged at `warn` and never stops the link.

CLI (**convention**): `domovoi agent status [--config PATH]` loads the agent config (same `ConfigArg` as
`run`), reads the status file from `config.agent.state_dir`, and prints one to three plain lines, for
example `incompatible: this agent speaks protocol 1.1, the hub speaks 1.2. Upgrade the agent, then
restart it. (updated 2026-01-02T03:04:05Z)`. Missing file: `no status recorded; the agent has not run
yet`. The file may be stale if the agent is not running; the line says when it was updated.

### 6.9 Migration — **fixed**

`crates/domovoi-store/migrations/0006_agent_protocol_report.sql` (see §8).

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file in §3. Confirm
   `PROTOCOL_VERSION` is 1.1, `authenticate` has the pre-challenge check, `0006_*` is the next free
   migration number (if prompts 08 or 09 are merged, take the next free number and keep the content),
   and that `AgentIdentity` literals exist in tests. Run `cargo build --workspace` once to see the baseline.
   Write down any difference before editing.
2. **Proto.** Replace `compatible`, delete floors and `at_least`, fix re-exports and unit tests,
   update doc comments. Update `tests/protocol/version.rs`. `cargo test -p domovoi-proto`.
3. **Core.** Add the types, the two `HostRepo` methods, `REPORT_REFRESH`, `LinkError::Incompatible`.
4. **Store.** Migration; extend `AgentRow` is **not** needed (new columns are read by new queries); implement
   `agent_report` and `record_agent_report` in `repos/hosts.rs` as one `write.begin()` transaction like
   `put_agent`. Timestamps are unix ms via the existing `ms`/`from_ms` helpers.
5. **Testkit.** Implement both methods on `InMemoryStore` with identical semantics; add the contract
   test `agent_report_is_recorded_only_on_change` to `host_repo_contract`.
6. **Hub core.** Reorder `authenticate`; add `audit` to `AgentServerDeps` (`pub audit: Arc<dyn AuditLog>`),
   passing `store.clone()` at every construction site in §3; add `compat.rs`; add `AgentServer::protocol_state`;
   pass `hosts` into `WsAgentLink::new`; implement `not_connected`; delete `Refusal` and friends and
   `refusal_reason`.
7. **Hub API.** DTO fields, `host_dto`, remove `refusal` from `AgentAdmin` and implementors. Regenerate
   OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`.
8. **Agent.** Remove `HubTooOld` and the floor check; `is_idle`; `Link::run` idle path; `status.rs`;
   `LinkParts.status_dir`; runtime wiring; `domovoi agent status`.
9. **Web.** Replace the `refused` connection with `incompatible` and `expected_incompatible`, using the
   words in §6.10.
10. **Tests** (§12), then book pages (§15).
11. **Checks and completion report** (see the standard blocks). Run, at least:
    `cargo fmt --all`; `cargo clippy -p <crate> --all-targets -- -D warnings` and
    `cargo nextest run -p <crate>` for `domovoi-proto`, `domovoi-core`, `domovoi-store`, `domovoi-testkit`,
    `domovoi-hub`, `domovoi-agent`, `domovoi`; `cargo nextest run -E 'rdeps(domovoi-core)'` (public port
    changed); `cargo xtask arch-check`; `cargo xtask openapi` then `cd web && npm run gen:api`;
    `cd web && npm run lint && npm run typecheck && npx vitest run src/features/hosts src/features/overview src/design`;
    `cargo xtask docs-links`. Review any `insta` snapshot diff.

### 6.10 Web words — **fixed**

In `web/src/design/fleetVocab.ts` `HOST_CONNECTIONS` replace `refused` with:
- `incompatible`: label "Incompatible", tone `danger`, icon `ban`, hint "The agent speaks another protocol
  version than the hub. Upgrade the agent, then restart it."
- `expected_incompatible`: label "Expected incompatible", tone `warning`, hint "The hub was upgraded and this
  agent has not reconnected yet. It will be incompatible if it still speaks the old protocol."
Use only tokens and `Icon` names that exist; read `web/DESIGN.md` first. `HostConnection` in
`design/status.ts` replaces `"refused"` with `"incompatible" | "expected_incompatible"`.
`hostConnectionOf` returns them from `host.protocol_state` (after the `connected` check, before `hub_only`).
Detail text, built in the feature from `reported_protocol`/`hub_protocol` (never from a raw code):
- incompatible: "agent speaks 1.1, hub requires 1.2; upgrade the agent";
- expected: "last reported 1.1, hub requires 1.2".
Rank both states with `offline` (rank 0) in `hostSort.ts`; both count as "needs a person" in
`hostFilter.ts`, `HostsPage.tsx` and `OverviewPage.tsx`/`OverviewTiles.tsx` where `refused` was counted.
Every file stays at or under 250 lines (split if needed).

## 8. Database and persistence

Migration `0006_agent_protocol_report.sql`:
```sql
-- What an agent reported after it proved its key (protocol compatibility, RW-17).
ALTER TABLE agents ADD COLUMN reported_protocol      TEXT;     -- 'major.minor', e.g. '1.1'
ALTER TABLE agents ADD COLUMN reported_release       TEXT;
ALTER TABLE agents ADD COLUMN last_authenticated_at  INTEGER;  -- unix ms UTC
ALTER TABLE agents ADD COLUMN incompatible_since     INTEGER;  -- unix ms UTC; NULL when compatible
```
All four are nullable; existing rows keep NULL (state `Unknown` until the agent next authenticates, which
fills them at its next connect). Additive only; an N-1 binary ignores the extra columns because `AgentRow`
selects named columns.

Transaction: `record_agent_report` reads and writes in one `write` transaction. The audit entry is a
separate `AuditLog::append` after it (documented non-atomicity; a later prompt moves both into a
`UnitOfWork`).

N-1 upgrade test (store crate, next to the existing migration tests; find them with
`grep -rn "migrat" crates/domovoi-store/tests`): create a database at schema 0005 with an `agents` row,
open with the new store, assert the row reads back through `get_agent` and `agent_report` returns `None`.

## 9. Protocol and compatibility

- Wire: **unchanged**. Same hello, challenge, welcome and reject payloads; 1.1 frames are byte-identical.
- Behaviour change visible to peers: agents 1.0 (one minor behind) were accepted before and are now rejected
  with `incompatible`; a 1.2 agent against a 1.1 hub is rejected. Agents that predate this release treat
  `incompatible` as final, exit, and restart every 5 s; the hub writes only on change so that loop causes no
  write storm. Log lines on a repeat refusal are at `debug`.
- An unauthenticated client now receives a challenge even when its `hello` carries a wrong protocol.
  It learns nothing more than before.
- A hello from another **major** is rejected at decode (`UnsupportedMajor`) and is neither authenticated nor
  recorded. Do not try to change this.
- `RejectReason::AgentTooOld` remains decodable; the golden frame `reject_agent_too_old` stays.
- An idle agent does not notice if the *hub* is later changed to match it (for example a hub rollback).
  The book says to restart the agent.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Unknown or revoked agent id | `reject` (`unknown_agent` / `revoked`) before the challenge; nothing stored |
| Hello with any protocol, signature invalid | `reject(bad_signature)`; nothing stored; no protocol check happened |
| Signature valid, protocol equal | record (only if changed), `welcome` |
| Signature valid, protocol differs | record, audit when changed, `reject(Incompatible { hub })`, close |
| `record_agent_report` fails | connection fails with the store error; no `welcome`; log at `error` |
| `audit.append` fails after a successful record | log at `error`, continue (reject or welcome as decided) |
| Same mismatching agent reconnects every 5 s | no row write inside `REPORT_REFRESH`; no audit; `debug` log |
| Hub restart | state derived from the row; `Incompatible` or `ExpectedIncompatible`, never lost |
| Hub upgraded, agent not reconnected | `ExpectedIncompatible` (shown in the UI); link calls fail with `NotConnected` |
| Agent rejected `Incompatible` | idle state, no `connect()`, hourly `error` log, status file updated |
| Agent rejected `Revoked` / `UnknownAgent` | still final: `run` returns the error |
| Agent shutdown while idle | `run` returns `Ok(())` promptly |
| Status file write fails | `warn`, link continues |
| Agent reads `welcome` with an old `hub_version` | accepted; no floor |

## 11. Security requirements

- **INV-10.** Nothing about an agent's protocol, release or incompatibility is stored or published before the
  challenge signature over this connection's channel binding verified. Test:
  `unauthenticated_hello_does_not_persist_incompatibility`.
- Fail closed: any store error in step 6 ends the connection without a `welcome`.
- Audit: each change of the recorded report writes `agent.protocol_reported`; it contains no secret.
- `agents.reported_*` are informational. They never grant anything; the session is still established only by
  the signed challenge.
- The status file holds no secret (mode 0o644). It is a hint, not a trust input.
- Do not add `#[allow]`, `unwrap` or `expect` in library code.

## 12. Testing requirements

Use `FakeClock`, `InMemoryStore`, `FakeAgent`, `TestHub` as existing. No sleeps. New items marked **(new)**.

**Proto** (`crates/domovoi-proto/tests/protocol/version.rs`)
- `exact_rule_matrix`: replaces `compatibility_matrix`. Cases `(agent, hub, accepted)`: `(1.1,1.1,true)`,
  `(1.0,1.1,false)`, `(1.2,1.1,false)`, `(2.1,1.1,false)`, `(1.1,2.1,false)`, `(1.65535,1.65535,true)`.
- **Golden frames 1.1.** The existing snapshots (`hello`, `challenge`, `challenge_response`, `welcome`,
  `reject`, `reject_agent_too_old`, ...) are the golden frames for 1.1. Add one test
  `golden_frames_stay_at_1_1` in `tests/protocol/snapshots.rs` asserting `PROTOCOL_VERSION` is 1.1 and that
  the `reject` sample carrying `Incompatible { hub: 1.1 }` still serialises as
  `{"code":"incompatible","hub":{"major":1,"minor":1}}` (read `samples.rs` for the existing sample).
  **No existing `.snap` may change.** If one changes, stop and report.

**Hub** (`crates/domovoi-hub/tests/hub/enrollment.rs` unless stated)
- `older_minor_agent_rejected_with_reason`: enrolled agent, `hello` protocol `1.0`, answers the challenge;
  expects `Reject(Incompatible { hub: 1.1 })`; then `store.agent_report(agent)` shows protocol 1.0 and
  `incompatible_since == Some(now)`.
- `newer_minor_agent_rejected_with_reason`: same with `1.2`.
- `same_protocol_different_patch_releases_communicate`: release strings `0.0.1`, `0.9.9`, `dev` with protocol
  1.1 all receive `welcome`.
- `unauthenticated_hello_does_not_persist_incompatibility`: hello with protocol `1.0`, answer the challenge with
  `answer_without_the_key` (existing helper); expects `BadSignature`; `agent_report` is `None`; no audit
  event of kind `agent.protocol_reported`; `server.protocol_state(host)` is `Unknown`. Replaces
  `an_agent_that_cannot_sign_its_challenge_records_no_refusal`.
- `unknown_agent_with_old_protocol_gets_unknown_agent_reject`: unchanged reject, nothing stored.
- `incompatible_state_survives_hub_restart` **(new harness helper)**: add `Hub::restart(&self) -> Hub` in
  `tests/hub/harness.rs` that builds a second `AgentServer` over the **same** `InMemoryStore`. Flow: reject a
  `1.0` agent; restart; `protocol_state(host)` is `Incompatible { reported: 1.0, .. }`; the hosts API (use the
  `api_harness` path with the same store, or call `protocol_state` directly) reports it.
- `expected_incompatible_after_hub_protocol_change`: unit test of `protocol_state` in `compat.rs`: a report with
  protocol 1.0, `incompatible_since: None`, hub 1.1 → `ExpectedIncompatible`; with `Some(t)` → `Incompatible`;
  equal protocols with a stale `Some(t)` → `Compatible`; `None` → `Unknown`.
- `repeated_mismatch_writes_once`: reject the same agent five times inside one hour; the audit log has exactly
  one `agent.protocol_reported`; `last_authenticated_at` unchanged; advance the clock 61 minutes, reconnect:
  `last_authenticated_at` advances, still one audit event.
- `compatible_reconnect_clears_incompatible_since`: reject at 1.0, then authenticate at 1.1 (a new `hello`):
  `incompatible_since` is `None` and a second audit event exists.
- `release_floor_no_longer_rejects`: replaces `agent_older_than_the_hub_minimum_is_refused` and
  `a_client_predating_the_reason_floor_is_refused_with_incompatible`: `hello_running(.., PROTOCOL_VERSION, "0.0.1")`
  gets `welcome`.
- Keep `incompatible_major_version_is_rejected` (payload major 2 in an otherwise valid frame): now it needs the
  signature first; update it to answer the challenge and expect `Incompatible`.
- `link_to_incompatible_host_is_unavailable` (`tests/hub/`): after the rejection, `server.link().run_tool(..)`
  (use any `AgentLink` method) returns `Err(LinkError::Incompatible)`; for a never-seen host it returns
  `NotConnected`; for a host whose row says protocol 1.0 with `incompatible_since: None` (expected
  incompatible) it returns `NotConnected`.
- API: `hosts_show_incompatible_and_expected_incompatible` in `tests/hub/api_admin.rs`, replacing
  `hosts_name_the_agent_refused_for_being_too_old`: put an agent and a report into the store, `GET
  /api/v1/hosts`, assert `protocol_state`, `reported_protocol`, `hub_protocol`, and that a connected host is
  `compatible`.

**Store/testkit**
- `agent_report_is_recorded_only_on_change` (contract, both stores): first record → `changed: true`; identical
  record at `now+1m` → `changed: false`, stored `last_authenticated_at` unchanged; at `now+61m` → `changed:
  false`, `last_authenticated_at` updated; a different release → `changed: true`; a mismatching record sets
  `incompatible_since` to `now` and a second mismatching record keeps the first value; a compatible record
  clears it; unknown agent id → `NotFound`.
- `migration_0006_keeps_existing_agents` (N-1).

**Agent** (`crates/domovoi-agent/tests/agent/link.rs`)
- `incompatible_agent_idles_without_reconnecting`: `TestHub` accepts once and rejects with `Incompatible`; assert
  the link task is **not** finished after `settle()`; advance the fake clock 3 hours with `settle()` between
  steps; `hub.accepted() == 1`; send shutdown; the task ends with `Ok(())`.
- `idle_incompatible_agent_logs_and_updates_status_hourly`: with `status_dir` set to a tempdir, after the reject
  the status file has `state == "incompatible"` and the hub/agent protocols; after advancing 1 hour,
  `last_notice_at` is later than before.
- `incompatible_reject_stops_with_upgrade_message` becomes `incompatible_reject_is_idle_not_final`: the error
  `is_idle()` and not `is_final()`; its text contains "upgrade".
- `agent_too_old_reject_from_older_hub_is_idle` replaces `agent_refused_by_the_hub_for_being_too_old_is_final`.
- `revoked_reject_still_ends_the_link`: keep the existing revoked test.
- `hub_older_than_the_agent_minimum_is_refused` becomes `welcome_from_any_hub_release_is_accepted`: welcome
  with hub release `0.0.1` is accepted and the session starts.
- `link_status_written_on_connect_and_drop`.
- CLI test in `crates/domovoi/tests/cli.rs`: `agent_status_reports_incompatible_state`: write a status file
  into a temp state dir, run `agent status --config <toml>`, assert the output contains "incompatible" and
  both protocol numbers; and `agent_status_without_file_says_not_run_yet`.

**Web** (vitest, update `hostsList.test.tsx`)
- `incompatible_agent_shows_both_protocols`: host with `protocol_state: "incompatible"`,
  `reported_protocol: "1.1"`, `hub_protocol: "1.2"` → row shows "Incompatible" and "agent speaks 1.1, hub
  requires 1.2; upgrade the agent".
- `expected_incompatible_agent_shows_last_reported`: shows "Expected incompatible" and "last reported 1.1,
  hub requires 1.2".
- `connected_host_is_never_shown_incompatible`.
- The axe check that every page test performs must stay green.

## 13. Acceptance criteria

1. `compatible()` is true exactly when major and minor are both equal; `MIN_AGENT_VERSION`, `MIN_HUB_VERSION`,
   `AGENT_TOO_OLD_REASON_SINCE`, `at_least`, `refusal_reason`, `Refusal` and `record_refusal` no longer
   exist; `rg "MIN_AGENT_VERSION|MIN_HUB_VERSION|record_refusal|HubTooOld" crates web/src docs/book` returns
   nothing.
2. `authenticate` performs no protocol comparison before the signature verifies (read the code and the test
   `unauthenticated_hello_does_not_persist_incompatibility`).
3. A mismatched but authenticated agent gets `Reject(Incompatible { hub })` and its `agents` row shows
   `reported_protocol`, `reported_release`, `last_authenticated_at` and `incompatible_since`.
4. The same mismatching agent reconnecting repeatedly writes the row and the audit log once per change (and
   at most one refresh per hour without audit).
5. After a hub restart (`incompatible_state_survives_hub_restart`) the host is `Incompatible`; after a hub
   upgrade that changed the protocol, a not-yet-reconnected host is `ExpectedIncompatible`.
6. `GET /api/v1/hosts` has no `refused` field and has the five new fields; OpenAPI and `schema.d.ts` are
   regenerated, not hand-edited.
7. The Hosts page shows "Incompatible" and "Expected incompatible" with the sentences in §6.10; both rank as
   needing attention.
8. An agent rejected as `Incompatible` does not return from `Link::run`, does not call `connect()` again,
   logs at `error` on entry and hourly, writes `link-status.json`, and `domovoi agent status` prints it.
9. An agent rejected as `Revoked` still stops.
10. Link calls to a host the hub rejected as `Incompatible` fail with `LinkError::Incompatible`
    (`agent_incompatible`); an `ExpectedIncompatible` host fails with `NotConnected`.
11. No `.snap` under `crates/domovoi-proto/tests/protocol/snapshots/` changed, `PROTOCOL_VERSION` is 1.1.
12. The book states the exact rule and the upgrade procedure (§15); `cargo xtask docs-links` passes.
13. `cargo xtask arch-check` passes; no new `#[allow]`; no file over 500 lines (web files over 250).

## 14. Migration and compatibility

- **Existing hub database.** Migration 0006 adds four nullable columns. Existing agents show `Unknown`
  protocol state until they next authenticate (seconds, for connected agents).
- **Existing agents.**
  - Same protocol 1.1: unaffected; they see no difference.
  - Agents at 1.0 (accepted before): now rejected `incompatible`. Pre-F3 agents exit and are restarted by
    systemd every 5 s until upgraded; F3 agents idle.
  - Upgrade order is hub first, then agents; with the exact rule, the hub's *next* protocol bump makes every
    old agent incompatible at once. This prompt does not bump the protocol (prompt 20 does).
- **Config.** No config key added or removed. `RestartSec=5` in the shipped unit is not changed.
- **Rollback.** An N-1 hub ignores the new columns. Rolling the hub back after agents were rejected leaves
  idle agents; restart them.

## 15. Documentation requirements

- `docs/book/src/reference/protocol.md`: rewrite "Versioning" (`:23-29`): current version **1.1**; exact
  major.minor rule; patch releases irrelevant; no release floors; the hub checks only after the signature
  verifies and answers `incompatible`; fix the handshake steps at `:53-55`; keep a sentence that `agent_too_old`
  is still decodable but no longer sent.
- `docs/book/src/install/upgrading.md` (`:36-38`): replace the window and floor paragraphs with the
  **upgrade procedure**: 1. upgrade the hub; 2. upgrade each agent through the supported install path;
  3. watch the Hosts page until no host is `Incompatible` or `Expected incompatible`. Say that an agent from
  this release on waits idle (does not reconnect, logs hourly, `domovoi agent status` shows it) and must be
  restarted after upgrading; older agents restart every 5 s until upgraded.
- `docs/book/src/concepts/hub-and-agent.md:31` and `docs/book/src/guide/agents.md:34`: align the sentence with
  the exact rule.
- `docs/book/src/reference/cli.md`: add `domovoi agent status`.
- `docs/book/src/reference/data-model.md`: mention the four `agents` columns only if the page lists `agents`
  columns today.
- `ARCHITECTURE.md`: do not edit it unless a rule there states the old window; if it does, change that
  sentence only and say so.
- Regenerate OpenAPI as in §7 step 7. No config reference change (`cargo xtask docs-config` not needed; run
  it and confirm no diff).
- Add a `CHANGELOG.md` entry only if the repository's convention (look at its top) requires one per change.
- Never mention prompts, the build plan or coding agents in any committed file.

## 16. Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## 17. Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## 18. Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
