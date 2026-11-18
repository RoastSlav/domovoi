# 41 — Packages: agent-side verification, install and uninstall

| | |
|---|---|
| Branch | `packages-agent` |
| Milestone | P1 |
| Depends on (must be merged) | 20 (merged: wire 1.2, `transfer_*`, `get_catalog`, agent journal), 25 (capability registry; **written but NOT merged when this was written — must be merged first**, and 24 with it). 22 and 23 are merged. |
| Can run in parallel with | 26–40 (order-flexible, never concurrent). 42 (`packages-hub`) must run **after** this prompt. |
| Produces (shared contracts) | `domovoi_core::package::*` (names, `KeyId`, `ArchiveDigest`, signing message, `SignatureFile`, `PackageManifest`, `PackageFailureCode`, list output types, argument and output keys); `domovoi_core::capability::package_contracts()`; agent `[packages]` config and `[agent] packages_dir`; the agent's `transfer_*` receiver; the agent backend of `agent.package.install`, `agent.package.uninstall`, `agent.package.list`; `PackageFixture` in `domovoi-testkit` |
| Consumes | wire 1.2 `transfer_*`, `run_capability`, `capability_result`, `refused`, `get_catalog` (prompt 20, frozen, **not changed here**); the agent journal (20); capability contract and catalogue types (24, 25) |
| Launch gate | none beyond the dependencies |
| Suggested model | large (security-critical: untrusted archives, signature verification, crash recovery) |

## 1. Objective

Make an agent able to **receive a package from the hub, verify it against keys the owner installed on the
host, and install or remove it atomically**, with no way for the hub (or anything on the wire) to widen what
the agent trusts. Concretely, this prompt adds, on the agent side only:

1. A receiver for the 1.2 `transfer_begin / transfer_chunk / transfer_end` frames that stores bytes into a
   private staging area and answers `transfer_result`.
2. Signature, digest and identity verification of a `<name>-<version>.tar.zst` plus a detached
   `<name>-<version>.sig.toml`, against a local key set (first-party release key + owner keys) and a local
   revocation list.
3. A hardened archive audit and extraction (no links, no traversal, caps on size, count, depth, time), an
   atomic rename into `packages_dir/<name>/<version>/`, and a local registry `.packages.toml` that records the
   signer `key_id`.
4. The three capabilities `agent.package.install`, `agent.package.uninstall`, `agent.package.list`, served
   through `run_capability` with the agent journal giving deduplication and crash visibility.
5. Crash recovery of every window between "bytes staged" and "registry written".
6. The `[packages]` section of `agent.toml` (absent = no installs), the `packages_dir` setting, and the
   catalogue entries that advertise the three capabilities only when the owner allows them.

It does **not** build the hub half (index, owner import, package store, relay, jobs: prompt 42), the install
wizard (U4), the helper or skill runner (H1), or any way to run a package's skills. The wire does not change.

## 2. Architectural context

**RW-26 / D-11 / D-16: who is trusted.** The hub can *offer* a package; only the **agent** decides whether it
installs. Trust is exactly two things the owner edits locally in `agent.toml`: the first-party release key and
any owner keys (`[[packages.trusted_keys]]`), minus a revocation list. There is no third-party registry. The
hub cannot add, replace or remove a key: no wire field, capability argument or settings push in this prompt
carries one, and the agent has no code path that mutates the key set at runtime. A malicious hub can withhold
installs or offer packages; it cannot make the agent install unsigned or foreign-signed code.

**Install never enables, never authorizes, never executes.** "Installed" means verified files under
`packages_dir` and a registry row. "Enabled" is a hub decision made in a separate, step-up action (prompt 42
and the existing `host.<name>.skills` setting). "Executable" additionally needs the skill runner (H1, prompt
47). In this prompt a package's skills are **not** added to the agent's `SkillCatalog`, not announced in
`hello.skills`, and not runnable. That keeps INV-3 intact: the hub can only run skills present in the agent's
local catalogue, and package skills are not in it until prompt 47 integrates them behind the runner.

**D-29 / RW-39: identities and where packages live.** Target model: the agent runs as `domovoi-agent`;
packages live in `packages_dir` (default `/var/lib/domovoi-packages`, owned by the agent user, directories
0755, files 0644, entry scripts 0755); skills execute only through the skill runner as `domovoi-skill`, which
can read but not write that tree; the helper and runner units and `/etc/domovoi/*` are root-owned, so the
installer cannot touch them. A capability package **never installs or updates the helper**. Helper
installation is a separate owner-controlled OS step.

**Until H1: the agent may still be root.** Today `install.sh` runs the agent as root (unit `ProtectSystem=full`,
so `/var/lib` is writable). 05 §4a says root installs keep working and are labelled "privilege boundary not in
effect". This prompt handles that as follows, and no more:

- Installs are allowed when `runs_as_root` is true, with **all** the same checks, plus: `packages_dir` must be
  owned by the agent's effective uid and not writable by group or others (a root agent never writes into a
  directory a non-root user controls).
- Nothing from the archive is trusted for ownership or mode: every file and directory is created by the agent
  with an explicit mode (dirs 0755, files 0644, 0755 only when the archive marks the file owner-executable);
  setuid, setgid, sticky bits, ownership, xattrs and timestamps from the archive are ignored.
- Because package skills have no execution path before H1, a package installed by a root agent cannot run as
  root (or as anyone). `agent.package.list` and the install result carry `privilege_boundary` =
  `not_in_effect` when `runs_as_root`, `in_effect` otherwise, so the hub can label it.
- Prompt 47 (installer migration) is what creates `domovoi-agent` and pre-creates `packages_dir`. This prompt
  does not change `install.sh` or any unit. Before that, the agent creates `packages_dir` itself (0755) if its
  parent exists; if it cannot, installs are refused with `packages_dir_unsafe` and a plain explanation.

**Wire and data flow (all of it already in wire 1.2).**

```
hub                                        agent                                   disk
 | transfer_begin{id A, package_artifact,…} -> receiver --(0600 part file)--> .staging/transfers/A.part
 | transfer_chunk{A, seq 0..n}              -> receiver (sequential, sha256 + size counted)
 | transfer_end{A}                          -> receiver --rename--> A.bin ; transfer_result{Stored|Failed}
 | (same three frames for id B, kind package_manifest = the .sig.toml)
 | run_capability{agent.package.install, args{package,version,artifact_transfer=A,signature_transfer=B}}
 |                                          -> journal accept/started -> verify -> audit pass -> extract to
 |                                             .staging/<exec>/tree -> validate -> rename -> registry write
 | <- capability_result{succeeded|failed}   <- journal finished
```

**Meaning of `TransferKind::PackageManifest` (the wire cannot change).** The frozen enum has only
`package_artifact` and `package_manifest`. This prompt fixes: `package_artifact` carries the
`<name>-<version>.tar.zst` bytes; `package_manifest` carries the detached `<name>-<version>.sig.toml` bytes
(the signed manifest of the package: name, version, digest, signer). The package's internal `package.toml`
travels inside the archive. Prompt 42 must send exactly that.

**04 §15.2 analogue.** An install runs *inside* the agent process, not in a child process. A crash mid-install
therefore leaves the journal with `started` and no `finished`, which the hub reads as `OutcomeUnknown`
(`agent_restarted`). Recovery makes the on-disk state converge (section 10), so the hub can retry with a new
execution id and get a correct, idempotent answer.

**Config naming (05 §5/§6 vs 04 §11.2).** 04 §11.2 names the agent switch `allow_install`; 05 §6 lists
`packages.enabled` among the feature switches. They are different layers: **agent** `[packages] allow_install`
is the owner's local gate (this prompt); the hub's `packages.enabled` feature switch is prompt 42's. The agent
section being absent means "no installs" and nothing is advertised.

## 3. Repository context

Verified in this checkout (branch `overhaul-investigation`, HEAD `74764a3`). Re-open each before editing and
report any difference. Migrations end at `0011_plan_binding.sql`; this prompt adds none.

**Wire 1.2 transfer and capability types (frozen, do not edit).**

- `crates/domovoi-proto/src/messages/transfer.rs:10-84`: `TransferBegin { transfer_id: Uuid, kind:
  Open<TransferKind>, size: u64, sha256: Sha256Bytes }`, `TransferKind { PackageArtifact, PackageManifest }`,
  `TransferChunk { transfer_id, seq: u64, bytes: Base64Bytes }`, `TransferEnd { transfer_id }`,
  `TransferResult { transfer_id, outcome: TransferOutcome }`, `TransferOutcome { Stored, Failed { code:
  Open<TransferFailureCode>, detail: String } }`, `TransferFailureCode { ChecksumMismatch, TooLarge, Storage,
  Aborted }`. There is no per-chunk acknowledgement and no abort frame.
- `crates/domovoi-proto/src/bytes.rs:127,134`: `MAX_BASE64_BYTES = 64 * 1024` and `Base64Bytes` (decode refuses
  more than 64 KiB).
- `crates/domovoi-proto/src/messages/actions.rs`: `RunCapability { execution_id: Uuid, contract: String,
  version: u32, target: TargetKey, args: BTreeMap<String, String>, plan_hash: PlanHash, authorization_kind:
  AuthorizationKind, expires_at: Timestamp, expected_safety: Option<ExpectedSafety> }`; `CapabilityOutcome {
  Succeeded { outputs: BTreeMap<String, String> }, Failed { code: Open<CapabilityFailureCode>, message } }`;
  `CapabilityFailureCode::Refused` ("a backend refused after accepting the work"); `Refused { code:
  Open<RefusalCode>, detail, retry_after, scope }`; `RefusalCode` has `ContractUnknown`,
  `ContractVersionUnsupported`, `AuthorizationExpired`, `AllowRiskDenied`, `JournalCorrupt`, `RateLimited`.
