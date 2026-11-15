# 55 — R-10 helper safety attestation research

| | |
|---|---|
| Branch | `spike-safety-attestation` |
| Milestone / research item | R-10 — end-to-end helper safety attestation (validation gate for H1 and M9) |
| Dependencies | None. Read `03 RW-37`, `03 RW-39`, `04b` in full, `04 §8.3–8.4` and `04 §9.1`, and R-10 / D-28 / D-29 in `99`. |
| Can run alongside other research | Yes. Prompts 53, 54 and 56 are independent. This is a separate, security-sensitive research task. |
| Produces | R-10 evidence report and a reproducible, isolated scratch harness. |
| Report location | Next free letter under `docs/rework/investigations/`, likely `L-r10-safety-attestation.md`. Check the directory at execution time; never overwrite an existing report. |
| Scratch artefacts | `docs/rework/investigations/probes/r10/` |
| Launch gate | None. No additional environment gate. |
| Suggested model | large |

## 1. Objective

Answer one question with reproducible evidence:

**Can the proposed helper safety-attestation design be implemented end to end without ambiguity or
security gaps in its encoding, hashes, signature verification, key lifecycle, nonce handling,
route-state observation, generation tracking, persistent state, locking, race protection and
adversarial-agent handling?**

Build a small isolated scratch implementation that reproduces the proposed contracts and tries to break
them. The deliverable is evidence, a reproducible harness and a list of gaps — not production code, and
not a proof that the proposal is correct merely because a reimplementation of its own assumptions agrees
with it.

This is research for R-10. The evidence it produces is needed before the helper ADRs (prompt 44), the
helper and skill-runner implementation (prompt 47) and the containment capability (prompt 46). R-10 stays
open until the owner reviews the evidence and records acceptance in the decision register and ADRs.

## 2. What this prompt is not

- It is not prompt 44 (`helper-adrs`), 47 (`helper` and `skill-runner`) or 46 (`containment`). No
  production module, subcommand, socket, unit, protocol message, endpoint, migration or config key.
- It does not implement the helper, the agent, the hub, the installer or any capability.
- It does not change the wire protocol, `PROTOCOL_VERSION`, or any message shape.
- It does not change `03`, `04`, `04b`, `06` or `99`, and does not accept R-10, D-28 or D-29.
- It does not create or read a real `/var/lib/domovoi-helper`, a real `attest.key`, a real helper state
  file or a real `/etc/domovoi/helper.toml`.
- It does not add a hub signing key, an owner-signed permit or any new trust root.
- It does not declare the helper production-ready, and a green scratch suite is not owner acceptance.

## 3. Architecture in plain language

The contract below is `03 RW-37` and `04b`. Reproduce it exactly; do not improve it in passing.

### 3.1 Key identity and enrollment

- The helper holds an Ed25519 key pair. A key exists only because someone ran the proposed
  `domovoi helper init` as root. The helper never generates a key implicitly, at startup or during
  operation.
- Private key: `/var/lib/domovoi-helper/attest.key`, root-owned, mode `0600`, in a root-owned `0700`
  directory. It is masked in the agent and skill-runner units.
- Public key: exactly the 32 raw Ed25519 public-key bytes. Delivered as standard padded base64 of those
  32 bytes in `hello.helper_identity`, `agent_status.helper_identity` and every `safety_attestation`.
  Any other length is rejected.
- `fingerprint` is the lowercase hex SHA-256 of those 32 raw bytes (64 hex characters), displayed in
  groups of four. The hub computes it itself and ignores any fingerprint or id the agent sends.
- `helper_key_id` is the first 16 hex characters of the fingerprint. It is a **label only** — never a
  verification input and never a trust decision. A matching key id with a different key is a mismatch.
- Enrollment: the hub shows the received key as unverified; the owner runs the proposed
  `domovoi helper fingerprint` as root on the host, compares the fingerprint out of band and confirms
  with step-up; the hub pins the exact 32 bytes. Nothing is pinned automatically, including on first
  contact.
- Verification requires both: the message's `public_key` equals the pinned key byte for byte, **and**
  `ed25519::verify_strict(pinned_key, signing_bytes, signature)` succeeds.
- Key events have defined behaviour: `rotate-key` (new key, `generation=1`), `recover` after lost or
  damaged state (retire old key and state, new key, `generation=1`), reinstall (`init` creates a key only
  when neither file exists), key received ≠ pinned key (unverified, containment blocked, old pin kept),
  owner re-enrollment (pin replaced, audited), owner unpin (no pin, containment blocked), agent
  re-enrollment or upgrade (helper pin unchanged), host deleted (pin deleted).

### 3.2 Nonce and signed report

- `attest_safety{nonce}`: the hub sends 32 cryptographically random bytes, base64 encoded. Each nonce is
  issued for one host, is single use, and is valid for 60 seconds. The nonce is the helper's **only**
  input to `safety.attest`.
