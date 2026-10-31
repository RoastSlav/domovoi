# P02 — Core model II: skills, actions, approvals, triage, cards, digests

| | |
| --- | --- |
| Phase | 1, Core contracts |
| Depends on | P01 |
| Crates | `domovoi-core` only |
| Size | L |
| Invariants touched | INV-1, INV-4, INV-6, INV-9 (type-level foundations) |

## Goal

Define everything that flows out of an incident: skill manifests and argument validation, the action state machine with plan hashes and human-only approval, the closed read-tool catalogue the model may use, triage reports, cards, digests, notes, acks, maintenance windows, baselines, advisories and chat interactions. Pure data and pure functions only.

## Read first

- `ARCHITECTURE.md` §5.4–§5.7, §11, §12, §15
- ADR 0004, ADR 0007
- P01 code in `crates/domovoi-core`

## In scope

Add `regex` (on the allowlist) to `domovoi-core`.

1. **`skill/` (`manifest.rs`, `params.rs`, `validate.rs`)**
   - `SkillName`: `^[a-z][a-z0-9_]{0,63}$`.
   - `Risk { Read, Write, Destructive }` (ordered).
   - `ParamKind`: `String { pattern: String, max_len: u16 }`, `Integer { min: i64, max: i64 }`, `Boolean`, `Enum { values: Vec<String> }`. `ParamSpec { name, kind, description, required: bool (default true) }`. Param names `^[a-z][a-z0-9_]{0,31}$`.
   - `SkillManifest { name, description, risk, entry: String (relative path inside the skill folder), params: Vec<ParamSpec>, timeout: SignedDuration, run_as: Option<String>, hosts: Vec<HostName> (empty = any), applies_to: Vec<CheckKey>, verify: Option<VerifySpec>, revert: Option<RevertSpec>, max_output_bytes: u32 }` with serde matching the `skill.toml` format in `ARCHITECTURE.md` §5.6 (durations as strings like `"90s"` via jiff's friendly format).
   - `VerifySpec { recheck: bool, delay: SignedDuration }` (v0.1: re-run the originating check after `delay`). `RevertSpec { entry: String, confirm_window: SignedDuration }`.
   - `SkillManifest::validate(&self) -> Result<(), Vec<ManifestError>>`. Rules: valid names; unique param names; `String` patterns compile, are anchored (`^…$`) and `max_len` 1..=1024; `Integer` min ≤ max; `Enum` non-empty, values unique; timeout 1 s..=1 h; `max_output_bytes` 1 KiB..=1 MiB; `entry` is relative, has no `..` component and no leading `/`; `entry` file name is not a shell (`sh`, `bash`, `dash`, `zsh`, `fish`, `ash`, `ksh`); `revert` only for `Write`; `Read` skills have no `verify` or `revert`.
   - `SkillManifest::manifest_hash()` → `ManifestHash` (SHA-256 of the canonical JSON serialization).
   - `SkillManifest::validate_args(&self, args: &BTreeMap<String, String>) -> Result<ValidatedArgs, ArgError>`: every required param present, no unknown params, string values match their pattern fully and `max_len`, integers parse within range, booleans are `true`/`false`, enums match exactly. `ValidatedArgs` has a private field; the only constructor is `validate_args`. It exposes `iter()` in declared param order and `canonical()` (a stable string for hashing).
   - `SkillSummary { name, description, risk, params, applies_to, manifest_hash }`: what the hub sees of a manifest. `SkillSummary::validate_args` delegates to the same logic.
2. **`action/` (`request.rs`, `approval.rs`, `transition.rs`)**
   - `PlanHash`: SHA-256 over `"plan:v1\n{skill}\n{manifest_hash}\n{host_id}\n{validated_args.canonical()}"`.
   - `ApprovalChannel { Web { typed_confirmation: bool }, Chat { platform: String, platform_user: String } }`.
   - `HumanApproval { user: UserId, channel: ApprovalChannel, at: Timestamp }` with `pub fn new(user, channel, at) -> Self`. Its doc comment must say: "Construct only in `crates/domovoi-hub/src/auth/` after authentication. `xtask arch-check` enforces this (INV-1)." No `Default`, no `Deserialize`.
   - `Principal { Human(UserId), System(&'static str) }`.
   - `ActionStatus { Proposed, Approved, Denied, Expired, Dispatched, Rejected { reason }, Succeeded, Failed { reason }, Verified, VerifyFailed }`.
   - `ActionRequest { id, incident: IncidentId, host: HostId, skill: SkillName, risk, args: BTreeMap<String, String>, plan_hash, status, proposed_by: Principal, approved_by: Option<HumanApproval>, expires_at, created_at, updated_at, chat_token_hash: Option<[u8; 32]> }`.
   - Transitions as methods returning `Result<ActionChange, ActionError>`, implementing the diagram in `ARCHITECTURE.md` §5.6:
     - `approve(&mut self, approval: HumanApproval, now)`: only from `Proposed`; `now < expires_at`; **if `risk == Destructive`, the channel must be `Web { typed_confirmation: true }`** (INV-6), otherwise `ActionError::DestructiveNeedsTypedConfirmation`.
     - `deny`, `expire(now)`, `dispatch(current_plan_hash)` → `Rejected` if the hash differs (INV-4), `complete(Result<(), String>)`, `verified(bool)`.
   - There is no method that approves without a `HumanApproval`.
3. **`triage/` (`tools.rs`, `report.rs`)**
   - `ReadTool`: closed enum `DockerPs, DockerLogs, DockerInspect, DockerEvents, HostOverview, Journal, ListeningPorts, SmartReport, IncidentHistory, NotesSearch, RunReadSkill` with `name()` (snake_case), `description()`, `ReadTool::ALL: [ReadTool; 11]`.
   - `ReadToolCall`: one variant per tool with typed, bounded arguments, e.g. `DockerLogs { container: String, lines: u16 (≤ 500) }`, `Journal { unit: Option<String>, since_minutes: u32 (≤ 1440), lines: u16 (≤ 500), kernel: bool }`, `RunReadSkill { skill: SkillName, args: BTreeMap<String, String> }`. `ReadToolCall::from_json(tool: &str, args: serde_json::Value) -> Result<Self, ToolArgError>` rejects unknown tools and out-of-range values. `ReadTool::json_schema(&self) -> serde_json::Value` returns the schema the model sees.
   - `EvidenceRef { label, value, source }`, `ActionProposal { skill: SkillName, args: BTreeMap<String, String>, reason: String }`.
   - `TriageReport { headline (≤ 120), summary, cause, evidence: Vec<EvidenceRef> (≤ 6), verdict, next_step (≤ 300), proposals: Vec<ActionProposal> (≤ 3), details: String }` with `validate()` and `serde(deny_unknown_fields)`.
   - `TriageBudget { max_tool_calls: u16, max_seconds: u32, max_output_tokens: u32 }`, `TriageOutcome { Report(TriageReport), Cached { report, from: IncidentId }, Skipped { reason }, Failed { reason } }`.
4. **`card.rs`**: `CardBadge { Verdict(Verdict), Severity(Severity) }`; `CardButton { Ack, Mute1h, Details, Approve { action: ActionId, token: String }, Deny { action: ActionId, token: String }, OpenInWeb { action: ActionId } }`; `Card { incident, host: HostName, badge, headline, bullets: Vec<String> (≤ 3), next_step: Option<String>, buttons, details: Option<String>, resolved: bool }`. No platform-specific fields (ADR 0010).
5. **`digest.rs`**: `DigestFact { host, key, label, value: f64 | text, unit: Option<String>, observed_at }`, `Trend { key, delta, period: SignedDuration, projection: Option<String> }`, `HostDigest { host, quiet: bool, lines: Vec<String>, trends: Vec<Trend> }`, `Digest { date: jiff::civil::Date, hosts, open_incidents: Vec<(IncidentId, String)>, advisories: Vec<Advisory> }`.
6. **`note.rs`**: `Note { id, host: Option<HostId>, title, body_markdown, tags, updated_at }`.
7. **`ack.rs`**: `EvidencePredicate { Equals { key, value }, LessOrEqual { key, value: f64 }, GreaterOrEqual { key, value: f64 }, Contains { key, text } }`; `AckRule { id, host: Option<HostId>, check: CheckKey, target_pattern: Option<String> (`*` wildcard only at start or end), predicates: Vec<EvidencePredicate>, note, expires_at: Option }` with `matches(&Signal, now) -> bool`. `MaintenanceWindow { host, starts_at, ends_at, note }` with `covers(now)`.
8. **`baseline.rs`**: `BaselineKind { Container, Unit, Port }`, `Criticality { Critical, Warning }`, `BaselineEntry { host, kind, key, criticality, learned_at, last_seen_at, managed: bool }`.
9. **`advisory.rs`**: `AdvisoryKind { HeartbeatMissing, TriageDegraded, StaleImage, StackWithoutSource, SecretExposed, BaselineEntryExpired, AgentOutdated }`, `Advisory { id, kind, host: Option, title, detail, created_at, resolved_at }`.
10. **`interaction.rs`**: `InteractionKind { Button { button: CardButton, incident: IncidentId }, ThreadMessage { incident, text }, Command { name, args } }`, `Interaction { platform, platform_user, kind, received_at }`, `InteractionReply { Accepted { message: Option<String> }, Rejected { reason: String } }`. No authorization data.

## Out of scope

- Ports and traits (P03), wire format (P05), any execution, any persistence.
- Rendering cards or digests to text (P12).

## Tests required

Unit tests next to the code; tests that construct `HumanApproval` go in `crates/domovoi-core/tests/` (the arch-check allows them only in test paths):

1. `manifest_validation_table`: one row per rule above, valid and invalid.
2. `args_reject_injection_strings`: with `pattern = "^[a-zA-Z0-9_.-]{1,64}$"`, values `"plex; rm -rf /"`, `"$(id)"`, `"a\nb"`, `"../../etc"`, `"x".repeat(65)` are rejected.
3. `args_reject_missing_and_unknown_params`.
4. `validated_args_canonical_is_order_independent`: same args inserted in different order → same canonical string.
5. `plan_hash_changes_with_args_manifest_and_host`.
6. Action transitions: one test per arrow, plus `approve_after_expiry_fails`, `destructive_needs_web_typed_confirmation` (chat and web-without-typing both fail), `dispatch_with_different_plan_hash_rejects`.
7. `crates/domovoi-core/tests/gates.rs`:
   - `read_tool_catalogue_is_read_only`: every `ReadTool::ALL` name is in an explicit expected list, and none contains `approve`, `exec`, `write`, `delete`, `restart`, `run_skill`, `shell`, `command`.
   - `read_tool_call_rejects_unknown_tool_names`: `approve_action`, `run_skill`, `execute` all fail `from_json`.
   - `no_approval_without_human`: compile-level documentation test that `ActionRequest::approve` takes `HumanApproval` (assert via a function pointer type).
8. `ack_rule_matching_table` including wildcard targets and expiry.
9. `triage_report_validation_rejects_long_headline_and_too_many_proposals` and `triage_report_rejects_unknown_fields`.
10. Insta JSON snapshots for a full `SkillManifest`, `ActionRequest`, `TriageReport`, `Card`, `Digest`.

## Architecture gates

- `cargo xtask arch-check` (only `regex` added to core's external dependencies).
- `HumanApproval::new` is called only in `crates/domovoi-core/tests/`.

## Acceptance criteria

- `cargo xtask ci` passes.
- A `skill.toml` example in a doc comment on `SkillManifest` deserializes (doctest).
- No public field allows mutating an `ActionRequest`'s status outside its transition methods.

## Deliverables

- `crates/domovoi-core/src/{skill/*,action/*,triage/*,card,digest,note,ack,baseline,advisory,interaction}.rs`
- `crates/domovoi-core/tests/{gates,actions,manifests}.rs` and snapshots

## PR

Title: `feat(core): skills, actions, approvals, triage reports and output models`