- `crates/domovoi-proto/src/messages/catalog.rs`: `GetCatalog`, `CatalogPage`, `CatalogEntry`.
- `crates/domovoi-proto/src/version.rs:17`: `PROTOCOL_VERSION = 1.2`.

**What the agent does today with those frames (the gap this prompt fills).**

- `crates/domovoi-agent/src/link/mod.rs:398-466` (`Link::handle`): `RunCapability` becomes
  `RequestKind::Capability` (`:431`); **there is no arm for `TransferBegin/Chunk/End`**, they fall to `other =>
  { tracing::debug!(... "ignoring a message") }` (`:446`). The roadmap's "transfer handlers present but unused"
  is wrong for the agent: nothing receives them, and the hub (`agent_server/session.rs:220`) only routes an
  incoming `TransferResult` to whoever waits on `reply_to`. File sizes: `link/mod.rs` is 468 lines, so put new
  logic in a new file and add at most ~15 lines to `mod.rs` (R8, 500-line cap).
- `crates/domovoi-agent/src/link/mod.rs:48,50,118`: `REQUEST_QUEUE = 32`, `OUTBOX = 256`, `LinkParts` (a struct
  literal in `runtime.rs:239` and `tests/agent/harness.rs:118`; a new field must be added to both).
- `crates/domovoi-agent/src/requests/mod.rs:208-224`: `run_capability(ctx, run) -> Message` is **sync and
  refuses everything**:

  ```rust
  /// Capabilities have no execution backend or registry in this milestone, so a request is
  /// refused honestly rather than accepted and left without a result.
  fn run_capability(ctx: &RequestCtx, run: &RunCapability) -> Message {
      if ctx.journal.as_ref().is_some_and(|journal| journal.corruption().is_some()) {
          return refused("journal_corrupt", RefusalCode::JournalCorrupt, "...");
      }
      refused("contract_unknown", RefusalCode::ContractUnknown, format!("... {} is unknown", run.contract))
  }
  ```

  `handle` (`:331`) calls it at `RequestKind::Capability(run) => run_capability(ctx, run)`. `requests/mod.rs` is
  464 lines: move capability code into a new `requests/capability.rs`. `RequestCtx` is `:70-101`
  (`journal: Option<Arc<Journal>>` at `:100`); struct literals in `runtime.rs:168` and
  `tests/common/skills.rs:95` need the new field.
- `crates/domovoi-agent/src/journal/mod.rs`: `Journal::accept(&AcceptedRequest, now, boot_id, mono_ns) ->
  Result<AcceptOutcome, JournalError>` (`:304`), `started(execution_id, pid, proc_start_time)` (`:352`),
  `finished(execution_id, CapabilityResult)` (`:382`), `AcceptOutcome { Accepted, Duplicate(ActionStatusAnswer),
  Refused(Box<Refused>) }` (`:57`), `AcceptedRequest { execution_id, contract, version, target, plan_hash: &str,
  authorization_kind, mutating }` (`:68`). `accept` applies the mutation rate limiter only when `mutating`
  and returns `Refused(JournalCorrupt)` when fenced. Nothing in the agent calls `accept/started/finished`
  outside tests yet: this prompt is the first production caller.
- `crates/domovoi-agent/src/journal/recovery.rs:330-360` (`recover_orphans_with`): for a `started` record it
  looks up `pid`; with `pid = 0`, `lookup` is `Gone` (`:358`), `pgid == 0`, so it `continue`s and leaves the
  record unresolved, signalling nothing. `journal/store.rs:348-388` (`answer_locked`) then reports a started,
  unfinished execution that is not in `running_now` as `OutcomeUnknown{agent_restarted}`, and an accepted-only
  one as `OutcomeUnknown{no_start_record}`. That is exactly right for an in-process install: **always call
  `journal.started(execution_id, 0, 0)`**, never the agent's own pid (a recorded own pid could make recovery
  inspect or signal a live group).
- `crates/domovoi-agent/src/boot.rs:38`: `boot_snapshot(&dyn HostFs) -> Result<BootSnapshot { boot_id: Uuid,
  monotonic_ns: u64 }, _>`; `accept` needs both. A failed snapshot means no mutation is accepted.
- `crates/domovoi-agent/src/fs_atomic.rs`: `write_atomic` (`:12`), `DurableFaults` (`:54`),
  `write_atomic_durable(path, bytes, mode) -> Result<(), DurableWriteError { BeforeCommit, Indeterminate }>`
  (`:86`), `sync_dir` (`:138`, `pub(crate)`), `read_optional` (`:153`). Reuse these for the registry.
- `crates/domovoi-agent/src/runtime.rs`: `open_journal` (`:88`), `runs_as_root()` (`:102`, linux `geteuid` via
  `nix`), `load_catalog` (`:111`), `AgentRuntime::new` (`:123`; builds `RequestCtx` at `:168`),
  `run` (`:233`; builds `LinkParts` at `:239`, spawns `serve` at `:271`).
- `crates/domovoi-agent/src/error.rs:143`: `AgentError` (add one variant).
- `crates/domovoi-agent/src/link/handshake.rs:78`: `AgentStatusSource`; `HelloExtensions.catalog_hash`
  (`None` until prompt 25 sets it).
- `crates/domovoi-runner/src/catalog/mod.rs:82,115`: `SkillCatalog::load(dir) -> (Self, Vec<CatalogIssue>)`,
  `summaries()`. `checks.rs:70-135`: a skill folder must hold a parsing, valid `skill.toml` whose `name` equals
  the folder; entry is relative, resolves inside the folder, is a regular executable file and not group/other
  writable; the folder is not world-writable. **Unix only**.
- `crates/domovoi-core/src/skill/manifest.rs`: `SkillManifest` has `run_as: Option<String>`.
- Config: `crates/domovoi-config/src/agent.rs:18` `AgentConfig` (`deny_unknown_fields`), `:42` `AgentSection`
  (`state_dir`, `skills_dir`, `host_root`, `mutation_limits`), `:70` `MutationLimits`, `:265` `AgentDirs` and
  `AgentSection::default_dirs`; validation `validate.rs:295` (`impl Validate for AgentConfig`); the config
  crate's allowed external deps (no `sha2`, `base64`, `hex`) are in `xtask/src/arch/rules.rs`. Reference
  docs: `docs/book/src/reference/configuration.md` (generated by `cargo xtask docs-config`).
- Workspace dependencies (root `Cargo.toml`): `tar = "0.4"`, `flate2`, `sha2`, `hex`, `base64`,
  `ed25519-dalek = "3.0.0"`, `subtle`, `toml`, `nix` (features `signal process user fs`), `tempfile`.
  **There is no zstd crate.** The agent already depends on `ed25519-dalek`, `sha2`, `base64`, `hex`, `toml`,
  `subtle`, `nix` (linux); it does not yet depend on `tar`.
- Core purity: `domovoi-core` may use only `serde, serde_json, thiserror, jiff, uuid, async-trait, sha2, hex,
  secrecy, regex` (`xtask/src/arch/rules.rs:15-31`). No `toml`, `base64` or `ed25519-dalek` there.
- Existing test support: `crates/domovoi-agent/tests/agent/hub.rs:180` `TestHub` (loopback TLS hub the test
  drives frame by frame: `HubConn::send/recv/expect`), `tests/agent/harness.rs` (`start_link`, `RunningLink`),
  `tests/common/skills.rs` (`Fixture`, `install`), `tests/gates.rs` (INV-3/INV-4 gates), testkit `FakeClock`
  (`advance`, `set`), `FixtureHostFs`, `ListRedactor`. Journal tests live inside the crate
  (`src/journal/tests.rs`) and use `Journal::init_unlocked`/`open_unlocked` (`#[cfg(test)]`).

**Planned names from unmerged prompts (introduced by 24 and 25 — verify names in code before use).**

- 24: `domovoi_core::capability::{CapabilityContract, ContractId, ContractVersion, ContractCategory,
  EffectClass, TargetKind, Reversibility, Idempotency, ExecutionPath, VerificationHint, ContractSource,
  ContractIndex}`, `builtin::builtin_contracts()` (exactly the ten read-tool contracts; 24's tests pin that
  count, **do not add to that function**).
- 25: agent-side `crates/domovoi-agent/src/requests/catalog.rs` (`catalog_entries`, `page`), `hello.catalog_hash`
  built from the same list, and the hub-side composition of the `ContractIndex`.

**Discrepancies found while writing this brief (resolved here; report if the code disagrees).**

1. Roadmap/prompt 20 say the agent has `transfer_*` handlers; it has none (above). This prompt adds them.
2. `package.toml` and the package layout are named in 04 §11.2 but never specified. Section 6.5 specifies them.
3. 04 §11.2 says reload by "SIGHUP or restart"; the agent handles no SIGHUP for any config. This prompt reloads
   keys on **restart only** and does not add signal handling.
4. 04 §11.2 says `package.toml` is read by streaming the entry before extraction; this prompt does it as a
   full **audit pass** (nothing written, all limits and shape rules enforced), then a second extraction pass.
5. The package build/sign tooling (who produces `.tar.zst` and `.sig.toml`) is in no prompt. Tests build
   archives with `PackageFixture` (section 6.9). Report this gap; do not invent a CLI.

## 4. Exact scope