- The report carries `nonce=<64 lowercase hex>`.
- Report bytes are exact UTF-8 bytes, every line terminated by one LF, in the fixed order of RW-37.
  Repeated `*` lines are sorted bytewise ascending with no duplicates.
- Signing bytes are exactly `domovoi-safety-attest-v1\n` followed by the full report bytes. The signature
  is exactly 64 bytes, base64 encoded.
- The message is `safety_attestation{public_key, report: base64(report bytes), signature: base64(64)}`.
- The hub verifies the signature over the received bytes **first**, using the pinned key, then parses
  those same bytes with a strict parser. It never re-serialises the report before verification.
- The report becomes the host's verified safety state only when the signature verifies, the nonce is
  valid and `complete=true`.

### 3.3 Encoding and hashes

`04b` is authoritative and byte-level. The harness must reproduce **every** C1–C4 and R1–R4 vector and
all invalid-input vectors, including exact byte lengths and SHA-256 digests.

- Lines: UTF-8, one LF each; no CR, trailing spaces, blank lines or BOM.
- Grammar: `line = key "=" value`; the key set and order are fixed by RW-37; fields are joined by `|`.
- Percent encoding is strict: `A–Z a–z 0–9 . _ : / @ -` literal; every other byte — including `|`, `=`,
  `%`, `;`, `,`, space, CR, LF and every non-ASCII byte — written as `%XX` with **uppercase** hex.
  A decoder rejects lowercase hex, an encoded literal byte (for example `%41`), a truncated escape, and
  any raw byte outside the literal set plus `%`.
- Typed fields: `ip` (IPv4 dotted decimal without leading zeros, or IPv6 RFC 5952 lowercase), `cidr`
  (network address, host bits zeroed, `/prefix`), `ifaddr` (interface address keeping host bits,
  `/prefix`), integers (decimal, no sign, no leading zeros, `0` allowed), `family` (`inet`|`inet6`), and
  `-` for an absent optional gateway.
- `config_hash` = SHA-256 of the line `domovoi-safety-config-v1` (with its LF) followed by the config
  lines in order, each LF-terminated. `route_state_hash` = the same with `domovoi-safety-routes-v1` and
  the route lines. The three domain prefixes (config, routes, signature) are distinct.
- Excluded from both hashes: `format`, `helper_key_id`, `nonce`, `boot_id`, `generation`, the two hashes,
  `complete`, `incomplete_reason`, `computed_at`.
- Config lines carry the effective configuration: normalised and de-duplicated `cidr`s; `protected_unit`s
  canonicalised (missing suffix becomes `.service`, case-sensitive) and unioned with the mandatory set
  `domovoi-agent.service`, `domovoi-helper.socket`, `domovoi-skill-runner.socket`, `sshd.service`, which
  cannot be removed; de-duplicated `protected_container` names (resolved to ids only at apply time);
  `max_ttl_s` from `[safety.containment] max_ttl` in seconds, default `3600`; `max_active`, default `0`.
- Comments, whitespace, TOML ordering and duplicate list entries do not affect the hash. A TOML file that
  fails to parse, or holds an invalid CIDR or name, yields **no** hash; the report is `complete=false`,
  `incomplete_reason=config_invalid`, and containment is refused.

### 3.4 Strict report parser

The parser validates the original signed bytes and must never normalise invalid input into valid input. It
rejects unknown, missing, duplicate or out-of-order keys; duplicated or unsorted repeated lines;
non-canonical typed values; invalid percent-encoding (lowercase escapes, encoded literal bytes, truncated
escapes); wrong field counts; raw separators inside a field; CR, BOM, blank lines, trailing whitespace and
a missing final LF; `format` ≠ 1; invalid UTF-8; a nonce that is for another host, expired, missing or
already used; public-key or signature lengths that are not exactly 32 and 64 bytes; invalid base64; an
invalid signature; and a substituted key.

### 3.5 Route-state observation

The helper observes, via netlink as root: all IPv4 and IPv6 policy rules; every routing table referenced
by a rule, plus `main` and `local`; interface addresses and link state.

Only this state enters the report and `route_state_hash`:

- `endpoint_route`: the kernel route-lookup result (equivalent to `ip route get`, honouring policy rules)
  for each management endpoint — every `hub_addr`, every `link_peer` address, and the network address of
  every configured `cidr`. Fields: endpoint, table, gateway or `-`, interface, source address.
- `route`: every route one of those lookups selected, plus every default route in any referenced table.
  Fields: family, table, destination CIDR, gateway or `-`, interface, metric.
- `rule`: every policy rule that can select one of those tables. Fields: family, priority, selector,
  table. The selector is `all` or the items present in the fixed order
  `from`,`to`,`iif`,`oif`,`fwmark`,`ipproto`,`sport`,`dport`,`uidrange`,`not`, joined by `;`, then
  percent-encoded as one field.
- `local_addr`: addresses on interfaces named by any `endpoint_route`. Fields: ifaddr, interface.

