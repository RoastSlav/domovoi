# 20 - Agent action journal and wire protocol 1.2

| | |
|---|---|
| Branch | `agent-action-journal` |
| Milestone | M1 |
| Scope | Protocol 1.1 -> 1.2, the complete v1 wire contract, the agent action journal, `action_status`, the agent mutation limiter |
| Depends on (must be merged into `overhaul-investigation` before implementation) | 13, 14, 15, 16, 17, 18, 19 |
| Parallel with | none; prompt 21 depends on this one |
| Produces | the frozen protocol 1.2 wire contract and the durable agent action journal |
| Gate | none beyond the listed dependencies |
| Suggested model | large |

Drafting and launch are different. This brief is written now because `03 RW-42` is its authoritative
contract. Implementation starts only when all seven dependencies have merged. At the time this was
written, 13 (#116) and 15 (#136) were merged and 14, 16, 17, 18 and 19 were not. Do not weaken the
list to match current progress.

## 1. Objective

Deliver one coordinated wire upgrade from protocol 1.1 to 1.2 that freezes the complete v1 wire
contract, and implement the parts of that contract that exist today:

1. the durable agent-side action journal and its epoch;
2. idempotent acceptance of execution requests;
3. the `action_status` query and its answers;
4. the agent-side mutation rate limiter from `03 RW-38`;
5. every 1.2 message type, with golden frames.

The point is a single question the hub must always be able to answer: did a dispatched execution never
reach the agent, finish, is it still running in this agent instance, or is its outcome unknowable after
a crash or journal loss?

Everything in the 1.2 message set is frozen here, including messages whose consumer lands in a later
milestone. Later milestones consume this wire; they do not change it.

## 2. Scope and non-goals

This prompt owns:

- `domovoi-proto`: `PROTOCOL_VERSION`, the 1.2 message types and their serde representation, `Open<T>`,
  the tolerant-decoding rules, the 1.1 and 1.2 golden-frame tests.
- `domovoi-agent`: the action journal, its epoch, the durable accepted/started/finished lifecycle, the
  `action_status` handler, the mutation limiter, and `domovoi agent journal reset`.
- `domovoi-config`: `[mutation_limits]` in the agent section.
- `domovoi-hub`: only the minimum needed to keep the session compiling and answering the new frames;
  no reconciliation.

This prompt does **not** own, and must not implement:

| Owner | Excluded work |
|---|---|
| 21 | hub reconciliation on `hello`, mapping `action_status` to hub execution state, `OutcomeUnknown` commit |
| 22 | `ExecutionId`, plan hash v2, `Authorization`, `PolicyPermit`, approval binding, CI authorization checks |
| 23 | the single execution boundary and its arch rules |
| 24 | capability registry, contract definitions, skill adapter |
| 25 | capability catalogue contents and the availability model |
| 44, 47 | helper and skill-runner implementation, helper ADRs, identity separation |
| 46 | containment implementation and its reconciliation |
| 49 | package installation UI or install workflow |
| later | any capability execution backend |

Consequences:

- Do not invent a capability registry, policy engine, permit issuance or helper operation to make a
  message round-trip. The messages exist; their producers and consumers arrive later.
- Do not add a SQLite migration. The action journal is host-local agent state, not hub state. The index
  row for this prompt has no migration.
- Do not add a general execution or authorization path. `run_capability` is a wire type here; the path
  that issues it belongs to 22 and the path that executes it belongs to 24/25.

## 3. Verified current state (re-check before editing)

Read these before changing anything. If one now differs materially, record the difference before
editing and stop if it changes the design.

| Area | File | State when this was written |
|---|---|---|
| protocol version | `crates/domovoi-proto/src/version.rs` | `PROTOCOL_VERSION = { major: 1, minor: 1 }`; `compatible()` already exact major+minor (prompt 13) |
| message registry | `crates/domovoi-proto/src/messages/mod.rs` | one `messages!` macro; `Message::Unknown`; `KINDS`; `kind()`; `from_payload()` |
| handshake types | `crates/domovoi-proto/src/messages/handshake.rs` | `HostFacts` with `boot_id: String` (line ~65); `Hello` with `capabilities: Vec<String>` (line ~164); `RejectReason` with `#[serde(other)] Unknown` |
| work types | `crates/domovoi-proto/src/messages/work.rs` | `RunTool`, `RunSkill` (`args: BTreeMap<String, String>`, `plan_hash: PlanHash`), `RunCheck`, results, `ToolFailureCode`, `SkillFailureCode` |
| stream types | `crates/domovoi-proto/src/messages/stream.rs` | `Ping{at}`, `Pong{at}`, `Signals`, `SignalsAck`, `DigestFacts`, `ConfigUpdate`, `ErrorMessage`, `Unsupported{kind}` |
| wire bytes | `crates/domovoi-proto/src/bytes.rs` | fixed-length base64 newtypes (`Nonce`, `PublicKey`, `Signature`, `Sha256Bytes`); standard padded base64 |
| golden frames | `crates/domovoi-proto/tests/protocol/` | `snapshots.rs` with `golden_frames_stay_at_1_1`; `version.rs` with `current_version_is_one_one`; `roundtrip.rs`, `auth.rs`, `samples.rs` |
| heartbeats | `crates/domovoi-agent/src/link/mod.rs` | the agent sends `Ping` on `welcome.config.heartbeat_seconds` and gives up after three unanswered; the hub answers `Pong` and never pings (`session.rs`) |
| agent state | `crates/domovoi-agent/src/{identity,fs_atomic,spool}.rs` | `identity.json` mode 0600; `write_atomic`, `read_optional`; `spool.jsonl` + `spool.json`, 10 000 batches / 8 MiB, `batch_seq` across restarts |
| agent requests | `crates/domovoi-agent/src/requests/mod.rs` | `run_tool`, `run_skill`, `run_check`, `get_logs`; 4 concurrent, queue 32, per-request deadline; no journal, no dedup |
| agent facts | `crates/domovoi-agent/src/facts.rs` | `boot_id` from `/proc/sys/kernel/random/boot_id` (line ~70), `boot_time` from `/proc/stat` |
| agent config | `crates/domovoi-config/src/agent.rs` | `AgentSection` with `deny_unknown_fields`; `state_dir` defaults to `/var/lib/domovoi-agent`; `[policy] allow_risk` |
| hub session | `crates/domovoi-hub/src/agent_server/session.rs` | answers `Pong`, acks signals after submit, discards late results ("a result arrived after its request gave up") |
| hub handshake | `crates/domovoi-hub/src/agent_server/handshake.rs` | challenge -> verify -> exact protocol compare -> durable report -> `Reject{Incompatible}` |
| link port | `crates/domovoi-core/src/ports/agent_link.rs` | `AgentLink` with `run_tool`, `run_skill`, `run_check`, `presence`, `availability`, `skills`; `HostAvailability` from prompt 14 |

Unavailable today: `Open<T>`, `agent_status`, `action_status`, `run_capability`, `refused`,
`get_catalog`, `transfer_*`, `attest_safety`, `safety_attestation`, `expected_safety`, `journal_epoch`,
`helper_identity`, `mutation_limits`, any action journal. Build them; do not look for them.

## 4. Protocol 1.2: version and compatibility

1. Change `PROTOCOL_VERSION` to `{ major: 1, minor: 2 }`.
2. Keep `compatible()` exactly as prompt 13 left it: same major and same minor.
3. Patch releases remain outside compatibility. `Release` parsing does not enter the rule.
4. Do not add a rolling window, adapter, feature negotiation or "one minor behind" tolerance.
5. Do not reintroduce `MIN_AGENT_VERSION` or `MIN_HUB_VERSION`. Prompt 13 removed them; if any trace
   remains, it is a bug to report, not a feature to extend.
6. Keep the handshake order and the durable `Incompatible` report from `03 RW-17` and `04 12`:
   `hello` -> lookup -> challenge -> signature -> exact compare -> unit-of-work commit of
   `reported_protocol`, `reported_release`, `last_authenticated_at`, `incompatible_since` + audit ->
   `reject{Incompatible}`. Nothing is written for an unauthenticated client.
7. Do not change the agent authentication mechanism, the nonce sizes, the signature domain string or
   the channel binding.
8. A later unavoidable wire change is a coordinated `1.3`, approved before work starts. None is
   planned. Do not leave room for one in the type design.

Update the tests that assert the version (`current_version_is_one_one` becomes a 1.2 assertion; keep
the name meaningful, for example `current_version_is_one_two`), and keep `golden_frames_stay_at_1_1`
as a 1.1 historical fixture beside a new 1.2 suite.

## 5. `hello` extensions

`Hello` gains the fields below. Every field is optional-tolerant on decode but always written by the
agent, so the golden frame shows real values rather than a minimal object.

| Field | Type | Value in this milestone | Later owner |
|---|---|---|---|
| `features` | `Vec<String>`, sorted | empty. The agent does not claim a feature it does not implement | 24, 25 |
| `journal_epoch` | `Uuid` | the journal epoch | this prompt |
| `catalog_hash` | `Option<Sha256Bytes>` | `None` until the catalogue exists | 25 |
| `ceiling_hash` | `Option<Sha256Bytes>` | `None` until the unattended ceiling exists | later |
| `ceiling_summary` | `Option<String>` | `None` for the same reason | later |
| `skill_runner_status` | `SkillRunnerStatus` | `absent` | 47 |
| `helper_status` | `HelperStatus` | `absent` | 47 |
| `runs_as_root` | `bool` | the effective uid is 0; today's shipped unit installs as root, so this is `true` | 47 |
| `helper_identity` | `Option<HelperIdentity>` | `None`. Never fabricate a key, never derive one, never accept one from config | 44, 47 |
| `boot_id` | - | already `HostFacts.boot_id`; do not add a second copy (see G8) | - |

```rust
pub enum SkillRunnerStatus {
    Absent,
    Unsupported { reason: String },
    Error { reason: String },
    Ok { version: String },
}

pub enum HelperStatus {
    Absent,
    Ok { ops: Vec<String> },
    Error { reason: String },
}
```

Both enums are closed protocol enums: their variants are produced by the agent, not by a skill or a
package. Serialise them as internally tagged objects (`{"state": "ok", "version": "1"}`) or externally
tagged, but pick one and freeze it in a golden frame. `helper_identity` is:

```rust
pub struct HelperIdentity {
    /// Base64 (standard, padded) of exactly 32 raw Ed25519 public-key bytes.
    pub public_key: PublicKey,
}
```

`PublicKey` already rejects any other decoded length; keep that rejection. The hub computes the
fingerprint itself (SHA-256 of those 32 bytes, lowercase hex) and pins only through the owner
enrollment flow in `03 RW-37`. The agent never sends a fingerprint and the hub never trusts one if it
appears.

Rules:

- A status field says what is true now, not what the product intends. `absent` for a component that
  does not exist yet is the honest value.
- Do not attempt to detect a helper that this milestone does not implement. `absent` is a constant here,
  and a test pins it.
- `features`, `catalog_hash`, `ceiling_hash` and `ceiling_summary` must keep their wire presence even
  while empty. A later milestone fills them; it does not add fields.

## 6. `agent_status`

Add the runtime-change message from `03 RW-42`:

```rust
pub struct AgentStatus {
    pub helper_status: HelperStatus,
    pub helper_identity: Option<HelperIdentity>,
    pub skill_runner_status: SkillRunnerStatus,
    pub runs_as_root: bool,
    pub ceiling_hash: Option<Sha256Bytes>,
    pub ceiling_summary: Option<String>,
}
```

Direction: agent -> hub. The hub answers nothing (there is no `agent_status_result`), records the new
values for the session, and keeps behaviour otherwise unchanged until prompt 25 consumes them.

Emission rules:

- At startup after `welcome`, before or immediately after the first `hello`-derived snapshot is used.
- After a probe refresh when any value differs from the last one sent.
- The probe cadence described in `04` is startup, every 60 seconds and after a transport error. The 60
  second probe does not exist for these fields yet, because the components do not. Implement the timer
  and the change comparison over the fields that do exist (`runs_as_root`), and send `agent_status` only
  when a value actually changes. Do not send an empty heartbeat.
- Do not implement a helper or skill runner to populate the message.

The hub side in this prompt stores the values per live session and includes them in whatever the
session already exposes. It does not persist them, does not derive availability from them and does not
trigger reconciliation. Prompt 25 owns availability.

## 7. `run_capability`, `expected_safety` and the result

### 7.1 Request

```rust
pub struct RunCapability {
    /// The wire form of the execution id. `domovoi-core`'s `ExecutionId` arrives with a later
    /// prompt; the wire uses the UUID and that prompt converts.
    pub execution_id: Uuid,
    /// Contract id, `<family>.<verb>`, for example `container.restart`.
    pub contract: String,
    /// Contract version, strictly increasing per contract.
    pub version: u32,
    /// Canonical target, for example `container:plex`.
    pub target: TargetKey,
    /// Canonical argument map; values are canonical strings, as `run_skill` already does.
    pub args: BTreeMap<String, String>,
    pub plan_hash: PlanHash,
    pub authorization_kind: AuthorizationKind,
    pub expires_at: Timestamp,
    /// Required for `contain`, refused when missing or malformed (RW-37).
    pub expected_safety: Option<ExpectedSafety>,
}

pub enum AuthorizationKind {
    Human,
    Policy,
}
```

Rationale for the encodings the design left open is in section 12 (G1-G3). Newtypes that belong to
later prompts are not introduced here: `ExecutionId` is 22's, `ContractId` is 24's. The wire keeps the
primitive and the later prompt converts at its boundary, so 20 does not create a type those prompts
must then work around.

### 7.2 `expected_safety`

```rust
pub enum ExpectedSafety {
    Verified {
        /// Lowercase hex SHA-256 of the helper public key, 64 characters.
        fingerprint: String,
        /// The helper's boot id, as reported in its report.
        boot_id: String,
        generation: u64,
        config_hash: Sha256Bytes,
        route_state_hash: Sha256Bytes,
    },
    Unverified {
        reason: String,
    },
}
```

- `verified` is the only shape usable for policy authorization. An `unverified` value with
  `authorization_kind == Policy` is refused by the agent with `refused{code: policy_requires_verified_safety}`
  once the enforcement path exists (22/46); this prompt only freezes the shape and the rule.
- A malformed value (unknown tag, missing field, wrong hash length) is refused, not coerced.
  Unknown *future* tags are preserved by tolerant decoding only where the message is not required for
  authority; `expected_safety` is authority-bearing, so it is strict.
- Do not implement attestation checking here. `verified` is a claim carried on the wire; verifying it is
  46's job using 47's attestation.

### 7.3 Result

`RW-42` says "and its result" without fixing the payload. Freeze the following narrow shape, which
mirrors the existing `ToolOutcome`/`SkillOutcome` convention and leaves per-contract detail in an
extensible map rather than in a new enum per capability:

```rust
pub struct CapabilityResult {
    pub outcome: CapabilityOutcome,
}

pub enum CapabilityOutcome {
    Succeeded {
        /// Per-contract outputs, canonical strings, keys defined by the contract.
        outputs: BTreeMap<String, String>,
    },
    Failed {
        code: Open<CapabilityFailureCode>,
        message: String,
    },
}

pub enum CapabilityFailureCode {
    SpawnFailed,
    TimedOut,
    ExitedNonZero,
    OutputTooLarge,
    HelperUnavailable,
    Refused,
}
```

- Refusals before execution use the separate `refused` message, not this result. `Refused` in the
  failure code exists only for a backend that refuses after accepting.
- `Open<CapabilityFailureCode>` keeps a future code decodable. Unknown codes are preserved and treated
  as `Failed`, never as success.
- If prompt 24 later proves this shape insufficient, that is a 1.3 conversation. Do not pre-emptively
  generalise it now.

## 8. Remaining 1.2 messages

Add every one of these, with golden frames. Their consumers arrive later.

### 8.1 `refused`

```rust
pub struct Refused {
    pub code: Open<RefusalCode>,
    /// Human-readable, short, already redacted.
    pub detail: String,
    /// Seconds until a retry may succeed.
    pub retry_after: Option<u32>,
    /// Present for `rate_limited`; see G6.
    pub scope: Option<RefusalScope>,
}

pub enum RefusalCode {
    RateLimited,
    CeilingExceeded,
    AllowRiskDenied,
    PlanHashMismatch,
    TargetChanged,
    AuthorizationExpired,
    PolicyRequiresVerifiedSafety,
    ContractUnknown,
    ContractVersionUnsupported,
    /// The action journal is corrupt; only a local owner reset recovers (11.5).
    JournalCorrupt,
}

pub enum RefusalScope {
    Global,
    Capability { contract: String },
}
```

`03 RW-42` lists `refused{code, detail, retry_after?}` and `03 RW-38` requires `scope` on a rate-limit
refusal. `scope` is therefore an additive optional field; see G6. A refusal ends that execution and
must not end the session.

`JournalCorrupt` exists because `03 RW-38` names `journal_corrupt` as a refusal the agent sends; the
serialised value is `journal_corrupt`, and `Open<RefusalCode>` keeps any later code decodable.

### 8.2 `action_status`

```rust
pub struct ActionStatusQuery {
    pub execution_ids: Vec<Uuid>,
    /// The epoch the hub believes it is asking about.
    pub expected_epoch: Uuid,
}

pub struct ActionStatusAnswers {
    pub epoch: Uuid,
    pub answers: Vec<ActionStatusAnswer>,
}

pub struct ActionStatusAnswer {
    pub execution_id: Uuid,
    pub state: ActionState,
}

pub enum ActionState {
    Finished { result: CapabilityResult },
    Running,
    NotFound,
    OutcomeUnknown { reason: Open<OutcomeUnknownReason> },
}

pub enum OutcomeUnknownReason {
    NoStartRecord,
    AgentRestarted,
    JournalLost,
}
```

The answer echoes the journal's actual `epoch`, so a hub holding a stale epoch can tell the difference
between "lost" and "never existed" without a second round trip. Bound `execution_ids`: reject more than
64 ids in one query with `error{code: "bad_request"}`, and answer in request order. Semantics are in
section 10.4.

### 8.3 `get_catalog`

```rust
pub struct GetCatalog {
    /// Zero-based.
    pub page: u32,
    /// At most 100; a larger value is clamped, not refused.
    pub page_size: u32,
}

pub struct CatalogPage {
    pub entries: Vec<CatalogEntry>,
    pub next_page: Option<u32>,
}

pub struct CatalogEntry {
    pub contract: String,
    pub version: u32,
    /// The descriptor object owned by the capability contract (prompt 24/25).
    pub descriptor: serde_json::Value,
}
```

The descriptor stays an opaque JSON object at the wire layer. Defining it here would move 24's contract
into 20. The page size cap keeps a page under the 1 MiB frame limit even with generous descriptors;
reject a page whose encoded size exceeds `MAX_FRAME_BYTES` rather than truncating silently.

### 8.4 Transfers

```rust
pub struct TransferBegin {
    pub transfer_id: Uuid,
    pub kind: Open<TransferKind>,
    pub size: u64,
    pub sha256: Sha256Bytes,
}

pub enum TransferKind {
    PackageArtifact,
    PackageManifest,
}

pub struct TransferChunk {
    pub transfer_id: Uuid,
    pub seq: u64,
    /// Base64 of at most 64 KiB of raw bytes.
    pub bytes: Base64Bytes,
}

pub struct TransferEnd {
    pub transfer_id: Uuid,
}

pub struct TransferResult {
    pub transfer_id: Uuid,
    pub outcome: TransferOutcome,
}

pub enum TransferOutcome {
    Stored,
    Failed { code: Open<TransferFailureCode>, detail: String },
}
```

`Base64Bytes` is a new variable-length base64 newtype in `bytes.rs`, because the existing ones are all
fixed length. Enforce the 64 KiB decoded cap at decode time so a chunk cannot approach the frame limit.
`sha256` is verified by the receiver of the last chunk (prompt 49); this prompt only freezes the shape
and the cap. There is no per-chunk acknowledgement: `transfer_end` is answered once with
`transfer_result`. That keeps the frame count linear and lets the sender pipeline chunks.

### 8.5 Attestation

```rust
pub struct AttestSafety {
    /// Base64 of exactly 32 random bytes, hub -> agent.
    pub nonce: Nonce,
}

pub struct SafetyAttestation {
    /// Base64 of exactly 32 raw bytes; same type and length rule as `helper_identity`.
    pub public_key: PublicKey,
    /// Base64 of the report bytes defined by RW-37 and `04b`.
    pub report: Base64Bytes,
    /// Base64 of exactly 64 bytes.
    pub signature: Signature,
}
```

`Nonce` and `Signature` already exist and already reject the wrong length. Cap `report` at 64 KiB; the
largest report in `04b` is far below that and the cap keeps a hostile peer from filling the frame.

### 8.6 `unsupported`

Protocol 1.2 carries `unsupported{type}`. That is the authoritative field name in both `03 RW-42` and
`04 12`; 1.1 shipped `Unsupported { kind }`, and that name is not carried forward.

- `type` is a Rust keyword, so the Rust field is named `type_name` and renamed on the wire:
  `pub struct Unsupported { #[serde(rename = "type")] pub type_name: String }`. The golden frame pins
  the wire name `type`, not the Rust identifier.
- 1.2 is one coordinated exact-version upgrade: a 1.1 agent is rejected by the 1.2 hub (`04 12`), so
  there is no peer that still sends `kind` and no compatibility reason to keep it. Do not add a serde
  alias that accepts both; one field name per protocol version.
- Preserve the 1.1 golden frames unchanged as historical fixtures. They stop being generated from the
  live sample set: move the 1.1 frames to literal JSON fixtures under
  `crates/domovoi-proto/tests/protocol/fixtures/1_1/`, assert them byte for byte in
  `golden_frames_stay_at_1_1`, and leave them untouched by the 1.2 type changes. `samples::all()` and
  `every_message_kind_matches_its_wire_snapshot` become the 1.2 suite. See G10.

### 8.7 `ping`

`03 RW-42` says `ping` is hub -> agent. Today it is agent -> hub, and the agent's liveness depends on
it (`link/mod.rs:294-322`, `link/handshake.rs` heartbeat). Make 1.2 match the authoritative direction:

- The hub sends `ping{at}` on the session heartbeat interval, which it takes from the agent's
  `welcome.config.heartbeat_seconds` (default 10 s).
- The agent answers `pong{at}` and treats three missed hub pings, or a silent link for three
  intervals, as a dead link.
- The hub treats three missed `pong`s as a dead session and closes it, which feeds the existing
  presence path.
- Remove the agent-initiated ping loop. Keep `Ping`/`Pong` as one type each; the direction is a
  property of the session, not of the message.
- Update `link/mod.rs`, `link/handshake.rs`, `agent_server/session.rs`, `agent_server/presence.rs` and
  the tests that assert the old direction. Both ends must still detect a half-open connection.

This is a behaviour change inside the coordinated upgrade. It is not a licence to redesign presence:
if the change requires touching `HostAvailability` (prompt 14's contract) or the presence state machine
beyond the ping source, stop and report instead of half-implementing it.

## 9. Tolerant decoding, `Open<T>` and the session

1. Keep `Message::Unknown` for an unknown `kind`, and keep answering `unsupported{type}`.
2. The session continues after an unsupported request. A test proves it (`unsupported_message_does_not_end_session`).
3. Add `Open<T>` in `domovoi-proto`:

```rust
/// A protocol value that may be a variant this build knows, or one it does not.
pub struct Open<T> {
    raw: String,
    known: Option<T>,
}
```

   - It serialises as the raw string it decoded from.
   - It deserialises from a string and parses it into `T` when possible.
   - `known()` returns `Option<&T>`; `raw()` always returns the original.
   - It is not a generic "any JSON" escape hatch. Use it only for data enums whose values can come
     from a skill, a package or a later milestone: `RefusalCode`, `CapabilityFailureCode`,
     `OutcomeUnknownReason`, `TransferKind`, `TransferFailureCode`.
4. Closed protocol enums stay closed: `AuthorizationKind`, `HelperStatus`, `SkillRunnerStatus`,
   `ExpectedSafety`'s outer tag, `ActionState`. A peer that sends something else here is malformed,
   not forward-compatible, because these decide authority or state.
5. Unknown fields continue to be ignored (`envelope.rs`), as today. Do not switch to
   `deny_unknown_fields` on wire types.
6. Keep message-kind registration in the single macro in `messages/mod.rs`. Do not add a second table.

## 10. The durable agent action journal

The signal spool is not the journal. `spool.jsonl` batches signals; the journal records execution
lifecycle. Keep the files, the types and the tests separate. Do not add execution records to the spool
and do not reuse `spool.json`.

### 10.1 Location and files

Under `agent.state_dir`:

| File | Content |
|---|---|
| `action-journal.bin` | append-only framed records |
| `action-journal.json` | metadata, defined below, mode 0600 |

The metadata object is the single source of truth for the epoch, the pruning marker and the
current boot's first-load time:

```json
{
  "format": 1,
  "epoch": "018f3c1e-7a2b-7c3d-8e4f-5a6b7c8d9e0f",
  "pruned": false,
  "boot": {
    "boot_id": "5f0c2c6a-1f4e-4a1b-9d3e-2b7c8a9d0e11",
    "first_load_mono_ns": 1845000000
  }
}
```

- `format` is the metadata schema version, `1`. An unsupported value is corruption: fail closed.
- `epoch` is the journal epoch (10.2).
- `pruned` is the sticky pruning marker (10.6). It is `false` at creation and becomes `true`
  permanently on the first prune, until an explicit local reset starts a new journal with a new epoch.
- `boot` is the boot-load state `11.3` requires. `boot_id` is the value from
  `/proc/sys/kernel/random/boot_id`, and `first_load_mono_ns` is `/proc/uptime` in nanoseconds at the
  moment this agent process first loaded the journal on that boot.
  - It is written when the journal is created, and again only when the current `boot_id` differs from
    the stored one, at the first load of that boot.
  - An ordinary process restart on the same boot leaves it untouched, so a restart inside the
    reboot-recovery window cannot restart the window.
  - It survives the restart because it lives in this file, not in memory.
- Every metadata writer produces the complete object from the in-memory state. Never patch a single
  key on disk and never write a literal `false` for `pruned`: a boot-load update must carry the
  current sticky value forward.

**Atomic, durable write procedure.** All metadata updates use exactly this sequence, in this order:

1. serialise the complete metadata object;
2. create `action-journal.json.tmp` in the same directory with mode 0600 and write the bytes;
3. `fsync` the temporary file;
4. `rename` it over `action-journal.json`;
5. `fsync` the parent directory, so the rename itself is durable;
6. only now treat the update as committed.

`fs_atomic::write_atomic` already does the temporary file and rename; extend it (or add a
`write_atomic_durable` used only by the journal) to `fsync` the directory after the rename, without
changing the behaviour of existing callers. A metadata update that fails at any step is not committed:
report the error and keep the previous file.

**Corrupt metadata.** Missing, malformed, unsupported `format`, wrong field type, a `pruned` value that
is not a boolean, or unreadable bytes are corruption:

- fail closed exactly as 10.6 describes: refuse mutations with `refused{code: journal_corrupt}`,
  keep reads working, and require the explicit local reset;
- answer `outcome_unknown{journal_lost}` for every absent execution id, because the pruning marker
  cannot be trusted;
- never regenerate the epoch, never clear `pruned`, never fall back to an in-memory default, and never
  delete the file.

A fresh install is the only case where a new metadata file and epoch are created: both files absent,
or the journal file absent or zero-length with no records to lose. A missing metadata file beside a
non-empty journal is corruption, not a new epoch (10.6).

### 10.2 Epoch

- A random UUIDv7 created with the journal, persisted in the meta file, reported as `hello.journal_epoch`
  and echoed by every `action_status` answer.
- It changes only when the journal is explicitly reset (`domovoi agent journal reset`) or irrecoverably
  replaced by that same local action.
- It never changes on restart, upgrade, corruption or a failed write.
- It lives only in `action-journal.json` (10.1). Missing or corrupt metadata never produces a new
  epoch; only the explicit local reset does.
- A query whose `expected_epoch` differs from the journal's is answered `outcome_unknown{journal_lost}`
  for every id. It is **never** `not_found`.

### 10.3 Records

Four record kinds, each fsynced before the next step proceeds:

| Record | Written | Fields |
|---|---|---|
| `accepted` | before replying to the hub and before spawning | `execution_id`, `contract`, `version`, `target`, `plan_hash`, `authorization_kind`, `wall_time`, `boot_id`, `mono_ns`, `mutating: bool` |
| `started` | immediately after spawn, before the child may be reported running | `execution_id`, `pid`, `proc_start_time` |
| `finished` | when the child completes, before the reply is sent | `execution_id`, `result` |
| `abandoned` | on restart, when a recorded pid still matches a live process and its group is killed | `execution_id`, `pid`, `reason` |

Ordering rules:

- `accepted` durability is the acknowledgement. If the fsync fails, do not reply, do not spawn, and
  return an error frame.
- A duplicate `execution_id` never spawns again. Look it up first; answer from the record set:
  `finished(result)`, `running`, `outcome_unknown{...}`. A duplicate that arrives while the original is
  still `accepted`-only but held by this same process is `running` only if this process holds the child
  handle; otherwise it is `outcome_unknown{no_start_record}`.
- `finished` is durable before the reply is written. A reply that was lost is recoverable from the
  journal; a result that was never recorded is not a result.
- Never infer success from a process exit. Only a durable `finished` record is success.
- Never infer `NotRun` from an empty journal unless the epoch matches and the journal has never pruned
  (10.6).

### 10.4 `action_status` semantics

| Journal state | Same process | After restart | Answer |
|---|---|---|---|
| `finished` | any | any, record present | `finished(result)` |
| `accepted`, no `started` | holds child? no | yes | `outcome_unknown{no_start_record}` |
| `started`, no `finished` | holds the child handle | - | `running` |
| `started`, no `finished` | - | yes | pid check, then `outcome_unknown{agent_restarted}` |
| absent, epoch matches, journal has never pruned | - | - | `not_found` |
| absent, epoch matches, journal has pruned | - | - | `outcome_unknown{journal_lost}` |
| absent, epoch differs | - | - | `outcome_unknown{journal_lost}` |

- `running` is never returned after a restart, even when the recorded pid is alive.
- An unknown id and an id outside retention are different answers. Collapsing them into `not_found`
  would turn "may have run" into "never ran", which is the exact failure this prompt exists to prevent.
- `not_found` is positive evidence: the request never reached this journal.
- The only positive evidence available today is "epoch matches and this journal has recorded every
  acceptance since it was created". That holds only while the journal has never pruned. See 10.6 and
  G13: the design provides no shared watermark that could prove membership for a hub-generated id
  after pruning, so a pruned journal answers `outcome_unknown{journal_lost}` for every absent id.

### 10.5 Crash windows

Reproduce `04 15.2` in full, including its **Hub outcome** column. The agent's answers and the hub
column are the contract: prompt 20 implements the agent column, prompt 21 consumes the hub column, and
prompt 46 owns row 8. Do not replace any expected behaviour with a different answer.

| # | Scenario | Agent journal after restart | `action_status` answer | Hub outcome |
|---|---|---|---|---|
| 1 | `accepted` durable, crash before launch | `accepted` only | `outcome_unknown{no_start_record}` | OutcomeUnknown (indistinguishable from #2) |
| 2 | launched, crash before `started` durable | `accepted` only | `outcome_unknown{no_start_record}` | OutcomeUnknown |
| 3 | running, agent crashes | `accepted`, `started{pid,t0}` | agent checks pid + start time: alive orphan -> kill group, record `abandoned`; either way `outcome_unknown{agent_restarted}` | OutcomeUnknown |
| 4 | process finished, crash before `finished` durable | `accepted`, `started` | same as #3 | OutcomeUnknown |
| 5 | `finished` durable, hub lost reply | `finished{result}` | `finished(result)` | result committed |
| 6 | hub received result, crashed before commit | `finished{result}` | `finished(result)` (within retention) | result committed |
| 7 | journal corrupt, replaced (new epoch) or id older than retention | - | `outcome_unknown{journal_lost}` / epoch mismatch | OutcomeUnknown |
| 8 | agent restarts with active containment | containment records | agent lists nft elements, reconciles, reports active set | hub updates containment state; kernel timeouts unaffected |
| - | request never reached agent | no record, epoch matches | `not_found` | NotRun |

- Row 8 is the expected contract, not work in this prompt. Prompt 46 owns containment reconciliation
  and its test; prompt 20 neither implements nor tests it.
- Row 6's "within retention" means the `finished` record is still present. If pruning removed it, the
  answer is `outcome_unknown{journal_lost}` and the hub outcome is OutcomeUnknown (G13); that is the
  conservative reading of the same row, not a different behaviour.
- The last row is `not_found` only while the journal has never pruned (10.6); after pruning it becomes
  `outcome_unknown{journal_lost}` and the hub outcome becomes OutcomeUnknown.

`04 15.2` adds the precise meaning of `running`, and it applies here unchanged: `running` is returned
only by the agent instance that started the execution and still holds its connection to the skill
runner or helper. When the agent restarts, the skill runner sees EOF and kills the skill's process
group (RW-39), so under D-29 the orphan case in row 3 cannot occur for skills; the pid check remains
for legacy root installs.

### 10.6 Persistence, framing and recovery

The design fixes durability and the framing properties, not the bytes. Use this format:

```
record := u32_be(payload_len) || sha256(payload)[32] || payload
payload := UTF-8 JSON object with "kind" and "format": 1
```

- Maximum `payload_len`: 1 MiB. A declared length above that is corruption, never an incomplete frame.
- Append with one write per record, then `fsync` the file before the step that depends on it. Keep a
  single writer; the journal is not shared across tasks without a mutex.
- Startup parses sequentially. A frame is either physically complete (the declared payload is fully
  present) or it is not. Two outcomes follow:
  - **Physically incomplete final frame.** Only these may be discarded: fewer than 4 bytes remain for
    the length prefix, fewer than 32 bytes remain for the checksum, or fewer bytes remain for the
    payload than the declared length. Truncate to the last complete frame and continue. That is safe
    because `accepted` is fsynced before the reply and the spawn, so a record that was never fully
    written was never acknowledged or started.
  - **Everything else is corruption**, including a complete final frame whose SHA-256 does not match.
    A complete frame with invalid JSON, an unsupported `format`, a missing or unknown `kind`, invalid
    fields, or an ordering that breaks the lifecycle is corruption too. Never truncate a complete
    record, including a final one: the bytes are all there, so a mismatch means damage, not a torn
    write.
- Corruption fails closed:
  - stop accepting mutations and answer `refused{code: journal_corrupt, detail}`;
  - keep serving read-only requests;
  - report the journal state in `agent_status`/`hello` (add a `journal_state` field only if RW-42
    already implies one; otherwise report it through the existing `refused` detail and logs, and record
    the choice);
  - never auto-repair, never truncate at or before a complete frame, and never start a new epoch;
  - recovery requires the explicit local `domovoi agent journal reset` (11.6).
- Retention: keep at least 10 000 executions and never drop an unresolved
  `accepted`-without-`finished` set. Pruning is allowed, but it permanently sets `meta.pruned` to
  `true` until a reset, because a pruned journal can no longer prove that a hub-generated id was never
  accepted.

**Pruning ordering (crash-safe).** One `Mutex<JournalState>` guards every journal operation, reads and
writes. `action_status`, acceptance, recovery and pruning all take that same mutex; there is no second
lock and no lock-free read path. Pruning holds it for the whole sequence below, so an `action_status`
query cannot observe a state where records have gone but `pruned` is still false.

1. Acquire the journal mutex.
2. Re-read the metadata from disk, or use the in-memory copy that was loaded under the same mutex.
3. If `pruned` is already `true`, skip the marker write.
4. Otherwise write `meta.pruned = true` with the complete metadata object, using the atomic, durable
   procedure in 10.1 (temporary file, `fsync`, rename, directory `fsync`).
5. Do not delete, truncate or compact a single record until step 4 has returned after the directory
   `fsync`. A failure at any point in step 4 aborts pruning: keep every record, keep the previous
   metadata, and report the error. Never delete first and mark afterwards.
6. Only now remove records, by rewriting the kept records into a temporary journal file, `fsync`ing it,
   renaming it over `action-journal.bin`, and `fsync`ing the directory. A crash before the rename
   leaves the old file intact; a crash after it leaves the kept records.
7. Release the mutex.
8. `meta.pruned` stays `true` for the life of this epoch. Only `domovoi agent journal reset` (11.6)
   starts a new epoch with `pruned = false`.

Crash semantics: a crash after step 4 and before step 6 completes leaves `pruned = true` with some or
all records still present. That is safe and intended: the next startup answers
`outcome_unknown{journal_lost}` for every absent id, even though nothing was removed. A little
unnecessary `OutcomeUnknown` is acceptable; a false `NotRun` is not. A restart in that state never
clears the marker; only the local reset does.

`action_status` and pruning share the mutex, so the last observable states are only "not pruned" or
"pruned", never a mix. If a query is already holding the mutex, pruning waits for it, and the query
sees the pre-prune journal; if pruning holds it, the query sees the post-prune state. Both are
consistent.

- `not_found` requires a matching epoch and `meta.pruned == false`. In that state the journal has
  recorded every acceptance since its creation, so an absent id is positive evidence that the request
  never arrived. Once `pruned` is true, every absent id with a matching epoch answers
  `outcome_unknown{journal_lost}`; an absent id with a different epoch does the same.
- Do not infer retention membership from wall time, monotonic time or UUID ordering. Execution ids are
  generated by the hub, the epoch and any agent-side watermark are generated by the agent, and the
  contracts guarantee neither synchronised clocks nor a shared generation order. See G13: if a future
  contract supplies a hub-generated retention watermark, this rule may be relaxed; until then, no such
  proof exists and the conservative answer stands. If a test or a reviewer asks for `not_found` after
  pruning, stop and report rather than weakening this rule.
- A record's `boot_id` mismatch on the same `mono_ns` is normal after a reboot; it never invalidates a
  record.

### 10.7 Recovery and orphans

- On startup, for each `started` without `finished`:
  1. read `/proc/<pid>/stat` field 22 (`starttime`, in clock ticks since boot);
  2. if it exists and equals the recorded `proc_start_time`, the process group is the original one:
     kill the group (`SIGTERM`, then `SIGKILL` after 2 s), append `abandoned`;
  3. otherwise, PID reuse or a dead process: append nothing and report `outcome_unknown{agent_restarted}`.
- A pid that exists but whose start time differs is **not** the original execution. Never kill it.
- Under the shipped systemd unit the cgroup already kills leftovers; the pid path exists for manual runs
  and legacy root installs. Do not claim the skill-runner EOF guarantee exists: the runner is 47.
- Containment records are not reconciled here (row 8). Leave the journal kind space for them and do not
  claim their reconciliation.

## 11. Agent mutation rate limiter

Separate from the unattended ceiling. Applies to **every** mutating execution, human- or
policy-labelled. Read-only contracts are not counted.

### 11.1 Configuration

```toml
[mutation_limits]
per_hour = 30
per_capability = { "container.restart" = 6 }
```

- Add `MutationLimits` to `AgentSection`, keeping `deny_unknown_fields` and the generated schema.
- `per_hour` defaults to 30; `0` means "no mutations at all" and must be honoured, not treated as
  unlimited.
- `per_capability` is optional, a map of contract id to per-hour limit; a value of `0` blocks that
  contract.
- The file is local and root-owned (0644). The hub cannot send, widen or override any of it. There is
  no `config_update` field for it.
- Do not reuse `[unattended]` or `[policy] allow_risk`; they mean different things.

### 11.2 Counted entries

Every `accepted` record for a mutating contract already carries `execution_id`, `wall_time`, `boot_id`,
`mono_ns` (`03 RW-38`, and section 10.3 above). The limiter reads the journal, not a second store, so
a crash cannot desynchronise the count from the record.

`mono_ns` must survive process restart on the same boot. `std::time::Instant` cannot. Use the existing
`HostFs` port to read `/proc/uptime` (seconds with 0.01 resolution) and convert to nanoseconds. That is
monotonic across processes, resets at boot, and is already how the agent reads `/proc`. If 10 ms
resolution proves insufficient for a test or for `retry_after`, stop and report before adding a
dependency; the `nix` workspace dependency would need its `time` feature, which is a change to
`[workspace.dependencies]` and needs justification.

### 11.3 Age, reboot and duplicates

- Same `boot_id`: age = `mono_now - mono_ns`. Negative age is corruption of the clock, not a fresh
  window; treat it as age 0 and log once.
- Different `boot_id`: age is unknown. The entry counts until the current boot's monotonic clock shows
  one full window elapsed **since the agent first loaded it after this boot**. That load time is
  `meta.boot.first_load_mono_ns`, written with `meta.boot.boot_id` exactly once per boot (10.1), so a
  restart during the window does not reset it. An ordinary process restart on the same boot finds the
  stored `boot_id` equal and leaves both fields untouched. A failed or corrupt metadata write is
  corruption: no mutation proceeds and no absent id becomes `not_found`.
- A reboot therefore never opens an unrestricted window, and a pre-reboot entry is retained for at
  least a full window.
- Wall time is diagnostics only. Roll it back, forward or freeze it: the limiter does not move.
- Counting is by distinct `execution_id`. A re-sent request counts once, because the duplicate is
  answered from its record and never becomes a new `accepted`.

### 11.4 Limit interaction and `retry_after`

The design does not settle this (G6). Use this narrow rule:

1. Both limits are checked. The effective allowance is the stricter of `per_hour` and
   `per_capability[contract]`.
2. The refusal names the binding limit: `scope = capability{contract}` when a per-capability limit is
   what the request would exceed, otherwise `scope = global`.
3. `retry_after` is the seconds until the oldest counted entry that blocks the request leaves the
   window, rounded up, minimum 1.
4. For an earlier-boot entry the leave time is "first load after this boot + one window", so
   `retry_after` is bounded by one window and never 0.
5. A refusal happens before `accepted` is written and before any spawn. The refused request must not
   appear as accepted, started or finished. It may be logged.

### 11.5 Corruption

- Only a physically incomplete final frame is discarded (10.6). That is the tail a crash can leave.
- A complete final frame whose checksum does not match is corruption, not a torn tail. Never truncate
  it. A complete frame with invalid JSON, an unsupported `format` or invalid fields is corruption too.
- Any such corruption fails closed: every mutating request is refused
  (`refused{code: journal_corrupt, detail}`), read-only requests still work, and no reset happens
  automatically.
- The condition is reported to the hub in the refusal detail and in local logs. Do not invent a new
  wire field for it unless section 8's `agent_status` already has a natural place; record the choice.

### 11.6 Explicit reset

Add `domovoi agent journal reset` to the agent binary:

- it runs locally, under the owner privilege that owns `state_dir`;
- it writes a new journal with a new epoch, leaving the old file renamed for local forensics rather
  than deleting it silently;
- it logs the action locally with the old and new epoch;
- it reports the new epoch to the hub through the next `hello`/`agent_status`, not through a new
  hub-controlled message;
- it never runs automatically and is never triggerable from the hub;
- it is unrelated to the helper's key rotation or recovery actions (47).

Resetting can at most let one window's allowance be used again, and only by a local owner action. It
never removes the limit.

## 12. Contract gaps recorded by this brief

Each item below is a real gap between `03`/`04` and the current source. The decision column is the
narrow choice this brief makes so implementation can proceed; the last column is what the
implementer must do if the choice does not hold.

| ID | Gap | Sources | Decision here | Stop and report if |
|---|---|---|---|---|
| G1 | `run_capability` result shape and all new result payloads are unspecified | `03 RW-42`, `04 12` | 7.3's `CapabilityResult` with `outputs: BTreeMap<String, String>` | 24's contract needs typed non-string outputs and cannot express them as canonical strings |
| G2 | `args`, `target`, `contract`, `version` encodings are unspecified | `03 RW-42` vs `work.rs` `RunSkill` | `BTreeMap<String, String>`, `TargetKey`, `String` (`<family>.<verb>`), `u32` | an existing type cannot represent a required target (for example a CIDR) without change |
| G3 | `Open<T>` does not exist; which enums are open is unstated | `03 RW-17`, `03 RW-42` | 9.3: open data enums, closed authority/state enums | a closed enum later needs to accept values from a package |
| G4 | journal framing, retention bound and compaction are unspecified | `03 RW-09`, `03 RW-38` | 10.6: length + SHA-256 framing, 10 000 executions, a permanent `pruned` flag | 10 000 is too small for a real host, or retention cannot avoid dropping unresolved records |
| G5 | no monotonic nanosecond source that survives a process restart | `03 RW-38` | `/proc/uptime` through `HostFs` | 10 ms resolution breaks a test or a `retry_after` bound |
| G6 | `per_hour` vs `per_capability` aggregation and `retry_after` are unspecified; RW-42's `refused` has no `scope` while RW-38 requires one | `03 RW-38`, `03 RW-42` | 11.4, plus `scope: Option<RefusalScope>` | the owner states a different aggregation rule |
| G7 | `retry_after` for earlier-boot entries is unspecified | `03 RW-38` | 11.4.4 | an entry's age cannot be bounded to one window |
| G8 | RW-42 lists `hello.boot_id` although `HostFacts.boot_id` already exists | `03 RW-42`, `handshake.rs:65` | reuse `HostFacts.boot_id`; add no second field | a later consumer needs a boot id outside facts, in which case propose a 1.3 field |
| G9 | RW-42 says `ping` is hub -> agent; 1.1 is agent -> hub | `03 RW-42`, `link/mod.rs:294-322` | 8.7: reverse the direction, both ends keep detection | the change spreads into prompt 14's availability or the presence state machine |
| G10 | RW-42 writes `unsupported{type}`; 1.1 ships `Unsupported{kind}` | `03 RW-42`, `04 12`, `stream.rs:68` | 1.2 uses the wire field `type` (Rust `type_name`); the 1.1 frames become frozen literal fixtures | a canonical document contradicts `type`, or a 1.1/1.2 interop requirement appears (there is none under the exact rule) |
| G11 | RW-42 lists `hello.features` while 1.1 ships `capabilities: Vec<String>` | `03 RW-42`, `handshake.rs:164` | add `features`; leave `capabilities` in place for now, both empty | a consumer requires one field to be removed, which is a 1.3 change |
| G12 | `agent_status`'s "report `journal_corrupt`" has no field | `03 RW-38`, `04 8.3` | report through the refusal detail and logs; no new field | a UI requirement needs it in `agent_status`; then add it to the frozen shape before 1.2 ships |
| G13 | nothing proves a hub-generated execution id is inside the retention window after pruning: ids come from the hub, any watermark comes from the agent, and no shared clock or ordering is guaranteed | `03 RW-09` ("only when the epoch matches and the id is inside the journal retention window"), `04 15.2` | 10.6: `not_found` only while `meta.pruned == false`; after pruning every absent id is `outcome_unknown{journal_lost}` | a later contract supplies a hub-generated retention watermark, or the owner requires `not_found` after pruning |

G13 is the reason this brief never claims retention membership is proven by UUID order or timestamps.
The conservative answer is deliberate: `OutcomeUnknown` is recoverable by a human, a wrong `NotRun` is
not. Do not "fix" it by adding a clock assumption.

Do not resolve any of these by editing `03`, `04`, `04b`, `06`, `07` or `99`.

## 13. Dependencies and integration

All of 13, 14, 15, 16, 17, 18 and 19 must be merged before implementation. Read their merged code, not
this table:

- **13** owns the exact protocol rule and the durable incompatibility report. Do not redo it; change
  only the version constant and the 1.2 types.
- **16** owns signal-spool durability. The journal is a new, separate file; do not change the spool's
  format, caps or tests.
- **14** owns `HostAvailability` and recovery ordering. Consume its API; do not build a parallel
  availability model from `agent_status`.
- **15** owns the hub effects lifecycle with lease fencing. That is hub state, not the agent journal.
  Do not merge the two or reuse its tables.
- **17** carries the integrated LLM reliability work. Do not regress it; this prompt does not touch it.
- **18** owns structured approvals and **19** owns operational readiness. This prompt does not change
  either, but the `refused` shape must remain usable by the approval card 18 introduced.
- **21** depends on this prompt. Do not implement any reconciliation, hello-time recovery or
  `OutcomeUnknown` hub commit here.

Repository rules that matter here:

- `domovoi-proto` stays pure: no I/O, no clock, no UUID creation at decode time.
- Spawning stays exclusively in `domovoi-runner`, with argv vectors, never `sh -c`.
- No new crates. `nix` is already a workspace dependency but not in `domovoi-agent`; prefer
  `/proc/uptime`.
- Time-dependent tests use the `Clock` port and `domovoi-testkit`; no sleeps.
- No `unwrap`, `expect`, `panic!`, `todo!` or `dbg!` in library code; no new `#[allow(...)]`.
- Non-test source files stay at or under 500 lines. The journal and the limiter each deserve their own
  module under `crates/domovoi-agent/src/journal/`.

## 14. Tests

Name tests after the behaviour. Keep the exact names below where they are given by a decision.

### 14.1 Protocol and compatibility

- `current_version_is_one_two`
- `exact_rule_matrix` (extend for 1.2, keep 1.1 cases)
- `older_minor_agent_rejected_with_reason`
- `newer_minor_agent_rejected_with_reason`
- `same_protocol_different_patch_releases_communicate`
- `incompatible_state_survives_hub_restart`
- `unauthenticated_hello_does_not_persist_incompatibility`
- `hello_1_2_round_trips_with_every_extension`
- `agent_status_round_trips_with_every_extension`
- `every_message_kind_matches_its_wire_snapshot` (extend, do not replace)
- `golden_frames_stay_at_1_1`
- `golden_frames_pin_1_2`
- `expected_safety_tagged_variants_roundtrip`
- `malformed_expected_safety_refused`
- `unknown_enum_value_preserved`
- `unsupported_message_does_not_end_session`
- `unsupported_uses_type_field_in_1_2`
- `unsupported_1_1_fixture_is_unchanged`
- `attestation_roundtrip_over_wire`
- `helper_identity_rejects_wrong_key_length`
- `transfer_chunk_rejects_oversized_payload`
- `action_status_rejects_more_than_64_ids`

### 14.2 Journal

- `accepted_is_durable_before_reply_and_spawn`
- `started_is_durable_after_spawn`
- `finished_is_durable_before_reply`
- `duplicate_execution_id_never_spawns_twice`
- `crash_after_accepted_is_outcome_unknown`
- `crash_after_started_is_outcome_unknown`
- `orphan_process_killed_on_agent_restart`
- `finished_durable_reply_lost_reconciles`
- `hub_crash_before_commit_reconciles_from_journal`
- `epoch_change_is_outcome_unknown`
- `not_found_with_matching_epoch_is_not_run`
- `started_without_finished_is_never_running_after_restart`
- `pid_reuse_is_not_the_original_process`
- `journal_retention_expires_into_outcome_unknown`
- `journal_retention_never_drops_unresolved_records`
- `journal_reloads_after_restart`
- `corrupt_journal_refuses_mutations`
- `torn_tail_record_is_discarded_safely`
- `truncated_final_record_is_discarded`
- `complete_final_record_checksum_mismatch_fails_closed`
- `interior_checksum_mismatch_fails_closed`
- `not_found_requires_an_unpruned_journal`
- `not_found_is_unavailable_after_pruning`
- `crash_after_prune_marker_before_record_removal_is_safe`
- `crash_during_record_pruning_after_marker_is_safe`
- `metadata_write_failure_prevents_pruning`
- `action_status_cannot_race_a_prune`
- `restart_after_prune_crash_never_returns_false_not_found`
- `metadata_round_trips_boot_load_state`
- `boot_load_state_survives_process_restart`
- `boot_load_state_written_once_per_boot`
- `metadata_update_preserves_pruned_flag`
- `corrupt_metadata_fails_closed`
- `failed_metadata_write_never_enables_not_found`
- `journal_reset_starts_a_new_epoch`
- `journal_reset_is_local_only`
- `signal_spool_is_independent_of_the_action_journal`
- `existing_spool_tests_still_pass` (run, do not rename)

### 14.3 Mutation limiter

Keep the exact RW-38 names:

- `rate_limit_survives_hub_restart`
- `rate_limit_holds_across_reboot`
- `wall_clock_rollback_does_not_reset_limit`
- `duplicate_execution_counts_once`
- `corrupt_journal_refuses_mutations`
- `torn_tail_record_is_discarded_safely`

`torn_tail_record_is_discarded_safely` is RW-38's name for the physically incomplete final frame. The
three framing tests in 14.2 are the finer-grained cases: truncated tail discarded, complete checksum
mismatch refused, interior checksum mismatch refused.

Add:

- `default_limit_is_thirty_per_hour`
- `per_capability_limit_is_stricter_than_global`
- `limit_applies_to_human_and_policy_authorization`
- `read_only_execution_is_not_rate_limited`
- `rate_limited_request_is_not_accepted_or_started`
- `refusal_reports_scope_and_retry_after`
- `retry_after_is_bounded_for_earlier_boot_entries`
- `limit_is_not_configurable_from_the_hub`

### 14.4 Gate and harness rules

- Use a fault-injecting journal and a fake runner that can crash between steps, as `04 15.2` requires.
- Extend `crates/domovoi-agent/tests/gates.rs`; never weaken it.
- Review every accepted `insta` snapshot. A 1.1 fixture must not be edited to look like 1.2.
- If a public protocol type changes, run `cargo nextest run -E 'rdeps(domovoi-proto)'`.

## 15. Implementation order

1. `domovoi-proto`: `Open<T>`, `Base64Bytes`, the new message types, the version constant.
2. Protocol tests: 1.1 fixtures untouched, new 1.2 samples and snapshots, round-trip and tolerant
   decoding tests.
3. `domovoi-config`: `[mutation_limits]` and its schema test.
4. `domovoi-agent`: journal module (epoch, framing, recovery, retention) with unit tests that do not
   need a hub.
5. `domovoi-agent`: journal-backed acceptance in the request handler, `action_status`, duplicate
   handling.
6. `domovoi-agent`: mutation limiter and the reset command.
7. `domovoi-hub`: session handling for the new frames, the reversed ping, and the minimum bookkeeping
   needed to keep the workspace compiling.
8. `hello`/`agent_status` population in the agent, with the honest `absent`/`None` values.
9. Architecture check, clippy, the full crate tests, and the documented checks.

Do not reorder the journal before the wire: the 1.2 types are what the journal's status answers use.

## 16. Checks

Run exactly these, and put the real output in the completion report:

- `cargo fmt --all`
- `cargo clippy -p domovoi-proto --all-targets -- -D warnings`
- `cargo clippy -p domovoi-agent --all-targets -- -D warnings`
- `cargo clippy -p domovoi-config --all-targets -- -D warnings`
- `cargo clippy -p domovoi-hub --all-targets -- -D warnings`
- `cargo nextest run -p domovoi-proto` (or `cargo test`)
- `cargo nextest run -p domovoi-agent`
- `cargo nextest run -p domovoi-config`
- `cargo nextest run -p domovoi-hub`
- `cargo nextest run -E 'rdeps(domovoi-proto)'`
- `cargo xtask arch-check` (new modules)
- `cargo xtask docs-links` if any book or reference page is touched
- `cargo xtask openapi && cd web && npm run gen:api` only if a hub API type changed; if it did, say
  why, because this prompt should not change the HTTP API.

CI runs the full suite; do not run the whole workspace locally by default.

## 17. Completion criteria

- `PROTOCOL_VERSION` is `1.2`, the exact rule is unchanged, and the 1.1 fixtures still pass as history.
- Every message family in `03 RW-42` exists, round-trips, and has a 1.2 golden frame.
- 1.2 `unsupported` carries `type` on the wire, and the 1.1 fixtures are literal JSON that the type
  changes did not touch.
- `hello` and `agent_status` carry every extension with honest values: `absent`, `None`, empty, and
  `runs_as_root` from the real uid.
- The journal is durable, framed, epoch-scoped, idempotent and restart-safe, and every crash window in
  `04 15.2` (except row 8) has a passing named test. The full five-column table is reproduced in
  10.5; row 8's implementation and test belong to prompt 46.
- The metadata file carries `format`, `epoch`, the sticky `pruned` marker and the current boot's
  `boot_id` + `first_load_mono_ns`. Every update uses the one atomic, durable procedure and preserves
  `pruned`; corrupt metadata fails closed and never regenerates the epoch.
- Pruning durably sets `pruned = true` before any record is removed, under the journal mutex, and a
  crash at either point never produces a false `not_found`. No record path can remove records while
  the marker is still false on disk.
- Only a physically incomplete final frame is truncated. A complete frame with a bad checksum, invalid
  JSON, an unsupported format or invalid ordering fails closed and needs a local reset, at the end of
  the file as anywhere else.
- `action_status` never turns "may have run" into `not_found`: absent ids are `not_found` only with a
  matching epoch and an unpruned journal, and `outcome_unknown{journal_lost}` otherwise. No UUID
  ordering or clock assumption is used as a retention proof.
- `RefusalCode` contains `JournalCorrupt`, and every corruption path emits
  `refused{code: journal_corrupt, detail}`.
- The mutation limiter holds across restart, reboot and wall-clock rollback, fails closed on corruption,
  and is reset only by the local owner action.
- No hub reconciliation, capability registry, policy engine, helper, containment or install workflow
  has been implemented.
- No new crate, no new migration, no new dependency without the justification section 11.2 requires.
- Every gap in section 12 is either implemented as decided or reported as still open.

## Execution constraints

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

## Repository rules

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
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
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

## Completion report

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