Files to **add** (each non-test file ≤ 500 lines; split rather than exceed):

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/package/{mod,names,digest,signature,manifest,failure,output}.rs` | pure shared contract types (6.1) |
| `crates/domovoi-core/src/capability/package.rs` | `package_contracts()`: the three contracts (6.2). Registered in `capability/mod.rs` |
| `crates/domovoi-config/src/packages.rs` | `PackagesSection`, `TrustedKey`, `PackageLimits`, shape validation (6.3) |
| `crates/domovoi-agent/src/packages/mod.rs` | `PackageService` facade, `PackageSurface`, `PackageDeps` |
| `.../packages/keys.rs` | `KeySet` (trusted, revoked, lookup) built once at start |
| `.../packages/error.rs` | `PackageError` (`thiserror`), `code() -> PackageFailureCode` |
| `.../packages/staging.rs` | staging area, staged transfer table, TTL, disk accounting |
| `.../packages/transfer.rs` | `TransferReceiver` state machine (6.4) |
| `.../packages/verify.rs` | signature file parse, trust, signature, digest, identity checks |
| `.../packages/archive.rs` | zstd + tar audit pass, `check_entry`, path rules |
| `.../packages/extract.rs` | extraction pass writing the staging tree |
| `.../packages/layout.rs` | `packages_dir` safety, protected roots, permissions, space probe |
| `.../packages/registry.rs` | `.packages.toml` read and durable write |
| `.../packages/install.rs`, `uninstall.rs`, `list.rs` | the three operations |
| `.../packages/recover.rs` | boot recovery and sweep |
| `.../packages/events.rs` | `PackageEvent`, `EventSink`, `TracingEvents` |
| `.../packages/tests/{mod,crash,transfer,verify,archive,install}.rs` | in-crate tests (`cfg(test)`) |
| `crates/domovoi-agent/src/link/transfers.rs` | link glue: inbox type, forwarding, the "no receiver" reply |
| `crates/domovoi-agent/src/requests/capability.rs` | `run_capability` (async), the journal flow, refusal order |
| `crates/domovoi-agent/tests/agent/packages.rs` | integration tests over `TestHub` and the request handler |
| `crates/domovoi-testkit/src/package.rs` | `PackageFixture`, `BuiltPackage` (new, 6.9) |

Files to **change** (minimal edits): `crates/domovoi-core/src/lib.rs` (`pub mod package;`),
`crates/domovoi-config/src/{lib,agent,validate}.rs` (`packages: Option<PackagesSection>`, `packages_dir`),
`crates/domovoi-agent/src/{lib,error,runtime}.rs`, `.../link/mod.rs` (new `LinkParts.transfers` field and three
match arms), `.../requests/mod.rs` (remove the old `run_capability`, add `packages` to `RequestCtx`, `handle`
awaits the new one), 25's `requests/catalog.rs` (advertise the package contracts, 6.6), the hub-side place
where 25 builds the hub's `ContractIndex` (one added source, 6.2), `crates/domovoi-agent/tests/agent/{harness,
main}.rs` and `tests/common/skills.rs` (new struct fields), `tests/gates.rs`, `crates/domovoi-testkit/{Cargo.toml,
src/lib.rs}`, root `Cargo.toml` (`ruzstd`), `crates/domovoi-agent/Cargo.toml` (`tar`, `ruzstd`), docs (section 15).

## 5. Explicit non-goals

- **No wire change.** Do not edit anything under `crates/domovoi-proto/`. Protocol stays 1.2.
- **No hub work**: no hub package store, index, owner import endpoint, `domovoi package import`, relay,
  install jobs, hub audit rows, hub `packages.enabled`, UI, OpenAPI change. Prompt 42 owns them.
- **No execution of package skills.** Do not add package skills to `SkillCatalog`, `hello.skills` or
  `run_skill`. Do not integrate the skill runner. The method `PackageService::resolve_for_execution` exists for
  prompt 47 to call; nothing in the agent calls it yet.
- **No helper, skill-runner, installer, unit-file or `install.sh` change.** No `domovoi-agent`/`domovoi-skill`
  user creation. No change to `[unattended]`/ceiling code.
- **No package build or signing tool**, no first-party key material in the repository.
- **No SIGHUP handler**, no runtime key mutation, no hub-pushed config for `[packages]`.
- **No version ordering policy**: any version may replace the installed one (rollback is a normal install).
- **No new journal behaviour**: use the journal as it is (`accept/started/finished`); do not change `journal/`.
- `run_as` other than `domovoi-skill` is unsupported (D-31). Package manifests may not set `run_as` (RW-26: packages never run as another identity);
  owner skills in `skills_dir` are untouched.
- Do not weaken `tests/gates.rs`, 24's `read_derived_contracts_are_read_only` gate, or any arch rule.

## 6. Required interfaces and contracts

### 6.1 Core: `domovoi_core::package` (new, pure; **fixed** names and semantics)

```rust
/// `^[a-z][a-z0-9]*(-[a-z0-9]+)*$`, 2 to 48 characters. Error field "package name".
pub struct PackageName(String);        // parse, as_str, Display, serde try_from/into String
/// `MAJOR.MINOR.PATCH` (no leading zeros) with an optional `-` pre-release of `[0-9a-z]+(\.[0-9a-z]+)*`,
/// at most 32 characters, no `+`. Compared as exact text. Error field "package version".
pub struct PackageVersion(String);
/// Exactly 16 lowercase hex characters. Error field "key id".
pub struct KeyId(String);
impl KeyId {
    /// First 16 hex characters of SHA-256 of the 32 raw public-key bytes.
    pub fn of_public_key(public_key: &[u8; 32]) -> KeyId;
}
/// SHA-256 of an archive, hex on the wire (defined with `sha256_newtype!` inside `package/digest.rs`).
pub struct ArchiveDigest([u8; 32]);
impl ArchiveDigest { pub fn from_bytes(bytes: [u8; 32]) -> Self; /* + the macro's as_bytes, Display, FromStr, serde */ }

/// The bytes that are signed: `"domovoi-package-v1\n{package}\n{version}\n{archive_sha256}\n{key_id}\n"`.
pub fn signing_message(package: &PackageName, version: &PackageVersion, digest: &ArchiveDigest, key_id: &KeyId) -> Vec<u8>;

pub const SIGNATURE_FORMAT: u32 = 1;
pub const SIGNATURE_ALGORITHM: &str = "ed25519";
pub const MAX_SIGNATURE_FILE_BYTES: usize = 16 * 1024;
pub const MAX_PACKAGE_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_INSTALLED_PACKAGES: usize = 64;
pub const MAX_SKILLS_PER_PACKAGE: usize = 32;

/// The detached signature file. `#[serde(deny_unknown_fields)]`. Parsed by callers with `toml`.
pub struct SignatureFile { pub format: u32, pub algorithm: String, pub key_id: KeyId,
    pub package: PackageName, pub version: PackageVersion, pub archive_sha256: ArchiveDigest,
    /// Standard padded base64 of exactly 64 bytes; decoded by the agent, not by core.
    pub signature: String }