Routes and addresses not involved in reaching a management endpoint are **not** attested, and changing
them does not invalidate confirmation. The report and the UI must never claim that "any route change"
invalidates. If rules, tables, lookups or addresses cannot be read, or a lookup fails, `complete=false`
with a reason and all containment is refused.

### 3.6 Safety identity and generation

Safety identity = `(fingerprint, boot_id, generation, config_hash, route_state_hash)`.

- `generation` is a u64 counting changes of the effective safety set, i.e. of the config lines and route
  lines.
- It increments only when the newly computed hashes differ from the last persisted ones.
- Adding or removing a containment element in the nft set does not change it.
- A new `boot_id` with identical content does not increment it.
- Under one key it never decreases and never repeats.
- `rotate-key` and `recover` start a new sequence at `generation=1`.
- The hub stores the highest verified generation per pinned key and rejects a lower one as
  `generation_regressed`.

### 3.7 Persistent state, locking and atomic writes

Files, all root, directory `/var/lib/domovoi-helper` `0700`:

- `attest.key` (`0600`): the Ed25519 private key.
- `state` (`0600`): LF lines `format=1`, `key_fingerprint=<64 hex>`, `generation=<u64>`,
  `config_hash=<64 hex>`, `route_state_hash=<64 hex>`, `boot_id=<uuid>`, then
  `checksum=<64 hex SHA-256 of all preceding bytes>`.
- `state.lock` (`0600`): an empty lock file.

Every helper instance takes `flock(LOCK_EX)` on `state.lock` and holds it across the whole sequence: read
and validate state; recompute config and route lines; compare; increment and write if changed; then sign,
or for an apply compare `expected_safety` and run the nft transaction. Concurrent instances therefore
cannot compute conflicting generations.

Atomic write: write `state.tmp`, `fsync` it, `rename` it over `state`, `fsync` the directory, and delete a
leftover `state.tmp` under the lock at startup. A checksum mismatch, a missing field or an unknown
`format` means corrupt.

Fail-closed conditions and their reasons: `state_missing` (state missing, key present), `state_corrupt`,
`state_key_mismatch`, `key_missing` (state present, key absent), `uninitialized` (both missing), and an
interrupted write (old file intact, generation not advanced, retried next request). None of them is reset
automatically; only the explicit root actions `init` (the only first-time path, refuses to overwrite),
`recover` (retire old key and state, new key, `generation=1`) and `rotate-key` (new key,
`generation=1`) change state.

### 3.8 Expected safety and the adversarial agent

`expected_safety` is a tagged value, required on every `Contain` request; a missing or unknown tag is
refused `malformed_safety`.

- `verified{fingerprint, boot_id, generation, config_hash, route_state_hash}`
- `unverified{reason}`, where `reason` is one of `key_not_pinned`, `key_mismatch`, `attestation_missing`,
  `attestation_stale`, `signature_invalid`, `report_incomplete`, `generation_regressed`,
  `path_uncovered`, `confirmation_stale`.

No fingerprint or identity is ever fabricated for `unverified`.

Verified path: pinned key, a verified complete attestation under 5 minutes old, current confirmations;
the full identity is bound to the confirmation, the permit or approval, and the dispatch. The helper
compares the full identity with its freshly computed one: a mismatch is `refused{safety_changed}`, unknown
state is `refused{safety_unknown}`.

Unverified human path: an authenticated human approval with step-up **and** typed confirmation; the
approval card states that coverage cannot be verified, gives the reason, and lists every discovered path
that is uncovered or unverified as not protected; the hub persists
`safety_assurance = unverified{reason, unprotected_paths}` and audits `containment.unverified_approved`.
The hub never issues a `PolicyPermit` with `unverified`; the agent refuses `unverified` when
`authorization_kind = policy` (`refused{unverified_requires_human}`); the helper skips only the identity
comparison and still recomputes its set, refuses overlap, enforces maxima and mutation limits, and fails
closed on unknown state.

Known changes between the steps and their required outcomes: `helper.toml` edited after attestation,
relevant network state changed, irrelevant network change, helper restart, host reboot, pinned key
changed, report older than 5 minutes, permit issued before a reboot, generation changed between
verification and apply, helper unable to recompute. RW-37's table gives the expected outcome for each;
the harness tests those outcomes.

**Security limitation that must be preserved, not repaired.** `expected_safety` is a consistency guard for
the honest path, not cryptographic proof of a hub permit. A compromised agent can omit or fabricate it and
call the helper directly. What always holds regardless of the request: the helper recomputes its actual
set, refuses any overlapping target, enforces its own TTL and active-count maxima and mutation limits, and
fails closed on unknown state. A hub-signed permit the helper could verify would be a separate
architectural change with a new trust root, hub signing-key custody and rotation, clock or nonce
semantics, and a protocol change. It is not part of this design.

## 4. Repository and environment context

Verify all of this before relying on it.

