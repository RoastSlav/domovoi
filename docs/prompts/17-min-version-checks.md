# 17 · Feature: minimum version checks, agent → hub and hub → agent

## Goal

Besides the protocol version, each side refuses a peer older than a minimum release
version, says so clearly in its log, and the hub shows refused agents on the Hosts page
("Agent 0.0.3 is older than the minimum 0.0.6; update it").

## What is already known (verified)

- Today only protocol versions are compared:
  `crates/domovoi-proto/src/version.rs` (`PROTOCOL_VERSION`, `compatible(agent, hub)`).
  The hub checks it in `crates/domovoi-hub/src/agent_server/handshake.rs`,
  `authenticate()`, right after the revoked check, and replies
  `RejectReason::Incompatible { hub }`.
- `Hello.agent_version` and `Welcome.hub_version` already carry release versions
  (`crates/domovoi-proto/src/messages/handshake.rs`).
- `RejectReason` has `#[serde(other)] Unknown`, so a new reason is wire-compatible: old
  agents see `Unknown`.
- The agent maps rejects in `crates/domovoi-agent/src/link/handshake.rs`, `refused()`,
  and receives `Welcome` at the end of the handshake function. Final errors (link stops,
  systemd restarts the agent) are listed in `LinkError::is_final()` in
  `crates/domovoi-agent/src/error.rs`.
- A semver comparison already exists in the hub: `crates/domovoi-hub/src/api/version.rs`
  (`triple()`, `is_older()`); unparseable versions ("dev") are never called outdated.
- Host presence for the UI comes from `crates/domovoi-hub/src/agent_server/registry.rs`;
  `HostDto` is in `crates/domovoi-hub/src/api/dto_admin.rs`, built in `api/hosts.rs`
  `host_dto()`.

## Read these files

- `crates/domovoi-proto/src/version.rs`, `messages/handshake.rs`
- `crates/domovoi-hub/src/agent_server/{handshake,registry}.rs`
- `crates/domovoi-hub/src/api/{version,hosts,dto_admin}.rs`
- `crates/domovoi-agent/src/link/handshake.rs`, `crates/domovoi-agent/src/error.rs`
- `web/src/features/hosts/HostStatus.tsx`

## Steps

1. **Proto (pure).** In `version.rs` add:
   ```rust
   /// A release version, `major.minor.patch`.
   #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
   pub struct Release { pub major: u64, pub minor: u64, pub patch: u64 }
   /// The oldest agent release this hub accepts.
   pub const MIN_AGENT_VERSION: Release = Release { major: 0, minor: 0, patch: 5 };
   /// The oldest hub release this agent accepts.
   pub const MIN_HUB_VERSION: Release = Release { major: 0, minor: 0, patch: 5 };
   /// `version` parsed as `major.minor.patch`, ignoring a leading `v` and a `-`/`+` suffix.
   pub fn parse_release(version: &str) -> Option<Release>
   /// Whether release `version` is at least `minimum`. Versions that do not parse pass,
   /// so dev builds are never locked out.
   pub fn at_least(version: &str, minimum: Release) -> bool
   ```
   plus `impl fmt::Display for Release` (`0.0.5`). Base `parse_release` on `triple()` from
   `crates/domovoi-hub/src/api/version.rs`, and make that file's `is_older` use
   `parse_release`, so there is one parser. Unit tests: equal, older, newer, suffixes,
   "dev".
2. **Proto.** Add `RejectReason::AgentTooOld { minimum: Release }` (before `Unknown`).
   `RejectReason` derives `Copy`, which is why `Release` is `Copy` too. Add a reject
   sample with it and update snapshots (rule 10; this is additive).
3. **Hub.** In `authenticate()`, after the protocol check:
   if `!at_least(&hello.agent_version, MIN_AGENT_VERSION)`, record the refusal (step 4)
   and reply `AgentTooOld { minimum: MIN_AGENT_VERSION }`. Log at `warn` with host,
   agent version and minimum.
4. **Hub registry.** Add to the registry a
   `refused: Mutex<BTreeMap<HostId, Refusal { version: String, minimum: String, at: Timestamp }>>`
   with `record_refusal(host, ..)`, `refusal(host) -> Option<Refusal>`, cleared when the
   host's agent connects successfully. Expose `refusal(host)` through the `AgentAdmin`
   trait in `api/mod.rs` (fakes return `None`).
5. **API.** Add `refused: Option<String>` to `HostDto`, set in `host_dto()` to
   `"agent {version} is older than the minimum {minimum}; update it"`. Regenerate OpenAPI.
6. **Agent.** In `refused()` map `RejectReason::AgentTooOld { minimum }` to a new
   `LinkError::TooOld { minimum: Release }` ("the hub needs agent {minimum} or newer;
   update this agent"). After receiving `Welcome`, if
   `!at_least(&welcome.hub_version, MIN_HUB_VERSION)` return a new
   `LinkError::HubTooOld { hub: String, minimum: Release }` ("the hub runs {hub} but
   this agent needs {minimum} or newer; update the hub"). Both are final in
   `is_final()`.
7. **Web.** In `HostStatus`, when `host.refused` is set, render it in the danger colour
   instead of the offline text.
8. **Docs.** In `docs/book/src/install/upgrading.md` and
   `docs/book/src/reference/protocol.md`, add the minimum-version rule and where to see a
   refusal. Mention in `docs/maintainers/repository-setup.md` (release steps) that the
   minimums are raised in `crates/domovoi-proto/src/version.rs` when a release breaks
   compatibility.

## Tests

- Proto: `at_least` unit tests; the new reject snapshot.
- Hub: an enrollment/handshake test (see `crates/domovoi-hub/tests/hub/agent.rs`) where
  the hello carries `agent_version: "0.0.1"` is rejected with code `agent_too_old`, and
  `GET /api/v1/hosts` then shows `refused`.
- Agent: a welcome with `hub_version: "0.0.1"` ends the link with `HubTooOld`; `"dev"`
  connects.

## Done when

- Too-old peers are refused on both sides with a clear log line; the Hosts page shows
  refused agents; dev builds still connect.
- All tests, snapshots and the OpenAPI files are updated.