/// `package.toml` inside the archive. `deny_unknown_fields`.
pub struct PackageManifest { pub format: u32, pub name: PackageName, pub version: PackageVersion,
    #[serde(default)] pub description: Option<String>, pub compat: PackageCompat }
pub struct PackageCompat { pub protocols: Vec<String>, #[serde(default)] pub platforms: Vec<String>,
    pub skill_contract: u32 }
```

The sample files (**fixed**, exactly what `PackageFixture` writes and the parsers accept):

```toml
# <name>-<version>.sig.toml
format = 1
algorithm = "ed25519"
key_id = "a1b2c3d4e5f60718"
package = "nginx-tools"
version = "1.2.0"
archive_sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
signature = "<base64 of 64 bytes>"

# package.toml (first-level file of the package)
format = 1
name = "nginx-tools"
version = "1.2.0"
description = "Nginx diagnostics"            # optional, at most 200 characters
[compat]
protocols = ["1.2"]                           # required, non-empty; the agent's PROTOCOL_VERSION must be listed
platforms = ["linux-x86_64"]                  # optional; empty or absent = any; else "<os>-<arch>" must be listed
skill_contract = 1                            # required; the agent supports exactly 1
```

`PackageFailureCode` (**fixed**, `serde(rename_all = "snake_case")`, `as_str()`, `FromStr`, `ALL`): the stable
reason codes in the first position of every package failure message:

`packages_disabled`, `transfer_unknown`, `transfer_expired`, `transfer_kind_mismatch`, `signature_file_invalid`,
`untrusted_key`, `revoked_key`, `bad_signature`, `digest_mismatch`, `identity_mismatch`, `too_large`,
`archive_invalid`, `manifest_invalid`, `incompatible`, `path_escape`, `path_invalid`, `link_entry`,
`special_entry`, `duplicate_path`, `limit_entries`, `limit_uncompressed`, `limit_file_size`, `limit_depth`,
`limit_path_length`, `limit_time`, `no_skills`, `skills_invalid`, `skill_conflict`, `skill_run_as_forbidden`,
`version_conflict`, `too_many_packages`, `packages_dir_unsafe`, `no_space`, `store_corrupt`, `storage`.

Argument and output keys and the list output types (**fixed**), in `package/output.rs`:

```rust
pub mod keys {
    pub const ARG_PACKAGE: &str = "package";               pub const ARG_VERSION: &str = "version";
    pub const ARG_ARTIFACT_TRANSFER: &str = "artifact_transfer";
    pub const ARG_SIGNATURE_TRANSFER: &str = "signature_transfer";
    pub const OUT_PACKAGE: &str = "package";               pub const OUT_VERSION: &str = "version";
    pub const OUT_KEY_ID: &str = "key_id";                 pub const OUT_ARCHIVE_SHA256: &str = "archive_sha256";
    pub const OUT_INSTALLED_AT: &str = "installed_at";     pub const OUT_SKILLS: &str = "skills";
    pub const OUT_REPLACED_VERSION: &str = "replaced_version";
    pub const OUT_ALREADY_INSTALLED: &str = "already_installed";
    pub const OUT_SIGNER_UPDATED: &str = "signer_updated"; pub const OUT_ENABLED: &str = "enabled";
    pub const OUT_PRIVILEGE_BOUNDARY: &str = "privilege_boundary";
    pub const OUT_REMOVED: &str = "removed";               pub const OUT_COUNT: &str = "count";
    pub const OUT_INSTALL_ALLOWED: &str = "install_allowed"; pub const OUT_PACKAGES: &str = "packages";
}
#[serde(rename_all = "snake_case")] pub enum PackageStatus { Active, DisabledUntrustedKey, MissingFiles }
#[serde(rename_all = "snake_case")] pub enum KeyState { Trusted, Unknown, Revoked }
#[serde(rename_all = "snake_case")] pub enum PrivilegeBoundary { InEffect, NotInEffect }
pub struct PackageListEntry { pub name: PackageName, pub version: PackageVersion, pub key_id: KeyId,
    pub archive_sha256: ArchiveDigest, pub installed_at: Timestamp, pub skills: Vec<String>,
    pub status: PackageStatus, pub key_state: KeyState, pub executable: bool,
    pub executable_reason: Option<String> }
```

`OUT_PACKAGES` is `serde_json::to_string(&Vec<PackageListEntry>)` (compact, sorted by name). All booleans in
`outputs` are the strings `"true"`/`"false"`; `OUT_SKILLS` is the sorted names joined by `,`; empty values are
the empty string. `OUT_ENABLED` is always `"false"`: install never enables.

### 6.2 The three contracts (`capability/package.rs`, **fixed**; field types are 24's)

`pub fn package_contracts() -> Vec<CapabilityContract>`, all `version = 1`, `source = BuiltIn`,
`execution = Agent`, `target_kind = Host`, `target_arg = None`, `constrainable_args = []`, `safety_facts = []`:

| id | category | effect | reversibility | idempotency | timeout | output limit | verification |
|---|---|---|---|---|---|---|---|
| `agent.package.install` | `package.manage` | `Mutate` | `Reversible` | `Idempotent` | 300 s | 8 192 | `Postcondition { "the package appears in agent.package.list with the signed key id and archive digest" }` |
| `agent.package.uninstall` | `package.manage` | `Mutate` | `Reversible` | `Idempotent` | 60 s | 8 192 | `Postcondition { "the package no longer appears in agent.package.list" }` |
| `agent.package.list` | `package.observe` | `Read` | `NotApplicable` | `Idempotent` | 30 s | 262 144 | `None` |

`input_schema` is an `object`, `additionalProperties: false`: install requires the four `ARG_*` keys
(`package` and `version` strings, both transfer ids UUID strings); uninstall requires `package`, `version`;
list has no properties. Do **not** touch `builtin_contracts()` (24 pins ten contracts). Add these as an extra
source wherever 25 builds the hub's `ContractIndex` and wherever the agent builds its catalogue (6.6), so the
hub knows the ids and `ContractIndex::new` still validates (two categories, each one effect class).
Package install and uninstall are **human-authorized only**: the agent refuses `authorization_kind = policy`
for them (section 11), and prompt 39/42 must not offer them to policy `Allow`.

### 6.3 Config (`domovoi-config`, **fixed** keys, **convention** wording)

```toml
[agent]
packages_dir = "/var/lib/domovoi-packages"   # default; must be absolute

[packages]                                    # section absent = no installs, nothing advertised
allow_install = false                         # default false
revoked = ["e5f6a7b8c9d0e1f2"]                # default []
[[packages.trusted_keys]]
id = "a1b2c3d4e5f60718"                       # 16 lowercase hex, must equal the key's own key id
public_key = "<base64 of 32 bytes>"
label = "Domovoi release"                     # optional, at most 64 chars, no control characters
[packages.limits]                             # all optional
compressed_bytes = 20971520                   # 1 KiB .. 256 MiB
uncompressed_bytes = 104857600                # 1 KiB .. 1 GiB
file_bytes = 20971520                         # 1 .. uncompressed_bytes
entries = 2000                                # 1 .. 20000
path_depth = 8                                # 2 .. 32
path_bytes = 255                              # 16 .. 1024
extraction_time = "60s"                       # 1 s .. 15 min (the existing `Duration` type)
```

`AgentConfig.packages: Option<PackagesSection>` (`#[serde(default)]`); `AgentSection.packages_dir: PathBuf`
(`#[serde(default = ...)]`); add `packages_dir` to `AgentDirs`/`default_dirs` (check every `AgentDirs {` use).
`PackagesSection` is `deny_unknown_fields` and `JsonSchema`. The staging-space requirement (2 x
`uncompressed_bytes` free) is derived, not configurable. Shape validation in `validate.rs` (errors unless
noted): `id` and `revoked` entries 16 lowercase hex; trusted `id`s unique; `public_key` 44 base64 characters
(alphabet only; decoding happens in the agent); `label` rules; each limit inside its bounds and `file_bytes <=
uncompressed_bytes`; `packages_dir` absolute. Warnings: `allow_install = true` with no trusted keys (no package
can verify); a key id that is both trusted and revoked (revoked wins).

### 6.4 Transfer receiver (agent, **fixed** behaviour)

State machine, one record per `transfer_id`, driven sequentially by one task (frames of one transfer must
never be reordered, so they do **not** go through the 4-way parallel request handler):

| Frame | Condition | Result |
|---|---|---|
| `begin` | `[packages]` absent, or `allow_install = false` | record `failed(Aborted, "package installation is not allowed on this agent")`; answer at `end` |
| `begin` | kind unknown (`Open` without known) | record `failed(Aborted, "unknown transfer kind")` |
| `begin` | kind `package_artifact` and `size == 0` or `size > limits.compressed_bytes` | record `failed(TooLarge, ...)` |
| `begin` | kind `package_manifest` and `size == 0` or `size > MAX_SIGNATURE_FILE_BYTES` | record `failed(TooLarge, ...)` |
| `begin` | `transfer_id` already active or stored | record `failed(Aborted, "duplicate transfer id")` (the existing transfer is untouched) |
| `begin` | 2 transfers already active, or 8 already stored, or staged bytes + `size` > `2 * compressed_bytes + MAX_SIGNATURE_FILE_BYTES` | record `failed(Storage, "too many staged transfers")` |
| `begin` | ok | create `.staging/transfers/<id>.part` with `create_new`, mode 0600; start `Sha256` and counters |
| `chunk` | unknown id | ignore (debug log); the later `end` answers |
| `chunk` | transfer already failed | discard the bytes |
| `chunk` | `seq != expected` (expected starts at 0) | fail `Aborted` ("chunk out of order"), delete the part file |
| `chunk` | received + len > announced `size` | fail `TooLarge`, delete the part file |
| `chunk` | write error | fail `Storage`, delete the part file |
| `end` | unknown id | reply `Failed(Aborted, "unknown transfer")` |
| `end` | already failed | reply with the recorded failure, forget the record |
| `end` | received != `size` or SHA-256 != announced | delete the part; reply `Failed(ChecksumMismatch)` |
| `end` | ok | `sync_all`, rename `.part` to `<id>.bin`, record as **staged** with its kind and `stored_at`; reply `Stored` |

Rules: exactly **one** `transfer_result` per transfer, sent as the reply to the `transfer_end` envelope
(`reply_to = end.id`), because the hub correlates results by the id of the frame it waits on (`agent_server/
session.rs:220-232`); an early failure is remembered and reported at `end`. Timing, using the `Clock` port
(tests advance the fake clock, never sleep): an **active** transfer idle for 60 s or older than 10 min is
dropped and its part file deleted; a **staged** transfer unused for 15 min is deleted. Disconnects: active
transfers are dropped when the link session ends; staged ones survive a reconnect. Frames arrive from the
link through a bounded `mpsc` (64) with `send().await` (backpressure into the transport); the heartbeat
tolerance (three missed pings) bounds how long a stuck disk can stall the link.

Link glue (**fixed** shape, **convention** names), in `link/transfers.rs`: `pub struct IncomingTransfer { pub id:
RequestId, pub frame: TransferFrame }`, `pub enum TransferFrame { Begin(TransferBegin), Chunk(TransferChunk),
End(TransferEnd) }`, `LinkParts.transfers: Option<mpsc::Sender<IncomingTransfer>>`. `Link::handle` maps the three
messages to `IncomingTransfer` and awaits `send`. With `transfers == None` (no `[packages]` section) it ignores
`Begin` and `Chunk` and answers `End` at once with `TransferResult{Failed{Aborted, "package installation is not
enabled on this agent"}}` as a reply to that frame. The receiver task (`TransferReceiver::serve(rx, link_handle,
shutdown)`) owns the records, answers through `LinkHandle::send(Message::TransferResult(..), Some(end.id))`, and
wakes on a `Clock` timer to expire transfers. Core methods, **convention**: `begin(&mut self, TransferBegin)`,
`chunk(&mut self, TransferChunk)`, `end(&mut self, TransferEnd) -> TransferResult`, `expire(&mut self, now)`;
pure state plus file I/O, no link dependency, so tests drive them directly.

### 6.5 Package layout, registry and install (**fixed**)

Archive layout: every entry lives under exactly one top-level directory named `<name>` (the package name):
`<name>/package.toml`, `<name>/skills/<skill>/skill.toml`, `<name>/skills/<skill>/<entry>`, anything else
regular. Extraction strips `<name>/`.

On disk:

```
<packages_dir>/                      0755, owned by the agent's uid, not group/other-writable
  .packages.toml                     0600, durable atomic writes (the registry)
  .staging/                          0700, wiped at every start
    transfers/<id>.part | <id>.bin   0600
    <execution_id>/tree/             extraction target; renamed into place
  <name>/<version>/                  0755 dirs, 0644 files, 0755 owner-executable files
```

Registry `.packages.toml` (**fixed**; `format = 1`, at most `MAX_INSTALLED_PACKAGES` entries):

```toml
format = 1
[[package]]
name = "nginx-tools"
version = "1.2.0"
key_id = "a1b2c3d4e5f60718"
archive_sha256 = "9f86…0a08"
installed_at = "2026-11-18T10:00:00Z"
execution_id = "018f…"
skills = ["nginx_reload"]
```

**One version per package.** Installing another version replaces the registered one.

### 6.6 Catalogue advertisement (25's agent catalogue, **fixed** rule)

`catalog_entries` gains a `PackageSurface { list: bool, mutate: bool }` argument (25's merged signature may
differ: adapt and report). `[packages]` absent: none of the three ids is listed. Present with `allow_install =
false`: only `agent.package.list`. `allow_install = true`: all three. The list feeds both `get_catalog` pages
and `hello.catalog_hash`, so they cannot disagree. Execution applies the same rule independently (section 10).

### 6.7 Capability arguments and results (**fixed**)

`run_capability` payload (JSON shape, `TargetKey` is not interpreted by the agent; prompt 42 recommends
`package:<name>`):

```json
{"execution_id":"018f3c2e-0000-7000-8000-000000000001","contract":"agent.package.install","version":1,
 "target":"package:nginx-tools",
 "args":{"package":"nginx-tools","version":"1.2.0",
         "artifact_transfer":"018f3c2e-0000-7000-8000-0000000000a1",
         "signature_transfer":"018f3c2e-0000-7000-8000-0000000000b2"},
 "plan_hash":"<64 hex>","authorization_kind":"human","expires_at":"2026-11-18T10:05:00Z"}
```

Success: `{"outcome":{"succeeded":{"outputs":{"package":"nginx-tools","version":"1.2.0","key_id":"a1b2c3d4e5f60718",
"archive_sha256":"9f86…","installed_at":"2026-11-18T10:00:03Z","skills":"nginx_reload","replaced_version":"",
"already_installed":"false","signer_updated":"false","enabled":"false","privilege_boundary":"not_in_effect"}}}}`.
Failure: `{"outcome":{"failed":{"code":"refused","message":"untrusted_key: key a1b2c3d4e5f60718 is not trusted
on this host"}}}`. Every package failure uses `CapabilityFailureCode::Refused` and a message
`"{PackageFailureCode}: {detail}"`, at most 500 characters, redacted and with attacker text escaped (`{:?}`)
and truncated to 100 characters. Uninstall outputs: `package`, `version`, `removed` (`"true"`/`"false"`),
`privilege_boundary`. List outputs: `count`, `install_allowed`, `privilege_boundary`, `packages`.

### 6.8 `PackageService` (agent, **fixed** where marked)

```rust
pub struct PackageService { /* store, KeySet, settings, staging, events, clock, runs_as_root */ }
impl PackageService {
    /// Opens `packages_dir`, runs recovery, builds the key set. Fails only for an invalid key (fatal at start)
    /// or an unsupported platform; an unusable directory yields a fenced service that refuses with a reason.
    pub fn open(cfg: &PackagesSection, dirs: PackageDirs, deps: PackageDeps) -> Result<Self, PackageError>;
    pub fn surface(&self) -> PackageSurface;                                  // fixed
    pub fn receiver(&self) -> TransferReceiver;
    pub fn install(&self, execution_id: Uuid, req: InstallRequest) -> Result<InstallOutcome, PackageError>;
    pub fn uninstall(&self, req: UninstallRequest) -> Result<UninstallOutcome, PackageError>;
    pub fn list(&self) -> Result<ListOutcome, PackageError>;
    /// For prompt 47: refuses unless the package is registered at `version`, its signer is trusted and not
    /// revoked now, and its files exist. Nothing calls it in this prompt.
    pub fn resolve_for_execution(&self, name: &PackageName, version: &PackageVersion)
        -> Result<ResolvedPackage, ExecutionDenied>;                          // fixed
}
pub enum ExecutionDenied { NotInstalled, VersionMismatch { installed: PackageVersion },
    UntrustedKey { key_id: KeyId, state: KeyState }, FilesMissing }
```

(The execution id is the wire's `Uuid`; if prompt 22's `ExecutionId` is the better fit at this boundary,
convert there and say so.) Supporting types, **convention**: `PackageDirs { packages_dir, state_dir, skills_dir }`;
`PackageDeps { clock: Arc<dyn Clock>, runs_as_root: bool, platform: String, events: Arc<dyn EventSink>, space:
Arc<dyn SpaceProbe> }` (`SpaceProbe::free_bytes(&Path)`, real Linux `statvfs` through `nix`);
`InstallRequest { package, version, artifact_transfer: Uuid, signature_transfer: Uuid }`;
`UninstallRequest { package, version }`; `PackageSurface { list: bool, mutate: bool }`. `KeySet` has **no mutating method after construction** and no constructor other than from the parsed
`PackagesSection`: this is what makes "the hub cannot add a key" structural. `list`'s `executable` is `false`
with `executable_reason = "skill_runner_not_integrated"` for every package until prompt 47 replaces that
logic; a test pins the constant so 47 notices.

### 6.9 `PackageFixture` (testkit, **new**, **convention**)

`crates/domovoi-testkit/src/package.rs`: `PackageFixture::new(name, version)`, `.skill(skill_name)`
(writes `skills/<n>/skill.toml` + executable `run`), `.file(path, bytes)`, `.protocols(&[..])`,
`.platforms(&[..])`, `.raw_entry(header_bytes_fn)` (forge a header: `tar::Header::set_path` refuses `..`, so
write the name field bytes directly), `.build() -> BuiltPackage { archive: Vec<u8>, sig_toml: String,
signing_key: SigningKey, key_id: KeyId, public_key_b64: String, archive_sha256: ArchiveDigest }`,
`.sign_with(&SigningKey)`, `.tamper_digest()`. Testkit dependencies to add (root versions, `{ workspace = true }`):
`tar`, `ruzstd`, `ed25519-dalek`, `sha2`, `hex`, `base64`, `rand`, `toml`. Prompt 42's tests reuse it.

### 6.10 New dependency (named by this brief)

Add `ruzstd` (pure Rust zstd) to `[workspace.dependencies]` (latest 0.8.x that builds), used by the agent
(streaming **decode**) and testkit (encode). Reasons: no C toolchain for untrusted-input decoding, no `unsafe`
in our code, memory bounded. Before relying on it: run `cargo deny check` (licence and advisories), and read the
version's source to confirm the streaming decoder rejects a frame whose window exceeds a bound (it exposes a
maximum allowed window size). If it has no bound, add an explicit pre-check of the first frame header's window
size (reject above 128 MiB) and say so in the report. If `ruzstd` cannot meet that or fails `cargo deny`, stop
and report instead of switching crates. `tar` and `flate2` already exist; do not add `flate2` here.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file in section 3. Confirm
   24 and 25 are merged and read their real names (`builtin_contracts`, `ContractIndex`, the agent
   `requests/catalog.rs`, where the hub builds its index). Confirm `link/mod.rs` has no transfer arm, that
   `run_capability` still refuses everything, that `journal.started(id, 0, 0)` leaves an unfinished record
   (`OutcomeUnknown{agent_restarted}`) and that `ruzstd` builds and bounds the window (6.10). Write the
   differences down before editing; stop if one changes the design.