- **No helper exists.** There is no `helper` subcommand in `crates/domovoi/src/cmd/mod.rs`, no
  `safety.attest` operation, no socket activation, no `SO_PEERCRED`, no `flock`, no
  `/var/lib/domovoi-helper` code, no attestation key handling and no helper state file. Confirm this with a
  grep before starting; if any of it has appeared, record what exists and reconcile the brief against it.
- **No attestation report, parser, hash or vector test exists.** `04b` is an unimplemented proposal.
  `PROTOCOL_VERSION` is `1.1` (`crates/domovoi-proto/src/version.rs`); the 1.2 `run_capability`,
  `attest_safety` and `safety_attestation` messages do not exist.
- **Existing, reusable patterns.** These are the repository's own conventions; read them before writing
  the harness, and do not modify them:
  - Ed25519 signing: `crates/domovoi-agent/src/identity.rs` uses `ed25519_dalek::{Signer, SigningKey}` and
    standard padded base64 from the `base64` crate.
  - Strict verification: `crates/domovoi-hub/src/agent_server/handshake.rs` uses
    `VerifyingKey::verify_strict` with `Signature::from_bytes`.
  - Atomic writes: `crates/domovoi-agent/src/fs_atomic.rs` writes a temporary file, `sync_all`s it and
    renames it over the target.
  - Hex, SHA-256, base64, TOML and randomness are already workspace dependencies: `sha2`, `hex`, `base64`,
    `toml`, `rand`, `ed25519-dalek` in the root `Cargo.toml` `[workspace.dependencies]`. The root manifest
    must not be changed for this research.
- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O; only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. The scratch harness is not
  a crate in this workspace and must not become one.
- **Design text.** Read `docs/rework/03-architecture-decisions.md` RW-37 (helpers, report, route coverage,
  identity, state, tests) and RW-39 (three identities), `docs/rework/04b-attestation-encoding.md` in full,
  `docs/rework/04-target-architecture.md` §8.3–8.4 and §9.1, and R-10 / D-28 / D-29 in
  `docs/rework/99-open-decisions.md`.
- **Environment.** `docs/rework/investigations/H-wsl2-research-environment.md` records Ubuntu 26.04.1 LTS,
  kernel `6.18.58.1-microsoft-standard-WSL2`, systemd 259.5, nftables 1.1.6. Confirm what is true now. The
  harness's basic suite must run without systemd, without root-only features, without a physical network
  and without a VPN.
- **Report naming.** `docs/rework/investigations/` currently holds `00-inventory.md`, `A-backend.md`
  through `H-wsl2-research-environment.md`. Take the next free uppercase letter at execution time; `L` is
  the expected value if nothing else lands first. Never overwrite an existing report.

Proposed outputs of this prompt, and nothing else:

- `docs/rework/investigations/<next letter>-r10-safety-attestation.md` — the report.
- `docs/rework/investigations/probes/r10/` — the standalone scratch harness, its fixtures and any helper
  scripts.

## 5. Evidence discipline

Label every finding with exactly one of:

- **demonstrated in the isolated scratch harness** — the harness produced it, with test name and output;
- **observed on WSL2** — seen in the live environment, read-only, with the command and output;
- **not demonstrated** — untested or inconclusive, with the reason;
- **requiring conventional Linux validation** — depends on a real host, another kernel, another
  filesystem, systemd, root-level netlink visibility, or power-loss behaviour.

Rules:

- A synthetic route fixture is not a live kernel observation. A passing Ed25519 test is not evidence of key
  custody in a production helper. A process-level locking test is not proof of power-loss durability.
- Record the environment and toolchain for every run: `uname -r` (if Linux), `rustc --version`,
  `cargo --version`, filesystem type of the scratch directory, and whether the test used fixtures or live
  reads.
- Report contradictions, ambiguities and missing details instead of silently resolving them. When the
  design is under-specified, record the gap and continue with the narrowest defensible reading, stated
  explicitly.
- **Known conflict to inspect and report:** `04b` §1 says a line value is "one or more fields", while
  RW-37's report layout permits `incomplete_reason=<token or empty>` — an empty value. Do not silently
  change either contract. Determine what a strict parser must do with `incomplete_reason=` (empty) when
  `complete=false`, whether any other key can carry an empty value, and what the consequence is for
  round-tripping, hashing and the vectors. Report the conflict, the options and their consequences.
- A green suite never writes "R-10 is resolved". Acceptance is the owner's act.

## 6. Safety and isolation

Mandatory:

1. Use only temporary directories for keys, state and lock files — for example a `tempfile`-created
   directory per test. Never write under a real `/var/lib/domovoi-helper`, `/etc/domovoi`, `/run` or any
   path an installed Domovoi uses. If such a path exists on the machine, treat it as read-only and say so.
2. Never run `domovoi helper init`, `recover` or `rotate-key` against a real installation, and never create
   a real attestation key. The scratch harness owns its keys and deletes them with the temporary
   directory.
3. Route tests use deterministic synthetic fixtures. A live read is allowed only as read-only observation
   (`ip rule show`, `ip route show table all`, `ip -6 route show`, interface addresses). Never add,
   replace or delete a route, rule, address, link, namespace or firewall rule.