2. **Core types** (6.1): `package/*`, `lib.rs`, unit tests, `package_list_entry_snapshot`.
3. **Contracts** (6.2): `capability/package.rs`; add the extra source where the hub builds its `ContractIndex`
   and where the agent catalogue is built; leave `builtin_contracts()` untouched; contract tests.
4. **Config** (6.3): `packages.rs`, `agent.rs` fields, `validate.rs`, config tests and schema snapshot; run
   `cargo xtask docs-config`.
5. **Testkit** (6.9): `PackageFixture` first, because every later test uses it. Add dependencies.
6. **Portable agent pieces** with their tests: `error.rs`, `keys.rs`, `verify.rs`, `archive.rs`, `events.rs`.
   These compile and test on Windows too.
7. **Filesystem pieces** (Linux implementations, see section 11): `layout.rs`, `registry.rs`, `staging.rs`,
   `extract.rs`.
8. **Operations**: `install.rs`, `uninstall.rs`, `list.rs`, `recover.rs`, the `InstallStep` fault points and
   the in-crate crash tests.
9. **Receiver and link**: `transfer.rs`, `link/transfers.rs`, `LinkParts.transfers`, the three match arms,
   `AgentRuntime` wiring (build the service in `new`, spawn the receiver task in `run`, stop it with the shared
   token). Update the struct literals in `runtime.rs`, `tests/agent/harness.rs`, `tests/common/skills.rs`.
10. **Capability backend**: `requests/capability.rs`, remove the old function, `RequestCtx.packages`, `handle`
    awaits the new function; catalogue advertisement (6.6).
11. **Gates and integration tests** over `TestHub`; extend `tests/gates.rs`.
12. **Docs** (section 15), then the checks and the completion report.

**`run_capability` (agent, `requests/capability.rs`), in this exact order.**

```
async fn run_capability(ctx, run) -> Message
  1. journal Some and fenced                         -> refused(JournalCorrupt)        // existing behaviour kept
  2. contract is not one of the three                -> refused(ContractUnknown)       // as today for every other contract
  3. run.version != 1                                -> refused(ContractVersionUnsupported)
  4. ctx.packages is None, or install/uninstall while !surface().mutate
                                                     -> refused(ContractUnknown, "package installation is not enabled in agent.toml")
  5. install/uninstall and authorization_kind == Policy -> refused(AllowRiskDenied, "package changes need a human authorization")
  6. install/uninstall and run.expires_at <= clock.now() -> refused(AuthorizationExpired)
  7. args wrong (missing, extra, unparsable)         -> Error{code:"bad_request"}      // nothing journaled
  8. journal is None, or boot_snapshot(fs) fails     -> Error{code:"unavailable"}      // no mutation accepted
  9. journal.accept(AcceptedRequest{execution_id, contract, version, target: run.target.as_str(),
        plan_hash: &run.plan_hash.to_string(), authorization_kind, mutating: contract != list}, now, boot_id, mono_ns)
       Refused(r)        -> Message::Refused(*r)
       Duplicate(answer) -> Finished{result} => CapabilityResult(result) | Running => Error{in_progress} | else => Error{outcome_unknown}
       Accepted          -> continue
 10. journal.started(execution_id, 0, 0)            // in-process work: pid 0 on purpose (section 3)
 11. result = spawn_blocking(service.install | uninstall | list); a join error is failed(storage)
 12. journal.finished(execution_id, result.clone()); if that fails log at error level and still reply with the real result
 13. Message::CapabilityResult(result)
```

`accept` applies the mutation limiter to install and uninstall (`mutating = true`), so `[agent.mutation_limits]`
including `per_capability."agent.package.install"` applies; list is not counted.

**Install (`install.rs`), one mutating operation at a time (a mutex inside the service).**

```
install(exec, req):
  ready()?                                   // fenced -> its code; re-check packages_dir (not a symlink, owner == euid, no g/o write)
  art = staging.claim(req.artifact_transfer, PackageArtifact)?      // transfer_unknown | transfer_expired | transfer_kind_mismatch
  sig = staging.claim(req.signature_transfer, PackageManifest)?
  outcome = (|| {
    sf  = verify::parse_signature_file(bytes)?                      // signature_file_invalid
    verify::same_identity(&sf, &req.package, &req.version)?         // identity_mismatch
    key = keys.lookup(&sf.key_id)?                                  // untrusted_key | revoked_key (revoked wins)
    verify::signature(&key, &sf)?                                   // bad_signature: verify_strict over signing_message
    sha256_stream(art) == sf.archive_sha256 else digest_mismatch    // BEFORE any archive parsing
    events.verified(..)
    audit = archive::audit(art, &req.package, &limits, clock, &sf.archive_sha256)?   // writes nothing
    man = parse PackageManifest(audit.manifest_bytes)?              // manifest_invalid; name+version == request else identity_mismatch
    compat(&man)?                                                   // incompatible: PROTOCOL_VERSION listed, platform, skill_contract == 1
    match registry.get(name):
      Some(e) if same version && same digest => return AlreadyInstalled{ signer_updated: set e.key_id = sf.key_id (durable write) if it differs }
      Some(e) if same version                => version_conflict
      None if registry.len() >= MAX_INSTALLED_PACKAGES => too_many_packages
    space.free_bytes(packages_dir)? >= 2 * limits.uncompressed_bytes else no_space
    fault(AfterAudit)
    tree = .staging/<exec>/tree ; extract::run(art, .., &tree, &sf.archive_sha256)?
    fault(AfterExtract)
    validate_tree(&tree, &man, skills_dir, &registry)?              // no_skills | skills_invalid | skill_conflict | skill_run_as_forbidden
    dest = packages_dir/<name>/<version>; delete it first if present (it is unregistered, so stale)
    mkdir <name> 0755 (+chmod) if absent; rename(tree, dest); sync_dir(packages_dir/<name>)
    fault(AfterRename)
    registry::write_durable(registry.with(entry{key_id, archive_sha256, installed_at: clock.now(), execution_id, skills}))
       BeforeCommit  -> remove dest; return storage
       Indeterminate -> re-read .packages.toml: entry present => continue; else remove dest, return storage
    fault(AfterRegistry)
    remove the replaced version's directory (best effort)
    fault(AfterOldRemoved)
    events.installed(..); Installed{ replaced_version, skills, .. }
  })();
  staging.delete_claimed(art, sig)                                  // consumed whatever the outcome, once claimed
  if Err(e) and e is not InjectedCrash: staging.remove_exec_dir(exec); events.rejected(code, detail)
```

`InstallStep { AfterAudit, AfterExtract, AfterRename, AfterRegistry, AfterOldRemoved }` plus a `#[cfg(test)]`
`Faults { crash_after: Option<InstallStep> }`: when reached the function returns `PackageError::InjectedCrash`
**without any cleanup**, so the test drops the service and reopens it with `PackageService::open` to run
recovery exactly as a restart would. **Uninstall:** claim the lock; `ready()?`; absent package = success
`removed = false`; other registered version = `version_conflict`; otherwise write the registry without the entry
(commit point, same `BeforeCommit`/`Indeterminate` handling), then `remove_dir_all(<name>/<version>)` and remove
`<name>/` if empty (best effort; recovery sweeps leftovers). Uninstall does not require the signer to be trusted.

**Archive audit and extraction.** The decode pipeline is `file -> HashingReader -> ruzstd StreamingDecoder ->
LimitedReader -> tar::Archive`. `LimitedReader` errors when decoded bytes exceed `uncompressed_bytes + entries *
1536 + 4096` (tar headers and padding), so a bomb stops at a known cap. **Never call `Entry::unpack`/`unpack_in`**
(they honour modes, owners, links and xattrs): read each entry's bytes and write them yourself.

```
check_entry(name, limits, entry, seen)           // shared by both passes
  type: Regular | Directory ok; Symlink | Link -> link_entry; anything else (char, block, fifo, GNU sparse,
        contiguous, pax global header) -> special_entry
  raw = entry.path_bytes(): contains NUL, not UTF-8 or contains '\\' -> path_invalid;  len > path_bytes -> limit_path_length
  text = raw minus one trailing '/' (directories); starts with '/' -> path_escape
  parts = text.split('/'): any ".." or parts[0] != name -> path_escape
                           any part empty, "." or starting with '.', or with a char outside [A-Za-z0-9._+-] -> path_invalid
  parts.len() > path_depth -> limit_depth;   Regular and size > file_bytes -> limit_file_size
  text or its ASCII-lowercase already seen, or a file/directory prefix clash -> duplicate_path
```

Audit pass: iterate all entries, count them (`limit_entries`), sum regular sizes (`limit_uncompressed`), read each
to the end, capture `<name>/package.toml` (at most `MAX_PACKAGE_MANIFEST_BYTES`, else `manifest_invalid`), test
`clock.now()` against the deadline per entry and per 64 KiB read (`limit_time`), then drain the rest of the decoded
stream and require zeros (`archive_invalid`: data after the end). Any decoder or tar error is `archive_invalid`.
The hashing reader's final digest must equal the signed digest, else `digest_mismatch`. It creates nothing.
Extraction pass: the same loop calling `check_entry` again; create parents under `tree` with `create_dir` and an
explicit `set_permissions(0o755)` (never rely on umask); create files with `create_new(true)`, mode explicit
afterwards (0o755 when `header.mode() & 0o100 != 0`, else 0o644), `sync_all` each file and `sync_dir` each
directory.

**Recovery (`recover.rs`), run at every `PackageService::open`.** Ensure `packages_dir` is safe (create it 0755
when absent); delete `.staging` entirely and recreate it 0700 (staged transfers and half-extracted trees do not
survive a restart; the hub re-sends); load the registry (absent = empty; unreadable, unparsable or `format != 1`
= **fence** the service with `store_corrupt`: no sweep, no write, and install/uninstall/list fail with that code
until the owner repairs the file); for each directory directly inside `packages_dir` (via `symlink_metadata`;
skip non-directories, names starting with `.`, and names that are not a valid `PackageName`) delete each
subdirectory with a valid `PackageVersion` name that the registry does not list, then remove the package
directory only if it is empty; leave anything else and log it; emit `MissingFiles` for registered entries whose
tree is gone and `KeyUntrusted` for registered entries whose signer is not trusted now.

## 8. Database and persistence

Not applicable for SQL: **no migration, no hub table, no change under `crates/domovoi-store/`**. All state is
agent-local files, specified in 6.5: the registry `.packages.toml` (format 1, durable atomic writes through
`fs_atomic::write_atomic_durable`, mode 0600), the package trees, and the disposable `.staging/`. The commit
point of an install is the registry write; the commit point of an uninstall is the registry write without the
entry. There is no N-1 upgrade test because no existing data format changes; the compatibility tests are in
section 14.

## 9. Protocol and compatibility

- **Wire 1.2 is unchanged**: no new message, field, code or version; golden frames untouched. `PROTOCOL_VERSION`
  stays 1.2 and the exact major.minor rule is unaffected.
- This prompt only **defines how existing frames are used**: `package_artifact` = archive bytes,
  `package_manifest` = `.sig.toml` bytes (section 2); one `transfer_result` per transfer, as the reply to the
  `transfer_end` frame; chunk `seq` starts at 0 and is gap-free; `agent.package.*` args and outputs (6.7).
- `hello.catalog_hash` and `get_catalog` change **only** for agents whose `agent.toml` has a `[packages]`
  section (6.6). Agents without it announce the same list as before.
- An agent build without this prompt silently ignores transfer frames, but it never advertises the package
  contracts, so the registry reports them `NotInstalled` and prompt 42 never sends transfers to it. Prompt 42
  must treat a missing `transfer_result` as a failed transfer, not as success.
- An agent binary older than this prompt rejects an `agent.toml` containing `[packages]` or `packages_dir`
  (`deny_unknown_fields`). Downgrading therefore means removing the section; say so in the docs.

## 10. Error handling and recovery

**Request level** (all happen before anything is written; none of the first six reach the journal):

| Situation | Behaviour |
|---|---|
| journal fenced (corrupt) | `refused journal_corrupt` (existing) |
| contract not one of the three | `refused contract_unknown` (existing behaviour for every other contract) |
| version other than 1 | `refused contract_version_unsupported` |
| `[packages]` absent; or install/uninstall while `allow_install = false` | `refused contract_unknown`, detail "package installation is not enabled in agent.toml" |
| `authorization_kind = policy` for install/uninstall | `refused allow_risk_denied` |
| install/uninstall with `expires_at` not in the future | `refused authorization_expired` |
| missing, extra or malformed args (including any key such as `public_key` or `trusted_key`) | `Error bad_request` |
| no journal, or the boot snapshot cannot be read | `Error unavailable`; nothing accepted |
| mutation limiter exceeded | `refused rate_limited` (from the journal) |
| duplicate `execution_id`: finished / running / anything else | replay the stored `capability_result` / `Error in_progress` / `Error outcome_unknown` (the hub must use a new id after checking `agent.package.list`) |