4. Do not require `sudo`, systemd, a VPN, a physical NIC or a container runtime for the basic suite. If a
   live read needs privileges that are unavailable, record it and fall back to fixtures.
5. Concurrency tests spawn child processes of the harness, each opening its own file descriptor on the lock
   file. A thread-only mutex test is not evidence of interprocess `flock` behaviour.
6. Failure injection is limited to the scratch filesystem: an unwritable directory, a pre-existing
   `state.tmp`, a truncated state file, a bad checksum, a modified `format`, and a `kill -9` of a child
   during the write sequence. Do not simulate power loss as if it were proven; state what a process kill
   does and does not show about durability.
7. Do not install packages, change system configuration, or leave a background process running. Clean up
   temporary directories and child processes in a way that also runs on failure.
8. No network access is required or permitted for the harness's tests. Verification is local and
   deterministic; randomness for nonces and keys comes from a cryptographic RNG, and any test asserting
   randomness asserts properties, not values.

## 7. Scratch harness design

Create a standalone Rust project at `docs/rework/investigations/probes/r10/`.

- Give it its own `Cargo.toml` with a `[workspace]` table so Cargo treats it as its own workspace. The root
  `Cargo.toml` lists `members = ["crates/*", "xtask"]`, so this directory is not a member — verify that
  with `cargo metadata` before and after, and prove the root workspace's member list is unchanged.
- Do not modify the root `Cargo.toml` or its `[workspace.dependencies]`. Pin the harness's own versions in
  its manifest. Prefer crates the repository already uses (`ed25519-dalek`, `sha2`, `hex`, `base64`,
  `toml`, `rand`, and the standard library's file and locking primitives) so the reproduction is close to
  what production would use.
- Organise it so that each area maps to a named module or test file the report can cite:
  - canonical encoding and decoding;
  - configuration normalisation and hashing;
  - route-state normalisation and hashing;
  - report construction and the strict parser;
  - Ed25519 key generation, signing and strict verification;
  - hub-side nonce issue, single-use store, expiry and pinned-key verification;
  - key lifecycle: init, refuse-to-overwrite, recover, rotate, unpin, re-enroll;
  - state file parsing, checksum, atomic write, temp cleanup and `flock`;
  - generation and safety-identity comparison;
  - `expected_safety` validation and a simulated helper apply path;
  - adversarial and concurrency scenarios.
- Keep fixtures as data files where a test would otherwise be opaque: the C1–C4 and R1–R4 inputs, the
  invalid-input vectors, and synthetic route snapshots.
- The harness may print or assert; it must not write outside its temporary directories.

**Vector discipline.** Do not hard-code the expected digests as the implementation's inputs. Construct the
exact bytes from the documented algorithms, hash them, and compare against the published values. Where a
vector disagrees, treat it as a finding: report the bytes you produced, the bytes expected, the lengths and
the first differing byte. Do not "fix" `04b` and do not adjust a fixture to make a digest match.

**Route fixtures.** Build synthetic snapshots that express policy rules, several tables including `main`
and `local`, IPv4 and IPv6, multiple interfaces and gateways, a VPN or tunnel route to a management
endpoint, route replacement, route deletion and a failed or partial read. The harness's route layer must
take that snapshot through an interface that a live reader could also implement, so the same code path
produces both fixture and live evidence.

## 8. Experiment and test matrix

Run every row. Each row needs a named test or a named check in the report. Where an RW-37 test name maps
directly onto the harness, use that name; otherwise name the scratch test after the behaviour. Mark a row
**not demonstrated** with the reason when it cannot be done here.

| ID | Area | Required coverage | Method | What it establishes |
|---|---|---|---|---|
| A1 | Encoding | Every valid C1–C4 vector: exact bytes, length and SHA-256 | Build config lines from the TOML inputs, hash, compare | That the config encoding and hash algorithm reproduce the contract exactly |
| A2 | Encoding | C2 equivalence: semantically equal TOML, different spelling and order, same hash | Parse both inputs, normalise, hash | That normalisation is value-based, not text-based |
| A3 | Encoding | C3 narrowing a CIDR changes the hash; C4 defaults and mandatory units | Hash each, compare with the published vectors | That content changes and defaults behave as specified |
| A4 | Encoding | R1–R4 route vectors, including the empty route state and the compound selector with a delimiter and space in an interface name | Build route lines, hash, compare | That the route encoding, sorting and percent-encoding reproduce the contract |
| A5 | Invalid encoding | Every invalid-input example in `04b`: an extra field from a raw pipe byte, lowercase hex, encoded literal byte, CIDR with host bits set, non-RFC-5952 IPv6, duplicate lines, unsorted lines | Feed each to the strict decoder | That invalid input is rejected rather than normalised |
| A6 | Invalid encoding | Beyond `04b`: missing final LF, CR, BOM, blank line, trailing space, unknown key, missing key, out-of-order key, wrong field count, truncated escape, raw non-ASCII byte, wrong `format` | Parser-level cases, one assertion each | That the parser is strict on boundaries the vectors do not enumerate |
| B1 | Signature | Raw-byte signing over `domovoi-safety-attest-v1\n` ‖ report bytes, and verification with `verify_strict` | Sign, verify, compare signature length 64 | Domain separation and signature shape |
| B2 | Signature | Verification uses the pinned 32 bytes, not `helper_key_id` | Sign with key A, present key id of A with key B, verify | That a key identifier cannot stand in for a key |
| B3 | Signature | Altered report, altered signature, wrong key and truncated signature are all rejected | Mutate one byte and re-verify | Tamper detection |
| B4 | Parser | Signature-first ordering: a report that would fail both signature and parse is rejected as a signature failure, and the received bytes are the ones parsed | Instrument the verification path; feed a valid signature over an invalid report and an invalid signature over a valid report | That the hub never reserialises before verifying, and never normalises |
| C1 | Key enrollment | Public-key exchange: exactly 32 bytes, standard padded base64, other lengths rejected; the hub computes the fingerprint from the bytes | Encode a key, decode, recompute the fingerprint | The exchange format and fingerprint derivation |
| C2 | Key enrollment | `first_contact_does_not_pin_key`, `key_id_never_used_for_verification`, `key_substitution_by_agent_is_detected` | Simulate an enrollment store with no automatic pin | That trust requires the out-of-band comparison |
| C3 | Key lifecycle | `init` creates a key and `generation=1` only when nothing exists and refuses to overwrite; `recover` retires and starts at 1; `rotate-key` starts at 1; unpin blocks; re-enrollment replaces the pin | Scripted sequences over a temp directory | The lifecycle rules and their fail-closed edges |
| D1 | Nonce | 32 random bytes; per-host binding; 60-second validity; single use | Issue, verify, reuse, age past 60 s with an injected clock, present another host's nonce | Nonce semantics without sleeping |
| D2 | Nonce | `helper_attestation_rejects_replayed_nonce`, `attestation_rejected_for_other_host`, expired and missing nonce | Library calls with explicit timestamps | The replay and freshness guards |
| E1 | Config hash | Normalisation of CIDRs, mandatory protected units, canonical unit suffixes, de-duplication, defaults `3600`/`0` | Build config lines from varied inputs | That the effective configuration is what is hashed |
| E2 | Config hash | Invalid TOML, invalid CIDR and invalid unit name produce no hash and `complete=false` with `incomplete_reason=config_invalid` | Feed each input | Fail-closed configuration handling |
| F1 | Route hash | Multiple tables and rules, IPv4 and IPv6, endpoint lookups, `endpoint_route` and selected `route` lines | Fixture through the route layer | That the route state is assembled as specified |
| F2 | Route hash | `nondefault_route_change_for_endpoint_changes_generation`, `irrelevant_route_change_keeps_generation`, `route_replacement_and_deletion_detected` | Three fixtures differing in one property each | Which changes matter and which do not |
| F3 | Route hash | `multiple_tables_and_rules_observed`, `multiple_interfaces_and_gateways_observed`, `vpn_tunnel_route_to_hub_observed` | Fixtures plus, if possible, a read-only live observation | Coverage of non-default topologies |
| F4 | Route hash | `incomplete_route_data_fails_closed`: a failed lookup, an unreadable table, a partial read | Fixture with an injected read error | That incomplete state never produces a hash |
| G1 | Generation | Relevant change increments; irrelevant change, containment-set membership change and boot-only change do not; rotation restarts at 1 | State machine over the temp state file | Generation semantics |
| G2 | Generation | `generation_never_decreases_under_same_key`, `hub_rejects_generation_regression` | Restore an older state file; feed the hub simulation | Regression rejection, including after an old state file is restored |
| H1 | State persistence | Exact state file format, field order, checksum over preceding bytes, `0600` file and `0700` directory | Create, parse, compare bytes | That the format and permissions are as specified |
| H2 | State persistence | Fail-closed conditions: `state_missing`, `state_corrupt`, `state_key_mismatch`, `key_missing`, `uninitialized` | One case each with its exact `incomplete_reason` | The documented fail-closed set |
| H3 | State persistence | Atomic write sequence, `fsync` of file and directory, leftover `state.tmp` deleted under the lock at startup, interrupted write leaves the old file intact | Inject a pre-existing `state.tmp`, a partial write, and a child `kill -9` mid-sequence | The write protocol's observable guarantees, and its limits |
| I1 | Concurrency | Multiple independent processes contending on `flock(LOCK_EX)`, no conflicting generation updates, `concurrent_attestations_serialize_generation_updates` | Spawn N children, each read-recompute-write, then inspect the final state | Interprocess serialisation |
| I2 | Concurrency | State cannot change between the comparison and the apply within the held lock; a simulated apply sees the state it compared | Instrument the simulated apply | The lock's scope, as specified |
| J1 | Safety races | `config_change_between_attestation_and_apply_refused`, `network_change_between_attestation_and_apply_refused`, `reboot_between_permit_and_apply_refused`, `generation_change_between_permit_and_apply_refused` | Change one input between the two simulated steps | Refusal `safety_changed` on each mismatch |
| J2 | Safety races | `helper_fails_closed_when_safety_state_unknown`; a config error mid-apply | Inject an unreadable config and a failed read | `safety_unknown` |
| K1 | Expected safety | Both tagged forms parse; a missing tag, unknown tag, missing field and extra field are refused `malformed_safety` | Parser cases | The tagged value's shape |
| K2 | Expected safety | Verified comparison over the full five-field identity, not a subset | Change one field at a time | That every field is compared |
| K3 | Adversarial agent | `helper_refuses_target_overlapping_exclusion_even_if_agent_requests`, `helper_refuses_overlap_without_expected_safety`, `exclusion_overlap_refused_even_on_unverified_human_path` | Simulated apply with fabricated, omitted and `unverified` expected-safety values | That enforcement is independent of what the request says |
| K4 | Adversarial agent | `agent_cannot_fabricate_effective_safety_set`: an attestation built from agent-supplied data fails signature verification | Attempt to sign with a different key and to alter relayed bytes | That only the helper's key produces an accepted attestation |
| L1 | Unverified path | `human_containment_with_no_pinned_key_uses_unverified_path`, `…_with_expired_attestation_…`, `…_with_invalid_signature_…`, `…_with_incomplete_routes_…`; `unattended_containment_refused_in_every_unverified_case`; `agent_refuses_unverified_with_policy_label` | Simulated agent and hub decision functions | The unverified human-only path and the policy refusal |
| L2 | Unverified path | `human_approved_containment_shows_uncovered_paths`; a fabricated identity is never present in an `unverified` value | Construct each reason and inspect the value | That `unverified` carries no identity |
| M1 | Limits | `agent_limit_stricter_than_helper_applies`, `agent_limit_above_helper_is_capped`, `helper_active_count_is_authoritative`, mutation limit enforcement | Simulated apply with limit combinations | That helper maxima cannot be widened from above |
| M2 | Freshness | `stale_attestation_blocks_unattended_containment`, `confirmation_survives_reboot_only_when_identity_otherwise_equal`, `permit_from_before_reboot_refused` | Injected clock and boot id | The 5-minute rule and the reboot exception |
| N1 | Cost | Cost of recomputing the configuration and route state per request in the scratch harness | Time N repetitions over a fixed fixture; report median, spread, N and the machine | A local measurement with stated limits, not a production threshold |
| O1 | Live observation | Optional read-only route observation on WSL2 as a contrast to the fixtures | `ip rule show`, `ip route show table all`, `ip -6 route`, `ip addr` | What a live host looks like; not a validation of the hash contract |

Rows that reference RW-37 test names must keep those names and state whether the test was reproduced,
adapted or marked **not demonstrated**.

## 9. Ordered instructions for the executor

1. **Inspect the repository and confirm the design and source state.** `git status --short --branch`;
   confirm the branch. Grep for `helper`, `attest`, `safety`, `flock`, `ed25519`, `SO_PEERCRED` under
   `crates/` and record what exists. If attestation, helper or safety-identity code already exists, record
   it and reconcile this brief before continuing; stop and report if the difference changes what you would
   build.
2. **Read RW-37 and `04b` again before implementing the harness**, plus RW-39's identity boundary,
   `04 §8.3–8.4`, `04 §9.1` and R-10 / D-28 / D-29.
3. **Create the isolated scratch workspace** at `docs/rework/investigations/probes/r10/` with its own
   `[workspace]` table; prove with `cargo metadata` that the root workspace membership is unchanged.
4. **Implement encoding and hash reproduction first** and make A1–A4 pass against the published vectors
   before building anything on top.
5. **Implement report construction, key handling, signing and the strict parser**, then B1–B4 and A5–A6.
6. **Implement nonce issuance, expiry, single-use storage and hub-side pinned-key verification**, then
   C1–C3 and D1–D2.
7. **Implement configuration and route state, generation and safety identity**, then E1–E2, F1–F4, G1–G2.
8. **Implement persistent state, checksums, atomic writes and interprocess `flock`**, then H1–H3 and I1–I2.
9. **Implement expected-safety comparison, the simulated apply path and the adversarial scenarios**, then
   J1–J2, K1–K4, L1–L2 and M1–M2.
10. **Run the complete matrix**, including the optional live read, and the cost measurement N1.
11. **Inspect every failed, skipped and inconclusive result.** A failing test is a finding, not a nuisance:
    decide whether the harness is wrong, the vector is wrong or the design is ambiguous, and report which.
12. **Write the report** at the path rule in §10 and keep the harness, fixtures and any scripts needed to
    reproduce it.
13. **Validate that nothing production changed:** `git status --short` shows only
    `docs/rework/investigations/` additions; the root `Cargo.toml`; `crates/`; `docs/rework/03`,
    `04`, `04b`, `06`, `07`, `99`; and the design pack are untouched. Confirm `pr-body.md` and other
    untracked user files are untouched.

Stop and report if the current source or design contradicts an assumption that affects the research, if a
vector cannot be reproduced after a second careful attempt, or if a test needs privileges or hardware this
environment does not have.

## 10. Report requirements

File: `docs/rework/investigations/<next free letter>-r10-safety-attestation.md` — `L` is expected if
nothing else has taken it. Never overwrite an existing report.

Required sections:

1. **Environment and toolchain** — OS, kernel if Linux, filesystem of the scratch directory,
   `rustc`/`cargo` versions, harness dependency versions, and which rows used fixtures versus live reads.
2. **Safety statement** — every temporary directory, key, state file and child process created and
   removed; confirmation that no real installation, config or route was touched.
3. **Harness design** — paths, modules, fixtures, and how to run the suite.
4. **Vector results** — one table per group: vector, expected length, actual length, expected digest,
   actual digest, result. Include the invalid-input vectors with the exact rejection message.
5. **Encoding and parser findings** — what the strict parser rejects, what it accepts, and any case the
   contract does not settle.
6. **The `incomplete_reason` conflict** — the `04b` §1 versus RW-37 reading, the options, the consequences
   and what the harness currently does.
7. **Signature, key and nonce findings** — including the key-id-is-a-label test, substitution, replay and
   expiry.
8. **Route-state findings** — which changes alter `route_state_hash` and which do not, with the fixture
   deltas; the VPN/tunnel case; incomplete reads.
9. **Generation and identity findings** — the increment rules, the reboot exception and regression
   rejection.
10. **State, atomic write and locking findings** — including what was injected, what was observed, and the
    explicit limits of process-kill evidence.
11. **Adversarial and expected-safety findings** — what a compromised agent can and cannot achieve, restating
    the documented residual risk without inventing a fix.
12. **Unverified human path findings** — the reasons covered, the policy refusal, and the "not protected"
    list.
13. **Cost measurement** — method, sample size, results and their limits.
14. **Test matrix status** — every row from §8 with pass, fail, not demonstrated or requiring conventional
    Linux validation.
15. **Unresolved ambiguities and contradictions** — each with the evidence and the decision it needs.
16. **Items requiring conventional Linux validation** — at minimum: live netlink observation on a real
    host with policy routing and a VPN, multiple systemd versions where relevant, a real filesystem's
    crash behaviour, root-only file visibility, and helper key custody in a real process boundary.
17. **Non-acceptance statement** — R-10 stays open; nothing here is production-ready; D-28 and D-29 are
    unchanged.

## 11. Completion criteria

- All C1–C4 and R1–R4 vectors reproduced exactly, with lengths and digests, or a reported contradiction
  with the bytes you produced.
- Every invalid-input case in `04b` rejected, plus the additional boundary cases in A6.
- Signature-first verification and strict parsing of the original bytes demonstrated.
- Nonce randomness properties, 60-second expiry, single use and host binding tested without sleeping.
- Key enrollment, no-automatic-pin, rotation, recovery, unpin and every fail-closed state tested.
- Route-state behaviour tested with synthetic fixtures, including relevant versus irrelevant changes,
  replacement, deletion, VPN/tunnel and incomplete reads.
- State checksum, atomic-write sequence, temp cleanup and interprocess `flock` tested.
- Adversarial-agent behaviour and the `expected_safety` limitation demonstrated, not argued.
- Every untested or inconclusive area recorded as **not demonstrated** with a reason.
- No production file, root manifest, protocol, schema, migration or architecture decision changed.
- R-10 remains unaccepted.

## 12. Open questions this prompt must not close

- D-28 and D-29 are not changed, and their acceptance is not implied.
- No new trust or authorization architecture is decided. A helper-verifiable hub permit remains out of
  scope and would be a separate architectural change.
- `expected_safety` is never described as proof of authorization.
- Route-state hashing is not extended to unrelated routes, and no "any route change invalidates" claim is
  made.
- The `04b` byte-level contract is not changed to suit a parser or test preference; the
  `incomplete_reason` conflict is reported, not resolved.
- WSL2 evidence is not treated as conventional Linux evidence.
- R-10 is not accepted, and the helper ADRs owned by prompt 44 are not authored here.
- The cost measurement is not turned into a production performance threshold or a dispatch budget.

Any proposed design change is a finding for owner review, not a silent edit.

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

For this research prompt, read that report as follows: "implemented behaviour" means the scratch harness
and the research artefacts it produced, never production functionality; "tests added" means the scratch
harness test scenarios and fixtures; "database and protocol changes" must be "none"; "security" means how
the harness kept its keys and state in temporary directories, cleaned up, and stayed read-only on the
host. Do not invent product behaviour to satisfy the template.