**Execution failures** (after `started`): every one is `capability_result failed{code: refused, message:
"<PackageFailureCode>: <detail>"}`, the journal records it as `finished`, the staged transfers are deleted, the
execution staging directory is removed, and nothing is left under `packages_dir/<name>/`. The codes are the list
in 6.1; the check that produces each is in section 7. `storage` covers every I/O error. The service **fences**
(every operation fails with the same code until a restart) on: `packages_dir_unsafe` found at start or before a
mutation, `store_corrupt`.

**Startup and runtime:** an invalid trusted key (bad base64, wrong length, not a valid point, weak, or `id`
different from the key's own key id) is **fatal** to agent start (`AgentError::Packages`), naming the entry; a
`packages_dir` that cannot be made safe fences the service but the agent keeps monitoring; `transfer_*` failures
are the table in 6.4; a journal `finished` append failure is logged and the real result is still sent.

**Crash windows** (restart = reopen the service; the journal answers `outcome_unknown` for the interrupted run):

| Crash point | On disk after restart + recovery | Retry with a new execution id |
|---|---|---|
| before extraction (including after audit) | nothing; `.staging` wiped | clean install (the hub re-sends the transfers) |
| `AfterExtract` | `.staging` wiped; nothing placed | clean install |
| `AfterRename` (tree placed, registry not written) | unregistered `<name>/<version>` swept; old version still registered and intact | clean install |
| `AfterRegistry` | new version registered; replaced version's directory swept | `already_installed = true` (idempotent) |
| `AfterOldRemoved` | fully installed | `already_installed = true` |
| uninstall, after the registry write | files swept | `removed = false` |
| uninstall, before the registry write | unchanged | uninstall succeeds |

## 11. Security requirements

- **Process identity.** The agent writes only `packages_dir`, `.staging` inside it, and the registry. It never
  chowns, never sets setuid/setgid/sticky, never changes anything outside `packages_dir`, never spawns a process
  (R3/R4 untouched), never touches the helper, skill-runner units, `/etc/domovoi/*` or `state_dir`. Every path is
  built from a validated `PackageName`, `PackageVersion` and `ExecutionId`-derived directory name, never from a
  request string or an archive path. `packages_dir` must be absolute, not `/`, not a symlink, owned by the
  agent's effective uid (Linux `geteuid` through `nix`), not group/other-writable, and must neither equal nor
  contain nor lie inside `state_dir`, `skills_dir`, `/etc/domovoi`, `/var/lib/domovoi-helper`, `/run`, `/proc`,
  `/sys`, `/dev`, `/boot`, `/usr`, `/bin`, `/sbin` or `/lib` (the protected list is an injectable option so
  tests can use temporary paths). The filesystem layer is `#[cfg(target_os = "linux")]`; on other targets
  `PackageService::open` returns `PackageError::UnsupportedPlatform` and the surface is empty.
- **Trust.** Signatures are verified with `ed25519_dalek::VerifyingKey::verify_strict` over exactly
  `signing_message(..)`. Key lookup treats a revoked id as revoked even when it is also trusted. `KeySet` is built
  once from `PackagesSection`, has no mutator, and nothing on the wire, in arguments, in `welcome.config` or in
  `config_update` can reach it. The hub sees key ids only (in outputs).
- **Authorization.** install/uninstall need `authorization_kind = human` and an unexpired `expires_at`; the agent
  does not recompute the plan hash (the hub's execution boundary owns it, and there is no implementation hash for
  built-in contracts) and records it in the journal. Signing is **not** authorization: a correctly signed package
  still needs the owner's approval, and installing it does not enable it.
- **Validation.** Everything in `check_entry`, the limits table, the digest recomputed three times over the same
  bytes (stream, audit pass, extraction pass), `package.toml` and `.sig.toml` strict parsing
  (`deny_unknown_fields`), skill folder validation by `SkillCatalog::load(<tree>/skills)` with zero issues, between
  1 and 32 skills, no skill named like an owner skill in `skills_dir` or like a skill of another installed package,
  and no `run_as` in any package skill manifest (RW-26: packages never run as another identity).
- **Fail closed.** Any doubt (unknown key, parse failure, unexpected entry type, missing file, unreadable
  registry, unsafe directory, a transfer or claim that is absent, expired or of the wrong kind) refuses.
- **Resource bounds.** Transfer: 64 KiB chunks, `compressed_bytes` cap, at most 2 active and 8 staged transfers,
  idle and total timeouts. Decode: bomb cap above. Time: `extraction_time` through the `Clock` port. Disk: 2 x
  `uncompressed_bytes` free. Memory: never buffer an archive; only `package.toml` (64 KiB) and `.sig.toml`
  (16 KiB) are held in memory.
- **Audit.** Events through `EventSink` (default `TracingEvents`, target `domovoi::package_audit`, fixed field
  names): `package.verified`, `package.rejected{code}`, `package.installed`, `package.uninstalled`,
  `package.key_untrusted` (at start for registered packages whose signer is not trusted), `package.recovered`,
  `package.transfer_rejected{code}`; each carries `execution_id` when it has one, package, version, key id and
  digest, never archive contents. The durable per-execution record is the action journal (`accepted`, `started`,
  `finished` with the result). The hub writes its own audit rows (prompt 42) from the capability results and from
  `agent.package.list`.
- **Secrets.** None are handled. Attacker-controlled text (entry paths, TOML values) appears in messages and
  logs only escaped and truncated to 100 characters.
- **Pre-H1 root agent.** Section 2: allowed, labelled `not_in_effect`, directory ownership rule, no ownership or
  mode taken from the archive, and no execution path for package skills.

## 12. Testing requirements

Naming follows behaviour. Fakes: `FakeClock` (`advance`), `FixtureHostFs`, `ListRedactor`, `TestHub`,
`Fixture` (all existing); **new** `PackageFixture` (testkit) and three test-local helpers in the agent crate: a
stepping clock (advances on every `now()`), a fake space probe, and a recording `EventSink`. Core and config tests
and the portable agent tests run natively; tests that need the filesystem layer are `#[cfg(target_os = "linux")]`
and run in WSL. Forge hostile headers by writing the raw tar name field (`set_path` refuses `..`).

**`domovoi-core` and `domovoi-config`.**
`package_name_accepts_and_rejects_by_grammar`, `package_version_accepts_and_rejects_by_grammar`,
`key_id_is_first_sixteen_hex_of_public_key_sha256` (compute the expected value independently, plus one literal
vector produced by `sha256sum`), `signing_message_is_byte_exact` (including the trailing newline),
`signature_file_rejects_unknown_field_wrong_format_and_wrong_algorithm`, `failure_codes_are_unique_and_round_trip`,
`package_list_entry_snapshot` (insta), `package_contracts_validate_and_index`,
`builtin_contracts_are_unchanged_by_package_contracts`; config: `packages_section_absent_means_no_installs`,
`packages_defaults_match_the_contract`, `trusted_key_id_must_be_sixteen_lower_hex`,
`duplicate_trusted_key_ids_are_rejected`, `limits_outside_bounds_are_rejected`,
`revoked_entry_must_be_sixteen_hex`, `packages_dir_must_be_absolute`, `allow_install_without_keys_warns`,
`key_trusted_and_revoked_warns`, and the schema snapshot.

**Agent, verification and archive** (`src/packages/tests/`):

| Test | Setup, action, expected |
|---|---|
| `agent_rejects_untrusted_signature` | signed by a key not in the set; `untrusted_key`; no `.staging/<exec>` created |
| `agent_rejects_revoked_key_even_when_trusted` | id in both lists; `revoked_key` |
| `agent_rejects_bad_signature` | flipped signature byte; signature over another version; `bad_signature` |
| `agent_rejects_identity_mismatch` | args vs sig file, sig file vs `package.toml`, package vs version; `identity_mismatch` each |
| `agent_rejects_digest_mismatch_before_extracting` | sig over other bytes; `digest_mismatch`; the audit pass never ran (no reads of entries), nothing written |
| `signature_file_is_strict` | oversize, unknown field, bad base64, 63-byte signature; `signature_file_invalid` |
| `compat_rules_are_enforced` | protocol not listed, platform not listed, `skill_contract = 2`; `incompatible` |
| `trusted_key_id_must_match_public_key` | `KeySet` construction fails |
| `agent_rejects_path_escape` | `../x`, `a/../../x`, `/etc/passwd`, `<name>/../x`, entry outside `<name>/`: `path_escape`; empty part, backslash, NUL, non-UTF-8, leading-dot part: `path_invalid` |
| `agent_rejects_links_in_archive` | symlink and hardlink: `link_entry`; device, fifo, GNU sparse, pax global header: `special_entry` |
| `agent_enforces_archive_limits` | one case per limit with lowered config: entries, total, per file, depth, path length, time (stepping clock), compressed size (`too_large`) |
| `decompression_bomb_stops_at_the_cap` | few KiB compressed, hundreds of MiB zeros; `limit_uncompressed`; returns promptly |
| `duplicate_and_case_colliding_paths_are_rejected`, `file_directory_clash_is_rejected` | `duplicate_path` |
| `data_after_archive_end_is_rejected`, `not_zstd_is_archive_invalid`, `missing_or_oversize_package_toml_is_manifest_invalid` | codes as named |
| `audit_pass_writes_nothing` | after every rejection above the packages directory is byte-identical to before |

**Agent, install, uninstall, list** (`src/packages/tests/install.rs`, Linux): `install_extracts_with_normalized_modes_and_records_signer`
(dirs 0755, files 0644, executable 0755; a setuid and a group-write bit in the archive are ignored; registry has
signer, digest, time), `install_never_enables` (`SkillCatalog` over `skills_dir`, `hello.skills` and the catalogue
inputs are unchanged; output `enabled = "false"`), `install_refused_when_allow_install_false`,
`install_refused_when_packages_section_absent`, `reinstall_same_archive_is_idempotent`,
`same_version_other_digest_is_version_conflict`, `update_replaces_old_version_and_removes_its_files`,
`rollback_requires_current_trust` (v2 by key A, remove A, rollback v1 by A refused, by B accepted),
`resigned_package_with_trusted_key_updates_signer_without_reextracting`,
`revoked_key_disables_installed_package`, `removed_key_disables_installed_package` (list status
`disabled_untrusted_key`, `resolve_for_execution` denied, `package.key_untrusted` emitted at open),
`uninstall_removes_files_and_registry_entry`, `uninstall_unknown_package_is_ok_with_removed_false`,
`uninstall_other_version_is_version_conflict`, `uninstall_works_when_the_key_is_revoked`,
`skill_name_conflict_with_owner_skill_is_rejected`, `skill_name_conflict_with_other_package_is_rejected`,
`run_as_in_package_skill_is_forbidden`, `package_without_skills_is_rejected`, `too_many_packages_is_refused`,
`no_space_is_refused_before_extracting`, `packages_dir_not_owned_by_agent_or_group_writable_is_unsafe`,
`packages_dir_symlink_is_refused`, `packages_dir_overlapping_protected_roots_is_refused`,
`root_agent_labels_privilege_boundary_not_in_effect`, `list_reports_status_key_state_and_missing_files`,
`list_output_fits_its_limit_at_maximum_packages`.

**Agent, crash and recovery** (`src/packages/tests/crash.rs`, Linux): `crash_after_each_install_step_recovers`
(table over `InstallStep`, asserting the section 10 table for registry, directories and the retry result),
`crash_during_uninstall_recovers`, `journal_reports_outcome_unknown_after_crash_mid_install` (reopen the journal),
`registry_write_failure_rolls_back_the_placed_tree` (`DurableFaults.rename`),
`indeterminate_registry_write_is_resolved_by_rereading` (`DurableFaults.dir_sync`),
`corrupt_registry_fences_and_sweeps_nothing`, `unregistered_versions_are_swept_but_foreign_files_are_not`,
`staging_is_wiped_at_start`.

**Agent, transfers** (`src/packages/tests/transfer.rs`): `transfer_stores_bytes_and_answers_once_at_end`,
`transfer_failure_is_reported_at_end_not_before`, `transfer_rejects_checksum_mismatch`,
`transfer_rejects_oversize_announcement`, `transfer_rejects_chunk_beyond_size`,
`transfer_rejects_out_of_order_chunk`, `transfer_rejects_duplicate_id`, `transfer_rejects_unknown_kind`,
`end_for_unknown_transfer_is_answered_aborted`, `idle_transfer_is_dropped_by_the_clock`,
`staged_transfer_expires`, `too_many_transfers_are_refused`, `staged_byte_budget_is_enforced`,
`transfers_refused_when_install_not_allowed`, `part_files_are_private`.

**Agent, integration over `TestHub`** (`tests/agent/packages.rs`, Linux where files are touched):
`package_install_end_to_end_over_the_link` (two transfers, `run_capability`, result, files, registry, journal
`Finished`), `list_over_run_capability_reports_the_installed_package`, `policy_authorization_is_refused`,
`expired_authorization_is_refused`, `duplicate_execution_id_replays_the_first_result_and_runs_once`,
`extra_capability_args_are_bad_request`, `get_catalog_advertises_package_contracts_per_config` (absent, false,
true), `catalog_hash_changes_with_allow_install`, `transfer_end_without_section_is_answered_aborted`,
`heartbeat_is_answered_while_chunks_flow`, `other_contracts_are_still_refused_unknown`,
`mutation_limit_applies_to_install` (`per_capability` = 0 gives `rate_limited`).

**Gates** (`crates/domovoi-agent/tests/gates.rs`; extend, never weaken): `hub_cannot_add_trusted_key` (an install
signed by an unknown key is refused; args carrying a key are `bad_request`; `agent.toml` and the `KeySet` are
unchanged; a source scan asserts `src/packages/keys.rs` has no `pub fn` taking `&mut self`),
`package_install_cannot_write_helper_paths` (hostile entries aimed at a sibling "helper" directory, and a
`packages_dir` inside a protected root, leave that directory unchanged),
`package_skills_are_not_runnable_before_the_skill_runner` (`run_skill` for an installed package's skill is
`NotFound`; `hello.skills` unchanged).

Out of this prompt (prompt 42): `partial_install_reports_per_host`, `owner_import_requires_trusted_owner_key`,
and every hub-side test of 04 §11.2.

## 13. Acceptance criteria

1. Wire 1.2 is unchanged; `git diff` shows no change under `crates/domovoi-proto/` and no golden frame change.
2. No SQL migration and no change under `crates/domovoi-store/`.
3. With no `[packages]` section the agent behaves as before: same `hello.catalog_hash` inputs, install and
   uninstall refused as `contract_unknown`, transfer frames answered at `transfer_end` with `Failed(Aborted)`.
4. `allow_install = false` advertises only `agent.package.list`; `true` advertises all three; the same rule is
   enforced at execution.
5. A package signed by a trusted, unrevoked key with a matching digest installs: files are under
   `packages_dir/<name>/<version>/` with modes 0755/0644/0755, the registry holds the signer `key_id`, digest and
   time, and the result says `enabled = "false"`.
6. Unknown key, revoked key, bad signature, digest mismatch and identity mismatch each fail with their code and
   leave `packages_dir` and `.staging` unchanged (no archive entry read for a digest or trust failure).
7. Path escape, links, special entries, duplicates and every limit are rejected by the audit pass before a byte
   is written; a decompression bomb stops at the decode cap.
8. Hostile entries cannot create anything outside `packages_dir/<name>/`; a gate test proves a sibling helper
   directory is unchanged.
9. Install of the same archive again is idempotent; same version with another digest is `version_conflict`;
   another version replaces the old one and removes its files.
10. Rollback needs a currently trusted key; a key removed or revoked after install makes `list` show
    `disabled_untrusted_key` and `resolve_for_execution` refuse; uninstall still works.
11. A crash at every `InstallStep` and during uninstall converges after restart as in the section 10 table, and
    the retry succeeds or is idempotent.
12. `run_capability` for the three contracts follows the order in section 7, journals `accepted/started/finished`,
    replays a duplicate execution id, applies the mutation limiter, and refuses policy authorization.
13. The journal reports `OutcomeUnknown{agent_restarted}` for an install interrupted by a crash.
14. A root agent installs with `privilege_boundary = "not_in_effect"` and the directory ownership rule enforced.
15. `transfer_*` frames are stored and answered once as section 6.4 says, bounded in time, count and bytes.
16. An invalid trusted key is fatal at start; an unsafe `packages_dir` fences packages but not monitoring.
17. Nothing can add, replace or remove a key at runtime; no package skill enters `SkillCatalog` or `hello.skills`.
18. All named tests exist and pass; non-test files are at most 500 lines; no new `#[allow]`, `unwrap`,
    `expect` or `panic!` in library code; `cargo xtask arch-check` passes.

## 14. Migration and compatibility

Existing agents and configs: nothing changes until the owner adds `[packages]`. A new `packages_dir` default does
not create anything unless `[packages]` exists. Hubs: no hub change here; a hub built before prompt 42 simply
never sends transfers. Existing owner skills in `skills_dir` are untouched and keep running as today. Data:
`.packages.toml` is new, `format = 1`; a future format bump must fence, never guess. Pre-H1 root agents: section 2.
After prompt 47 (agent as `domovoi-agent`, `packages_dir` pre-created, skill runner) the same directory, registry
and rules apply; 47 only has to integrate package skills behind the runner and replace the constant
`executable_reason`.

## 15. Documentation requirements

- `cargo xtask docs-config` regenerates `docs/book/src/reference/configuration.md`; commit the result and review the
  `[packages]` and `packages_dir` rows. Accept the config schema snapshot only after reading the diff.
- `docs/book/src/reference/agent.md`: a "Packages" section: layout of `packages_dir`, trust model (local keys, no
  registry, hub cannot change keys), restart-only key reload, revocation effect, install never enables or runs,
  the pre-H1 root labelling, the limits table, crash recovery.
- `docs/book/src/reference/protocol.md`: the use of `transfer_*` (kind meaning, single result, chunk rules,
  timeouts) and the three `agent.package.*` capabilities with argument and output keys.
- `docs/book/src/security.md`: one paragraph on package trust and its limits (owner-signed = as trusted as files the
  owner places; execution only through the skill runner, not yet available).
- `docs/book/src/guide/skills.md`: one paragraph that packages are inert until the skill runner exists.
- `ARCHITECTURE.md`: §7.2 (agent: transfer receiver, package service), §12 (packages next to skills), §14
  (`[packages]`, `packages_dir`), §15 (invariants: the hub cannot add keys; install never enables).
- No OpenAPI change, so no `cargo xtask openapi` or `npm run gen:api`.
- Commits and the PR must not mention prompts, the build plan or coding agents (repository rule).

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

Task-specific: the only new dependency is `ruzstd` (6.10), plus existing workspace crates added to
`domovoi-agent` and `domovoi-testkit`. Prompts 24 and 25 must be merged; if their names differ from the ones
used here, use the merged names and report each difference. Run Linux-only tests in WSL (`wsl.exe -d Ubuntu --`,
`CARGO_TARGET_DIR=$HOME/domovoi-target`), and say which tests ran where.

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

Task-specific checks: crates edited are `domovoi-core`, `domovoi-config`, `domovoi-agent`, `domovoi-testkit`
(and the hub only where the contract index gains its extra source), so use `rdeps` for `domovoi-core` and
`domovoi-config`; run `cargo xtask arch-check` and `cargo deny check` (new dependency); `cargo xtask docs-config`
and `cargo xtask docs-links` for the docs. Commit as `Rostislav <raikov0411@gmail.com>`; the PR title has one
scope, for example `feat(agent): verify, install and remove signed packages`.

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
