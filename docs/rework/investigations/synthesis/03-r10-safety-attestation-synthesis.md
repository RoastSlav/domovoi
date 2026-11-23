# R-10 synthesis: helper safety attestation

Status: synthesis of research evidence. **R-10 is not resolved, accepted or closed by this document.** Every "recommendation" below is a proposal awaiting an owner decision; nothing in `03`, `04`, `04b`, `99` or the ADRs is changed. Acceptance is the owner's act.

Scope: attestation encoding, trust, state, `expected_safety` and fail-closed behaviour. The isolation and delivery side (accounts, sockets, unit sandboxes) is covered by a separate synthesis; it is referred to here only for key custody.

Notation. Source ids: **L** = `L-r10-safety-attestation.md` (the harness report), **M** = `M-r3-r9-r10-real-user-pass.md`, **N-pi** = `N-pi-r3-r9-r10-systemd257.md`, **N-add** = `N-addendum-r10-harness-on-pi.md`, **S** = `S-host-c-r3-r9-r10-ubuntu-kernel7.md`, **U** = `U-host-t-r3-r9-ubuntu2404-systemd255.md` (OS-level custody only; no toolchain, so no harness run); `§n` is a section of that report. Labels: **[H]** demonstrated by the harness (fixtures, simulated hub and helper); **[OS]** demonstrated at operating-system level with a stand-in process; **[code]** read from source in this synthesis, not executed; **[rec]** recommendation; **[idea]** idea beyond the evidence; **[ND]** not demonstrated.

## 1. Scope, sources, environments

### 1.1 Sources read

- Evidence: L (G-1 to G-20, §15), N-add, N-pi (§3.5, §3.9, §3.10), M (R-10 parts: §3 rows (a), (e), (f), findings F-4 to F-8), S (§3.4, §3.8, §3.9), and the harness code in `probes/r10/src` (`hub.rs`, `helper.rs`, `safety.rs`, `decision.rs`, `coverage.rs`, `config.rs`, `state.rs`, `lines.rs`, `encoding.rs`) and `probes/realuser`.
- Contract: `04b-attestation-encoding.md` (read fully), `03` RW-37 (and RW-13, RW-39 where linked), `99` R-10 and D-28, ADR 0032 (questions C1 to C12), the planning index rows `helper-adrs`, `three-identities-helper-and-skill-runner` and `containment`.
- Code: `crates/domovoi-proto` (`ExpectedSafety`, `RunCapability`, `AttestSafety`, `SafetyAttestation`, `HelperIdentity`, `HelperStatus`, `Sha256Bytes`, `Base64Bytes`), `crates/domovoi-core/src/knowledge/paths.rs` (`UnverifiableReason`).

### 1.2 Environments

| Id | Environment | What ran |
|---|---|---|
| E1 | WSL2, Ubuntu, x86_64, kernel 6.18.58, ext4 in a VM, Ryzen 5 7600X, rustc 1.97.0 (L §1) | full harness (95 tests), live read-only route observation O1 (L §14), real-user custody run on systemd 259.5 with two real accounts (M) |
| E2 | Raspberry Pi 5, aarch64, Debian 13, kernel 6.18.60, ext4 on a USB 5400 rpm HDD, user-local rustc 1.100.0 (N-add, N-pi §1) | full harness (95 tests, raw output counted: 95 `ok` lines), OS-level custody stand-in (N-pi §3.5), `fsync` cost of a state write (N-pi §3.9) |
| E3 | Ubuntu 26.04.1, x86_64, kernel 7.0.0-43, ext4 on NVMe, AppArmor and Yama active, distribution rustc 1.93.1 (S §1) | full harness (95 tests), cost with `--nocapture`, OS-level custody stand-in on a kernel with Yama (S §3.4), full-payload state write cost (S §3.9) |

All three harness runs were by an unprivileged user on fixtures. The repository's `rust-version` is 1.94 (root `Cargo.toml`) [code]; the Pi's distribution Rust 1.85.1 failed to build the harness (`File::lock` needs 1.89, `sha2` 0.11.1 needs 1.88; N-add), so the workspace MSRV is not affected by that limit.

### 1.3 What "3 of 3" means

The harness reads synthetic route snapshots, an injected clock and an injected `boot_id`. A pass on three hosts therefore shows portability across CPU architecture, kernel, filesystem and toolchain for the encoding, hashing, signing, parsing, state file, `flock` and decision logic. It adds **nothing** about netlink semantics, policy routing, VPNs or the real kernel `boot_id`. Only E1 read a live host (O1, read-only; it showed a trivial single-interface host). A live read on E3 was made but its output was deliberately not kept (S §2); on E2 none was made.

Custody evidence is of two different kinds and is kept apart in section 2: the harness code behind a real user boundary ran once (M, E1), while E2, E3 and the fourth host of U used an OS-level stand-in process, not the harness.

## 2. Consolidated findings

### 2.1 Vector and test groups

"Reproduced" counts hosts where the group's tests passed (per-file counts identical on all three: concurrency 3, config_routes 11, cost 1, encoding_vectors 5, enrollment_nonce 14, expected_safety 30, parser_strictness 8, signature 10, state_lifecycle 13).

| Group | Result | E1 | E2 | E3 | Reproduced | Limit |
|---|---|---|---|---|---|---|
| Config vectors C1 to C4 (320, 320, 320, 198 bytes; C1 = C2 digest) | all four digests match byte for byte on first attempt (L §4) | pass | pass | pass | 3 of 3 | inputs from `04b`; only the TOML-to-lines path is exercised |
| Route vectors R1 to R4 (583, 585, 25, 123 bytes) | all four match (L §4) | pass | pass | pass | 3 of 3 | R1 is not a valid output of the selection rule; R4 uses a kernel-illegal name (L §8): they prove encoding, sorting and hashing only |
| Invalid-input vectors (7 inputs) | each rejected with a distinct error (L §4) | pass | pass | pass | 3 of 3 | |
| Parser strictness (BOM, CR, no final LF, blank, trailing space, dup, unsorted, non-UTF-8, raw non-ASCII, leading zeros, empties) | each rejected with its own error; `render(parse(x)) == x` (L §5) | pass | pass | pass | 3 of 3 | selector ambiguity (G-10) is also demonstrated, not repaired |
| `incomplete_reason` empty-value conflict | default policy "empty only when `complete=true`"; literal `04b` reading rejects every complete report (L §6) | pass | pass | pass | 3 of 3 | harness policy, not contract |
| Signature, domain separation, `verify_strict`, forged and replayed input | bare-report signature does not verify; altered byte, 63-byte signature, bad base64, 31/33-byte keys refused; nonce not burnt by forgery (L §7) | pass | pass | pass | 3 of 3 | random draws only property-tested |
| Key, fingerprint, pinning, key substitution | fingerprint computed by hub; no pin on first contact; substitution refused (L §7) | pass | pass | pass | 3 of 3 | step-up modelled as a boolean |
| Nonce (single use, host binding, expiry, injected clock) | pass (L §7) | pass | pass | pass | 3 of 3 | boundaries `>= 60` and `>= 300` are harness readings |
| Acceptance boundary (`complete=false` signed report) | `SignedIncomplete`, nothing stored, gate returns `attestation_missing` (L §7) | pass | pass | pass | 3 of 3 | |
| Route-state (multiple tables, rules, gateways, VPN, replacement, deletion, incomplete reads, irrelevant change) | each changes or keeps the hash as expected (L §8) | pass | pass | pass | 3 of 3 (fixtures) | no netlink; only E1 live read, trivial host |
| Generation and identity | +1 per change; membership change keeps it; boot-only change keeps it; rotation and recover restart at 1; regression rejected; **repeat accepted** (L §9) | pass | pass | pass | 3 of 3 | the repeat test documents a gap |
| State file, atomic write, interrupted write, tokens | exact format, modes, checksum, stage order, `SIGKILL` mid-write leaves old file (L §10) | pass | pass | pass | 3 of 3 | durability not shown; not run as root |
| Concurrency (`flock`, 8 processes, kill -9) | generations 2 to 9 without duplicate (L §10) | pass | pass | pass | 3 of 3 | no run without the lock; wall time 9.36 s on E2 against 0.03 s on E3 (raw output); cause not established |
| Adversarial and `expected_safety` | forged set refused; compromised agent cannot widen enforcement but can omit the guard; strict tagged parser (L §11) | pass | pass | pass | 3 of 3 | |
| Races (config, network, reboot, generation between steps) | each `safety_changed`; comment-only edit does not (L §11) | pass | pass | pass | 3 of 3 | route change between recompute and apply untestable |
| Unverified human path | decision functions give the right reason, card lists "Not protected" (L §12) | pass | pass | pass | 3 of 3 | step-up, typed confirmation, audit persistence [ND] |
| Limits (agent stricter, helper authoritative, TTL, mutation) | pass (L §11) | pass | pass | pass | 3 of 3 | in-memory limiter, plain set size |

### 2.2 Cost, including the write

| Measurement | E1 (WSL) | E2 (Pi) | E3 (Ubuntu) | Source |
|---|---|---|---|---|
| compute config and route lines and hashes (fixture, 5 rules, 5 routes), median | 43.8 us | passed, not recorded | 113.9 us | L §13, S §3.8 |
| hub strict parse and hash check, median | 75.5 us | not recorded | 167.1 us | same |
| full attest without write (lock, read state, compute, sign), median | 515.1 us | not recorded | 784.9 us (p90 787.0) | same |
| compute with 2000 unrelated routes | 476.0 us | not recorded | 853.6 us | same |
| state write (write `state.tmp`, `fsync`, `rename`, `fsync` directory), median | not measured | 67 ms (p90 212, max 722; 2-byte payload, 200 cycles, USB HDD) | 1.213 ms (p90 1.475, p99 2.082, max 6.242; real 385-byte payload, NVMe); 1.243 ms for 2 bytes | N-pi §3.9, S §3.9 |
| same on tmpfs | n/a | 0.01 ms | 0.017 ms | same |

Reconciliation with L §13: L's 515 us (E1) and S's 785 us (E3) are the cost of the no-write path in a dev (unoptimised) build, and L states that it excluded the write and the `fsync`. The 0.8 ms on E3, the 1.2 ms write on E3 and the 67 ms write on E2 come from different hosts; only E3 has both, so "about 2 ms for an attest that changes state on NVMe" is an addition of two medians, not an end-to-end measurement. The device matters (55 times between E3 and E2), the payload does not (S §3.9). Release builds were not measured.

Consequences from code [code] (`helper.rs` `advance`, `attest`, `apply_contain`):

- A write happens only when the hashes changed **or `boot_id` differs from the persisted one**: the first attest after every reboot pays one full write even when nothing else changed.
- `apply_contain` also calls `advance`, so an apply that meets changed content writes state before it refuses with `safety_changed`.
- The write happens inside the `flock` (the session holds the lock across it), so a concurrent attest or apply queues behind it. Measured tail on the USB HDD: 212 ms p90 and 722 ms maximum per write; on E3 6.2 ms maximum. For containment this is a latency floor on a worst-case collision, not a correctness problem.
- Steady state (unchanged state) costs no write. Attestations are requested on `hello`, on `agent_status`, every 15 minutes, on Re-check and before permit issuance (RW-37), so the write cost is bounded by change events, not by request rate.
- The harness orders the write (including both `fsync`s) before signing, so no signed `complete=true` report ever names a generation that is not durable [code]; this order must be a stated rule (section 5.6).

### 2.3 Custody

| Evidence | Host | What it is | Result |
|---|---|---|---|
| M §3 rows (a), (e), (f) | E1 (systemd 259.5, two real accounts uid 999 and 997) | **the harness code** (`HelperDir`, `attest`, `Hub`) behind a line protocol on a unix socket, run as root by a unit; probes as the two real users | both users refused for stat, open, copy, create, rename, link, chmod, unlink, `flock` on 0700/0600 files; `/proc/<pid>` `environ`, `fd`, `mem`, `maps`, `ptrace`, `process_vm_readv` refused (with and without `ProtectProc=invisible`); forgery by own key `KeyMismatch`, foreign signature or changed byte `SignatureInvalid`, replay `Nonce(Unknown)`, honest relay by the agent user verifies; permissions unchanged after restart and `SIGKILL` |
| N-pi §3.5 | E2 (no LSM, no Yama) | OS-level stand-in (a root process holding a key file) probed by real `domovoi-agent` and `domovoi-skill` accounts | same refusals; DAC only |
| S §3.4 | E3 (Yama `ptrace_scope=1`, AppArmor) | same stand-in; also Yama same-uid test | same refusals; Yama separates same-uid siblings (`EPERM` on attach) |
| U §3.5, §3.6 | fourth host (Ubuntu 24.04.5, systemd 255.4, kernel 6.8; Yama 1, AppArmor; harness not run) | same kind of stand-in (a root process holding a key file) probed by real probe accounts; `gdb` as well; a docker-group account | same refusals; `gdb -p` refused for both accounts and allowed for root; Yama same-uid result as on E3; the docker-group account read the key through a read-only bind |

Not shown anywhere: the production helper binary (none exists), a sandboxed helper unit, `gdb` on E3 (`gdb -p` was refused on E2 and on the fourth host), the cross-uid denial separated from Yama (M F-5: Yama was left as found, so independence from Yama is an inference). The RW-37 test `helper_attestation_key_not_readable_by_agent_or_skill` is covered only as an adapted probe (M, E1).

M F-8 changes the weight of the rollback finding: with the layout's modes an agent cannot touch the state file at all; a rollback needs an actor who can already write the helper directory, which also reads the key. See G-15.

## 3. Analysis

Each item: spec, what the harness read, security consequence, options, recommendation (**[rec]**, awaiting owner decision). "Weakens fail-closed" is answered per item. Extra findings found while reading code and specs in this synthesis are numbered X-1 and following; they are not in L.

### 3.1 Spec contradictions to resolve (04b against RW-37)

**G-1 `incomplete_reason` empty.** Spec: `04b` §1 "value is one or more fields"; RW-37 `incomplete_reason=<token or empty>`. Harness: empty only for this key and only when `complete=true` (L §6). Consequence: the literal `04b` reading rejects every complete report, so nothing could ever be verified: that fails closed but makes the feature unusable; "accept empty anywhere" lets `complete=true` carry a reason or `complete=false` carry none. No adversary gain either way. Options: exception (harness); marker value (`-`) changing signed bytes of every complete report; accept anywhere. **[rec]** exception, written into `04b` §1, with the consistency rule "empty iff `complete=true`". A marker would be equally sound but changes RW-37 text for no security gain.

**G-2 Hash when none exists.** Spec: RW-37 hashes are 64 hex; `04b` §3 says an invalid configuration "yields no hash". Harness: 64 zeros in incomplete reports (L §15). Consequence: a verifier must know to ignore the sentinel; if it ever accepted a zero hash in a complete report, an identity of zeros could be bound to permits. Harness rejects nothing special about zeros in complete reports except via recomputation. **[rec]** one rule: a `complete=false` report has the fixed minimal shape of section 5.4 (zero hashes, no list lines, zero maxima); a `complete=true` report is never accepted unless both hashes equal the recomputation, which a zero value can never satisfy.

**G-3 Singular keys in incomplete reports** (`max_ttl_s`, `max_active`). Harness: `0`/`0`. Consequence: none if the hub never reads them from an incomplete report. **[rec]** covered by the minimal shape (`0`/`0`, required zero).

**G-4 `generation` in fail-closed reports.** Harness: `0` or the persisted value; hub rejects a complete report with generation 0 (a harness rule, `Rejection::GenerationZero` [code]). Consequence: if an incomplete report's generation fed the high-water mark it could poison it; the harness never updates the mark from an incomplete report [code]. **[rec]** incomplete reports carry `generation=0`; complete reports with 0 are rejected; the hub never reads the generation of an incomplete report.

**G-19 UTF-8 subsumption.** Every byte the grammar admits is ASCII, so the UTF-8 check is implied by the raw-byte rule; harness keeps a separate first step for a distinct error (L §5). No security effect. **[rec]** keep (ADR C12): free, and a distinct error helps diagnosis.

### 3.2 Under-specified grammars and behaviours

**G-6 `init` hash content.** `init` writes `generation=1` plus two hash fields; from what is not said (L §9). With real hashes the first report is generation 1; with a zero sentinel it is 2 (demonstrated). Consequence: only an off-by-one against "starts at 1". Options: A, hashes computed at init (ADR C3 recommendation): `init` needs the whole observer and valid `helper.toml` at install time and at `recover`/`rotate-key` time, exactly when the host may be damaged; B, `init` writes `generation=0` with zero hashes and the first complete attestation moves it to 1 (hashes differ from zeros): first report is 1, no observation at init, and "a complete report with generation 0 is invalid" stays true. **[rec]** B, because `recover` runs when state is broken and A would make it fail on an invalid config; A is acceptable if the owner prefers the RW-37 wording ("creates state with generation=1") unchanged. This differs from the ADR's recommendation; the reasoning is not tested.

**G-10 Selector ambiguity.** Items are joined by `;` and the whole string is encoded once, so interface `a;not` and the pair `iif=a`, `not` give identical bytes (demonstrated, L §5). Consequence: two different kernel rule sets can produce the same signed line, hence the same hash: a rule that selects by an interface named `x;not` (inverted) can be indistinguishable from `iif=x` plus `not`. This is an integrity gap of the observation, not of the signature. Options: escape each item; use a separator the name grammar excludes; refuse such names. **[rec]** escape the interface-name value inside the item with the same strict field encoder before joining, then encode the whole selector once as `04b` already says. [code]-checked by construction: for names without `;`, `=`, `%` or other escaped bytes (R4's `br-lan`) the bytes are unchanged, so the R4 digest should not change (to be confirmed by running the vector). This corrects ADR C4's statement that R4 changes. Selector names must be bytes: the harness carries `iif`/`oif` as `String` while `ifname` fields are bytes [code] (X-8).

**G-11 Unspecified grammars.** Harness readings: ifname 1 to 15 bytes; port 1 to 65535; table, priority, metric `u32`; `max_ttl` `<digits><s|m|h|d>`; unit suffixes (service, socket, target, mount, automount, swap, timer, path, slice, scope, device), characters `[A-Za-z0-9:_.@-]` up to 255; container `[A-Za-z0-9][A-Za-z0-9_.-]*`; unknown `[safety]` keys refused; missing `[safety]` = empty; CIDR without prefix refused; retired keys kept in `retired/`. Consequence: ambiguity in a grammar makes the hub and helper disagree, which fails closed (reject) but can split a fleet on upgrade. **[rec]** adopt with the amendments of section 5.2 (ranges for ipproto, ports, `start <= end`; `max_ttl >= 1`; backslash question for unit names, X-9; destroy rather than keep retired private keys, section 5.5).

**G-12 Timing and labels.** Harness: `>= 60` and `>= 300` expire; freshness from hub receipt; no `helper_key_id` cross-check; nonce consumed after signature, parse and hash checks (L §7). Consequences: (a) *effective staleness*: a nonce is valid 60 s and a report is fresh 300 s from receipt, so the observed state can be up to 360 s old at the last moment of use [code] (X-3); the helper compares the full identity at apply time, so the effect is bounded to the permit-issuance decision, not to apply; (b) consuming a nonce on first sight lets an unauthenticated message burn it (harness avoids this, demonstrated). **[rec]** keep the strict boundaries and the late consumption; measure freshness from the nonce **issue** time on the hub's own clock (the helper can only compute after it receives the nonce, so the state is at most 300 s old from issue); do not cross-check `helper_key_id` (keeps "label only"); record `computed_at` for display and skew alarms only. `computed_at` is helper-clock data the hub never validates [code].

**G-13 Missing `unverified` reasons** for a signed but unparseable report and for nonce failures. Harness maps to `report_incomplete` and `attestation_missing`. Consequence: none for safety (both refuse); diagnosis is poorer. **[rec]** keep the closed set of nine on the wire; add a hub-internal `detail` code in the audit/log (section 5.7) instead of widening the wire enum.

**G-16 Apply-time behaviour.** (a) TTL above the maximum: harness refuses (test names say "capped"); (b) `max_active=0`: harness "no containment"; (c) mutation limit: in memory, one hour, no config; RW-39 says the helper limit is `[limits] mutations_per_hour` persisted in a root-owned state file under `/var/lib/domovoi-helper/` [RW-39 text]; (d) active count is a plain set size. Consequences: clamping would silently change a duration the permit and plan named; an in-memory limiter resets on every helper start (the helper is a per-connection instance under `Accept=yes` in 0031's candidate design), so the limit would never hold across instances at all [code: `HelperRuntime` is per process]. **[rec]** refuse above maximum and rename the test; `0` means none; persist the limiter per RW-39 with a boot-aware age rule like RW-38; count the nft set elements from the kernel (`list`), not a local set. Also add the refusal `no_management_cidrs` which the harness already enforces but no source states (X-4).

### 3.3 Security-relevant gaps

**G-5 Reports that cannot be signed** (`uninitialized`, `key_missing`, `key_corrupt`). The helper returns an unsigned error; the hub's nearest reason is `attestation_missing`; the agent's relay is unspecified (L §15). `HelperStatus::Error { reason: String }` already exists as an agent-supplied channel [code]. Consequence: an unsigned reason is by nature untrusted: a compromised agent can invent it. It cannot unlock anything (the gate needs a verified report). **[rec]** the helper returns a closed token; the agent relays it in `agent_status`; the hub shows it as an unverified hint and gates on `attestation_missing`. M F-6 adds that an unopenable lock is reported as `state_corrupt: lock`, misleading: use distinct tokens (section 5.7).

**G-14 No revocation of stored verified state.** Harness: a later incomplete or rejected report does not revoke the earlier verified state; the gate stays open until the report ages out (up to 300 s); for key change the harness had to add a per-host mismatch flag (L §7). Consequence: no bypass (the helper still refuses at apply time with `safety_unknown`, demonstrated), but the hub may offer a permit that the helper will refuse (wasted permits, an inaccurate "verified" display). A signed `complete=false` report from the pinned key is authenticated negative evidence, so revoking on it moves toward fail-closed. Options: C5's three; **[rec]** revoke on signed incomplete (authenticated, cheap), on key mismatch, and on `generation_regressed`; do not revoke on unauthenticated failures (an agent could use them to deny service, though it can already deny service by not relaying). This is slightly stronger than ADR C5 and costs more blocking on transient helper failures.

**X-1 The mismatch flag is sticky** [code] (`hub.rs`): `verify` inserts the flag on a mismatching key; `gate` blocks while `mismatch[host] == pin`; neither `pin` nor a later successful `verify` clears it. Re-pinning the same key leaves the host blocked; only changing the pin does. So one forged attestation from a compromised agent blocks unattended containment until the owner rotates to a different key. This fails closed (denial of service only) and the harness test (`key_rotation_blocks_until_reenrolled`) does not exercise the same-key case. **[rec]** gate on the latest attestation outcome: blocked while the latest received attestation was a mismatch, cleared by the next one that verifies under the pin; every mismatch audited (`helper.key_mismatch`) and alerted. A bogus key followed by an honest relayed report leaves the pinned helper's own signed report as the only thing the gate trusts, so clearing opens nothing.

**G-15 Rollback and generation repeat.** Spec: "under one key it never decreases and never repeats" (RW-37). Harness: a state restored one step behind makes the helper issue generation 3 again for different content; the hub accepts it because only strictly lower is rejected (demonstrated, L §9; M re-ran it). Consequences, from `coverage.rs` [code]: confirmations, permits and dispatch compare fingerprint, generation and **both hashes** (and `boot_id` for permits and dispatch), so a repeated generation with different hashes can never match an older binding: it becomes stale. A repeat with identical hashes means an identical effective set. So the weakness is **detection and alerting**, not an unsafe containment. Preconditions (M F-8): the actor needs write access to the helper directory (root, or a snapshot or backup restore, which restores key and state together). Options and assessment:

| Option | Assessment (reasoning, not tested) |
|---|---|
| Hub high-water mark per key: reject `generation < highest`, and `generation == highest` with a different `config_hash` or `route_state_hash` | closes every repeat the hub has seen; one comparison; needs the mark persisted durably (the harness keeps it in memory) and written with the acceptance |
| Monotonic counter mixed with `boot_id` | a snapshot restore restores the counter with the key; `boot_id` is random per boot and cannot order anything: no help |
| Seal state to the key | state already carries `key_fingerprint`; a restore brings both back: no help against restore, only against editing |
| Do nothing | gap documented |

**[rec]** the hub high-water mark including the equal-generation hash comparison, persisted in the same transaction that accepts the report. Remaining gap: generations the hub never saw (the agent can call the helper directly and advance it) and a restore of the hub's own database.

**X-2 `expected_safety` absence.** RW-37: "required on every `Contain` request; a missing tag is refused"; but helper enforcement step 2 says "when present", and the harness helper accepts absence (demonstrated: `helper_refuses_overlap_without_expected_safety`). The agent-side check refuses absence (`agent_check`), the helper does not. A compromised agent omits it and skips only the identity compare, the documented residual risk. **[rec]** the helper also refuses a missing or malformed tag (`malformed_safety`); the only way to skip the comparison is an explicit `unverified`. This adds no protection against a compromised agent but removes accidental omission by an honest one and makes the skip explicit and loggable.

**X-12 Hub state must be durable.** In the harness pins, nonces, `highest`, stored state and the mismatch flag live in memory [code]. Nonces may be volatile (a restart merely forces re-request); pins, the high-water mark and the verified state must be persisted.

### 3.4 Wire drift

**G-18 `ExpectedSafety` in proto against RW-37** [code]. Facts: `PROTOCOL_VERSION` is 1.2; `expected_safety: Option<ExpectedSafety>` with `#[serde(default)]`; `Unverified { reason: String }`; `Verified { fingerprint: String, boot_id: String, generation: u64, config_hash: Sha256Bytes, route_state_hash: Sha256Bytes }`; internally tagged by `state`. `Sha256Bytes` is checked now: a fixed 32 bytes encoded as **lowercase hex**, uppercase or wrong length refused at decode (`bytes.rs`), which closes the "hash encoding" half of G-18. Remaining drift: reason is a free string (RW-37: nine tokens); `fingerprint` is a bare `String` although it is also a 32-byte lowercase-hex digest; `boot_id` is an unvalidated `String`; no refusal of unknown fields is visible on the enum (whether the serde derive refuses an extra field on an internally tagged enum is not asserted here: write it as a test); the field is optional on the wire although required for `Contain`. `AuthorizationKind` in the same file is the precedent for a closed authority-bearing enum. Section 5.8 lists the changes. Whether 1.2 has shipped decides whether R14 (breaking change bumps the major version) applies: no source says.

### 3.5 Observation limits

**G-7 `rule` selection and the R1/R4 vectors.** `04b`: "every policy rule that can select one of those tables"; harness reads every lookup rule. R1 contains `local_addr=10.8.0.2/24|wg0` that no `endpoint_route` names; R4's `eth|0 x` is kernel-illegal (L §8). **[rec]** add a sentence to `04b` §4 that vectors prove encoding only, and add separate **semantic vectors** (input snapshot to expected lines) for the selection logic (section 7).

**G-8 Unrepresentable routing state** (rule actions other than lookup, route types, scope, protocol). Harness refuses a non-lookup rule with `rule_unrepresentable` and ignores route type. Consequence: a `blackhole`, `unreachable` or `prohibit` route/rule on a management path changes reachability but not the hash if route type is not modelled. **[rec]** the helper reports `complete=false` (token `route_unrepresentable`) for any rule action other than lookup and for any selected route whose type is not `unicast`; adding a `type` field to `route` is the alternative (changes `04b`). The rule is already in ADR 0032 item 12; the token is open.

**G-9 Only the CIDR's network address is looked up** [H]. A more specific route inside a configured CIDR (`192.168.1.128/25 via ...`) is invisible: management traffic to part of the range may leave by another path while the hash stays equal. Fail-closed is weakened for exclusions that are broader than their lookups. Options: look up also the last address (cheap, still partial); attest every route that overlaps a management CIDR (complete, larger reports, more generation churn on hosts with many bridge routes); accept and document. **[rec]** document now; test the overlap option on a Docker host (S noted a dozen bridge routes) before choosing, see idea I-1.

**G-17 Effective set versus report.** `04` §9.1 lists loopback and link-local in the effective set, but the report has no line and the source of `link_peer` in the helper is not specified (harness: injected). The exclusion list used at apply time in the harness is `cidr` lines, hub addresses and link peers only [code]. **[rec]** state that loopback and link-local are always excluded by the helper's code (not attested, since they are constants of the helper version), and specify `link_peer` as observed by the helper from the agent's established connections (open: needs a real-host design).

**G-20 Lock scope.** The `flock` protects helper state, not the kernel; a route change between recompute and nft apply is the accepted race. Not testable in the harness. Idea I-2 narrows it.

### 3.6 Further extra findings

- **X-3** effective staleness 360 s: see G-12.
- **X-4** the harness refuses containment when no `cidr` is configured (`Refused::NoManagementCidrs`) [code]; no source states it. **[rec]** adopt: with no management CIDR the only exclusions are hub and link-peer addresses, which is not a confirmed management configuration.
- **X-5** the helper must check at start (M F-4): directory root-owned 0700, files root-owned 0600 regular and not symlinks, checked on opened descriptors; otherwise fail closed (`permissions_unsafe`). The harness accepted a pre-existing 0755 directory.
- **X-6** `Base64Bytes` caps the report at 64 KiB at decode [code]. A host whose report would exceed it must fail closed (`report_too_large`) rather than emit something the hub refuses to decode. Not tested.
- **X-7** durability order: the state must be durable (file and directory `fsync`) before the report is signed; if `rename` succeeded but the directory `fsync` failed the harness reports `state_write_failed` with the old generation, and the next request reads the new file: generation is not reused [code]. Power loss was never exercised.
- **X-8** selector names as bytes (G-10).
- **X-9** unit-name grammar: the harness excludes `\`, which systemd allows in unit names (for example the output of `systemd-escape`). This is from general systemd knowledge, not from the sources; verify with `systemd-escape --path` on a host. A protected unit with an escaped name would be refused as `config_invalid` today.
- **X-10** mutation limit persistence (G-16).
- **X-11** `computed_at` unvalidated (G-12).

### 3.7 Threat table

| Threat | What the evidence shows blocked | Not blocked or not shown |
|---|---|---|
| Compromised agent | cannot forge an attestation (own key `KeyMismatch`, foreign signature `SignatureInvalid`, replay `Nonce(Unknown)`) [H] and [H, real users, M]; cannot read key, state or lock, or ptrace the helper (M, N-pi §3.5, S §3.4); cannot widen the helper's set, exceed TTL, active or mutation limits (L §11) | can omit or send `unverified` for `expected_safety` and call the helper directly (residual risk, demonstrated); can withhold relays or inject one wrong key (X-1, denial of service) |
| Rolled-back state | strictly lower generation rejected by hub [H]; the layout stops an agent from touching state (M F-8) | equal-generation repeat accepted today (G-15); a restore also restores the key; hub DB restore |
| Replay | nonce single use, host bound, 60 s, a forged reply does not burn it [H] | a genuine report held back by the agent until just before the nonce expires (state up to 60 s older than the receipt time, X-3) |
| Key rotation or loss | rotate and recover restart at 1; per-key high-water; pin kept; mismatch blocks [H]; unpin and host delete block | sticky flag (X-1); retained old private key (G-11); loss of both files needs `init`/`recover` by hand; `init` crash window leaves key without state |
| Stale report | `>= 300` refused at the gate [H]; the helper recomputes at apply [H] | reference clock (G-12); reboot exception applies to confirmations only |
| Race attest, permit, apply | config, network, reboot, generation change each `safety_changed`; a comment-only edit does not; the lock serialises state [H] | kernel route change between recompute and nft apply (G-20); `link_peer` and loopback not attested (G-17) |
| Malicious or mistaken `helper.toml` | invalid file gives `config_invalid`, no hash, containment refused [H]; wide CIDRs only add exclusions (overlap refused) | the file is root-owned, so an actor who can edit it is outside this model; nothing bounds `max_ttl` or `max_active` from above; a narrow CIDR list is caught only by owner confirmation |
| Unattended or human path misuse | every one of nine reasons refuses a policy request; human path needs step-up and typed confirmation and lists "Not protected" [H, decision functions] | the authenticated surface and audit persistence [ND] |

## 4. Conclusions

### 4.1 Supported by the evidence

1. The `04b` encoding is internally consistent enough to implement: all eight vectors reproduce byte for byte on three architectures and kernels, and the listed invalid inputs are rejected with distinct errors (L §4, section 2.1).
2. Signature-first verification with a pinned key, strict parsing of the received bytes, hash recomputation and a signed-incomplete rejection work as RW-37 describes, as a harness (L §7).
3. A compromised agent cannot forge an attestation or read the key on four hosts at OS level (the three of E1 to E3 and the fourth host of U, which ran no harness), and with the harness code behind real users once (M, N-pi §3.5, S §3.4, U §3.5).
4. Atomic state write, `flock` serialisation and kill -9 behaviour work on ext4 on NVMe, USB HDD and a VM (L §10, N-pi §3.9, S §3.9). The steady-state attest costs no write; a changing attest adds one write whose cost is set by the device.
5. The races named in RW-37 are refused `safety_changed`; helper-side enforcement (overlap, TTL, active, fail-closed on unknown state) holds whatever the request says (L §11).
6. The weaknesses named in G-1 to G-20 are real, and several (G-1, G-2, G-6, G-10, G-15, X-1, X-2) have a concrete low-cost fix (section 5).
7. The rollback repeat is a detection gap rather than an unsafe containment (confirmations and permits bind both hashes), and needs write access to the helper directory (G-15, M F-8).

### 4.2 Not supported, or open

- Anything about live route observation: policy routing, VPN, multiple tables, route types, `ip route get` semantics with `iif`, `fwmark`, `uidrange` (fixtures only; L §16).
- Power-loss durability, `ENOSPC`, the `rename`-to-directory-`fsync` window, other filesystems (SIGKILL keeps the page cache; N-pi §3.9 F4, S §3.9).
- The production helper: no `helper` subcommand, `safety.attest`, `SO_PEERCRED` check or key code exists. Custody through the real binary and a sandboxed helper unit are [ND].
- The authenticated surface: step-up, typed confirmation, audit persistence, `safety_assurance` (L §12).
- Cost in a release build, with netlink, a process start and a socket round trip; the Pi harness cost.
- Whether Yama or AppArmor are needed for custody (M F-5; they were not separated).
- Systemd versions older than 255 and non-Ubuntu/Debian distributions, SELinux.
- The two excluded RW-37 tests: `helper_attestation_key_not_readable_by_agent_or_skill` (adapted only) and `monitoring_unaffected_by_missing_management_paths` (not run).
- Contradictions between sources that remain to be settled (section 8.2).

## 5. Recommendations as implementation requirements

All of this section is **[rec] awaiting owner decision**. Where it departs from the harness or from an ADR 0032 recommendation, it says so. The requirement words MUST and MUST NOT describe what an implementation must do if the owner adopts the recommendation.

### 5.1 Normative encoding rules (proposed final form)

Unchanged from `04b` §1 to §3 unless listed.

- **E1.** The strict percent-encoding, typed fields, sorted repeated lines, hash prefixes and signing prefix of `04b` stay as written.
- **E2.** `incomplete_reason` is the only key whose value may be empty, and it is empty if and only if `complete=true`. (G-1)
- **E3.** A report with `complete=false` has the minimal shape of 5.4. A report with `complete=true` MUST have non-zero hashes equal to the recomputation from its own lines, `generation >= 1` and an empty `incomplete_reason`. (G-2, G-3, G-4)
- **E4.** The selector's `iif` and `oif` values are the interface-name bytes encoded with the strict field encoder **before** the items are joined by `;`, and the joined selector is then encoded once as a field. Names without escaped bytes are unchanged, so R4 should not change. (G-10)
- **E5.** Interface names are carried as bytes everywhere (selector, `local_addr`, `route`, `endpoint_route`).
- **E6.** The helper MUST emit `complete=false` (`route_unrepresentable`) for a policy rule whose action is not lookup and for a selected route whose type is not unicast, never a hash that omits it. (G-8)
- **E7.** Loopback and link-local are excluded by helper code and are not in the report. (G-17)

### 5.2 Grammars (ABNF-style; `%x` ranges; "decoded" means after percent-decoding)

```abnf
; ---- wire characters
literal     = ALPHA / DIGIT / "." / "_" / ":" / "/" / "@" / "-"
HEXUC       = DIGIT / %x41-46                       ; 0-9 A-F, never lowercase
pct         = "%" HEXUC HEXUC                       ; MUST NOT encode a byte that is in `literal`
fieldchar   = literal / pct
field       = 1*fieldchar                           ; decoded form: 1 or more octets
HEXLC       = DIGIT / %x61-66
; ---- scalars
uint        = "0" / (%x31-39 *DIGIT)                ; canonical decimal; range given per use
u32         = uint                                  ; value <= 4294967295
u64         = uint                                  ; value <= 18446744073709551615
port        = uint                                  ; 1..65535
prefix4     = uint                                  ; 0..32
prefix6     = uint                                  ; 0..128
family      = "inet" / "inet6"
ip          = ip4 / ip6                              ; ip6 = lowercase, compressed per RFC 5952
cidr        = ip "/" prefix                         ; host bits zero
ifaddr      = ip "/" prefix                         ; host bits kept
hash64      = 64HEXLC
hex16       = 16HEXLC
boot-id     = 8HEXLC "-" 4HEXLC "-" 4HEXLC "-" 4HEXLC "-" 12HEXLC
timestamp   = 4DIGIT "-" 2DIGIT "-" 2DIGIT "T" 2DIGIT ":" 2DIGIT ":" 2DIGIT "Z"   ; valid calendar time
token       = %x61-7A *63(%x61-7A / DIGIT / "_")    ; incomplete_reason, 1..64 chars
; ---- names (decoded octets)
ifname      = 1*15OCTET                             ; wire grammar only; see note A
unit        = 1*255unitchar                         ; canonical: carries a known type suffix
unitchar    = ALPHA / DIGIT / ":" / "_" / "." / "@" / "-"   ; note B: "\" under review
unit-suffix = "service" / "socket" / "target" / "mount" / "automount"
            / "swap" / "timer" / "path" / "slice" / "scope" / "device"
container   = (ALPHA / DIGIT) *254(ALPHA / DIGIT / "_" / "." / "-")
; ---- config duration (helper.toml `max_ttl`)
max-ttl     = 1*DIGIT ("s" / "m" / "h" / "d")       ; no sign, no sum, lowercase; value >= 1, seconds fit u64
; ---- route and rule values
local-addr  = ifaddr "|" ifname-f
route       = family "|" u32 "|" cidr "|" (ip / "-") "|" ifname-f "|" u32
rule        = family "|" u32 "|" selector-f "|" u32
endpoint    = ip "|" u32 "|" (ip / "-") "|" ifname-f "|" ip
link-peer   = ip "|" port
ifname-f    = field                                  ; encoded ifname
; selector (before the outer field encoding)
selector    = "all" / item *(";" item)               ; items in this fixed order, each at most once
item        = "from=" cidr / "to=" cidr / "iif=" ifname-e / "oif=" ifname-e
            / "fwmark=0x" hexn "/0x" hexn / "ipproto=" uint8 / "sport=" range16
            / "dport=" range16 / "uidrange=" range32 / "not"
ifname-e    = field                                  ; strict-encoded ifname bytes (E4)
hexn        = "0" / (%x31-39 / %x61-66) *HEXLC       ; lowercase, no leading zeros, <= 8 digits
range16     = uint "-" uint                          ; each <= 65535, start <= end
range32     = uint "-" uint                          ; each <= u32 max, start <= end
```

Notes. **A** the kernel's own name rules (non-empty, under 16 bytes, no `/`, no whitespace, believed to exclude `:`) are not enforced on the wire so that R4 stays a valid encoding vector; the helper only emits names the kernel returned. Check `ip link add 'a:b' type dummy` on a host before relying on the `:` belief. **B** the harness excludes `\`; systemd allows it in unit names (X-9, unverified here). **C** the harness accepts `u32` for ipproto, ports and ranges and does not enforce `start <= end` [code]; the narrower ranges above are a recommendation and need the matching tests. **D** `ip6` equals RFC 5952 only as far as the standard formatter does: single-zero-group and IPv4-mapped corners have no vector; add them.

### 5.3 The unspecified pieces stated once

| Piece | Rule |
|---|---|
| table, priority, metric | `u32` canonical decimal |
| port | 1 to 65535 |
| `max_ttl` | grammar `max-ttl`; `0` refused as `config_invalid` (the harness accepted it) |
| `max_active` | non-negative integer; `0` means no containment; default `0` |
| unit names | `unit`; no suffix gets `.service`; case-sensitive |
| container names | `container`; names stay names, ids resolve at apply time |
| unknown keys under `[safety]` | refused (`config_invalid`); missing `[safety]` equals empty |
| hash representation in `expected_safety` | 64 lowercase hex characters (already what `Sha256Bytes` enforces) for `config_hash` and `route_state_hash`; the same for `fingerprint`; `boot_id` as `boot-id` |

### 5.4 Report format, fail-closed reports, what is signed

Complete report: exactly the RW-37 layout, with E2 to E5.

Incomplete report (minimal shape, [rec]; the harness instead carries partial lines and a mix of real hashes and zeros):

```text
format=1
helper_key_id=<hex16>
nonce=<hex64>
boot_id=<boot-id>
generation=0
config_hash=0000000000000000000000000000000000000000000000000000000000000000
route_state_hash=0000000000000000000000000000000000000000000000000000000000000000
complete=false
incomplete_reason=<token>
computed_at=<timestamp>
max_ttl_s=0
max_active=0
```

(no repeated lines). The hub requires exactly this shape for `complete=false`, skips hash recomputation, and never stores or uses any value from it other than the token and the receipt time. Why: the information in an incomplete report is not trusted for any decision, and a fixed shape removes every sentinel ambiguity (G-2 to G-4). Cost: harness test `incomplete_route_data_fails_closed` and `incomplete_reason_empty_value_conflict_is_visible_in_three_readings` need small changes; the owner may instead keep the harness form.

Signed bytes: `"domovoi-safety-attest-v1\n"` followed by the report bytes, for complete and incomplete reports alike (a report is signed whenever a usable key exists). When there is no usable key the helper returns an unsigned closed token (`uninitialized`, `key_missing`, `key_corrupt`, `permissions_unsafe`, `lock_unavailable`), the agent relays it as a hint, and the hub gates on `attestation_missing`.

Contract tokens for `incomplete_reason` (RW-37 and L): `state_missing`, `state_corrupt`, `state_key_mismatch`, `config_invalid`, `lookup_failed`. Harness additions to ratify or drop: `state_write_failed`, `config_unreadable`, `rule_unrepresentable` (proposed rename `route_unrepresentable`), further route-read tokens (L §8), `report_too_large` (proposed, X-6).

### 5.5 Key lifecycle

| Event | Requirement |
|---|---|
| init | root only; refuses if either `attest.key` or `state` exists; creates the directory 0700 root, key with exclusive create 0600 (`fsync` file and directory), lock file, then `state` with `generation=0` and zero hashes (option B of G-6; option A writes computed hashes and `generation=1`); prints the fingerprint; never runs implicitly. A crash between key and state leaves key without state: `recover` is the way out (nothing is pinned yet) |
| rotate-key | needs key and state; writes new key and new state, sequence restarts; the old **private key is deleted** after the new files are durable, and the old public key, fingerprint and state are kept under `retired/<ts>/` for audit [rec; the harness keeps the private key at 0600] |
| recover | for lost or damaged state; needs at least one of the two files; same retirement as rotate-key; always a new key |
| retire | hub side: unpin or delete host removes the pin; containment blocked |
| fingerprint | `domovoi helper fingerprint` computes from the private key file and prints 64 hex in groups of four plus the 16-character id |
| start-up checks | directory and files root-owned, modes 0700/0600, regular files, opened without following symlinks, checked on descriptors; lock openable; else fail closed (X-5) |

Key custody is as in the separate isolation synthesis: root-owned 0700 directory; the key is protected from the agent by file permissions and from the skill runner by file permissions plus the unit's mask (a mask in the agent unit would remove the agent's own socket). Reason for deleting the old private key: RW-37 never trusts an old key again, so the retained copy has no use and is a credential that remains valid against any pin the hub still holds; "recover" may follow a suspected compromise.

### 5.6 State file, recovery, rollback defence

- Format unchanged (RW-37): `format=1`, `key_fingerprint`, `generation`, `config_hash`, `route_state_hash`, `boot_id`, `checksum=<sha256 of all preceding bytes>`, each LF-terminated, parsed strictly; any deviation is corrupt.
- Write: `state.tmp` written and `fsync`ed, `rename`, directory `fsync`, all inside the lock, and **before** the report that names the new generation is signed. A failure at any step yields `complete=false`, `state_write_failed`, no generation change visible to the hub.
- A leftover `state.tmp` is deleted under the lock. Missing, corrupt or mismatching state is never reset automatically.
- Rollback and repeat: the hub high-water mark of 5.7 step 10. No helper-side change.

### 5.7 Hub verification procedure and refusal tokens

Wire `unverified.reason` stays the closed nine: `key_not_pinned`, `key_mismatch`, `attestation_missing`, `attestation_stale`, `signature_invalid`, `report_incomplete`, `generation_regressed`, `path_uncovered`, `confirmation_stale`. The hub-internal `detail` (audit and logs) is richer. Contract = C, harness = H, proposed = P.

| Step | Check | On failure: wire reason / detail | Nonce | Origin |
|---|---|---|---|---|
| 0 | issue `attest_safety{nonce}`: 32 random bytes, bound to host, store issue time | n/a | issued | C |
| 1 | decode `public_key` (padded base64, exactly 32 bytes), `report` (at most 64 KiB), `signature` (exactly 64 bytes) | `signature_invalid` / `bad_encoding` | kept | H |
| 2 | pin lookup: none | `key_not_pinned` | kept | C |
| 2 | received key differs from pin (byte compare) | `key_mismatch`; audit `helper.key_mismatch`; latest outcome = mismatch (X-1) | kept | C, P |
| 3 | `verify_strict(pin, "domovoi-safety-attest-v1\n" + report, signature)` over the received bytes | `signature_invalid` | kept | C |
| 4 | strict parse of the same bytes (UTF-8, grammar, order, duplicates, canonical values, `format` is 1) | `report_incomplete` / `report_malformed` or `format_unsupported` | kept | H |
| 5 | `complete=true`: recompute both hashes from the report's own lines | `report_incomplete` / `hash_mismatch` | kept | C |
| 5 | `complete=false`: shape equals 5.4 | `report_incomplete` / `report_malformed` | kept | P |
| 6 | nonce: found, same host, age < 60 s | `attestation_missing` / `nonce_unknown`, `nonce_other_host`, `nonce_expired` | consumed once found | H |
| 7 | `complete=false` and valid: record reason token and receipt time for display; revoke verified state (G-14) | `report_incomplete` / `helper_incomplete:<token>` | consumed | P |
| 8 | `generation >= 1` | `report_incomplete` / `generation_zero` | consumed | H |
| 9 | `generation < highest` | `generation_regressed` | consumed | C |
| 10 | `generation == highest` and `config_hash` or `route_state_hash` differs from the stored ones | `generation_regressed` / `generation_repeated` | consumed | P |
| 11 | accept: in one transaction store report bytes, identity, nonce issue time, and set the high-water mark `(generation, config_hash, route_state_hash)` | verified | consumed | C |

Gate on every use (permit issuance, dispatch, confirmation): pinned; latest outcome is not a mismatch; a stored verified state exists; `now - nonce_issued_at < 300 s` (strict) else `attestation_stale`; every confirmation current (fingerprint, generation and both hashes equal, `boot_id` may differ for confirmations only, age under 30 days) else `confirmation_stale`; every candidate path covered by the verified report else `path_uncovered`. A verified identity is stored only by step 11 (ADR gate `ATTEST-2`).

### 5.8 `expected_safety` guard semantics

- The helper requires the tag on every `Contain` request; absent or malformed is `malformed_safety` (X-2).
- `verified{...}`: after recompute and `advance`, the helper compares the full identity (its own fingerprint, `boot_id`, generation, both hashes) against the freshly computed one: unequal is `safety_changed`; uncomputable is `safety_unknown`.
- `unverified{reason}`: only the comparison is skipped; everything after it still runs.
- Always, in this order: canonicalise target; refuse overlap with the actual set (configured CIDRs, hub addresses, link peers, loopback, link-local); refuse if no management CIDR is configured (`no_management_cidrs`); enforce protected units and containers; TTL above the effective maximum is refused (`ttl_above_maximum`, not clamped); active count at the maximum is refused (`max_active_reached`); mutation limit (`mutation_limit`).
- Races: a change between attestation, permit and apply moves the helper's identity and is refused; the lock serialises helper state, not the kernel (accepted race, I-2).

### 5.9 Proto and core changes (list; none is made here)

- `ExpectedSafety::Unverified.reason`: `String` to a closed enum of the nine tokens (snake_case, no unknown-value preservation, as `AuthorizationKind`).
- `ExpectedSafety::Verified.fingerprint`: bare `String` to a 32-byte lowercase-hex type (the same decode rules as `Sha256Bytes`); `boot_id`: validated lowercase UUID type.
- Extra or missing field, unknown tag and unknown reason each refused: named test `expected_safety_refuses_extra_field` and the rest; the mechanism (attribute or custom deserialize) is the implementer's.
- `RunCapability.expected_safety` stays optional on the wire (other capabilities) but is required for `Contain`, enforced in one validating constructor used by the agent and the helper request decoder.
- A report larger than 64 KiB is refused by the helper before signing.
- Core: add `PathUncovered` and `ConfirmationStale` to `UnverifiableReason`; keep `NotAvailable` internal and map it to `attestation_missing` on the wire; add `describe` texts. The wire enum and the core enum then map one to one except `NotAvailable`.
- Protocol version: R14 applies only if 1.2 has shipped; no source says.

### 5.10 Unverified human path

Requires an authenticated human, step-up and typed confirmation; the card states that coverage cannot be verified, gives the reason token and lists every uncovered or unverified discovered path as "Not protected"; the hub persists `safety_assurance = unverified{reason, unprotected_paths}` and audits `containment.unverified_approved`; no `PolicyPermit` is ever issued with `unverified`; the agent refuses `unverified` when `authorization_kind = policy` (`unverified_requires_human`); an `Unverified` value serialises to exactly `state` and `reason`. The helper skips only the identity comparison. Demonstrated as decision functions [H]; the surface is [ND].

### 5.11 TTL, `max_active`, mutation limit

Effective maximum TTL is the smaller of the helper's `max_ttl_s` and the agent's stricter limit; effective `max_active` likewise; the active count comes from the helper's own view of the nft set, never from the request; the mutation limit is persisted (RW-39) with a boot-aware age rule so a helper restart or reboot never reopens the window. Defaults: `max_ttl_s=3600`, `max_active=0`.

### 5.12 Pseudocode

Helper attest:

```text
fn safety_attest(nonce: [u8;32]) -> Result<Signed, UnsignedToken>
  dir  = open_dir_nofollow(HELPER_DIR);  check root-owned, mode 0700   else Err(permissions_unsafe)
  lock = openat(dir, "state.lock", nofollow); check root 0600          else Err(lock_unavailable)
  flock(lock, EX)                                  // held to the end of the function
  unlink_if_exists(dir, "state.tmp")
  key = load_key(dir)?                             // Err: uninitialized | key_missing | key_corrupt
  st  = load_state(dir)                            // Err(token) -> return signed_incomplete(token)
  if st.key_fingerprint != fingerprint(key)  -> signed_incomplete(state_key_mismatch)
  cfg    = effective_config(read("helper.toml"))   // Err -> signed_incomplete(config_unreadable | config_invalid)
  routes = observe(netlink, endpoints(cfg))        // Err -> signed_incomplete(token)
  report = render_complete(nonce, boot_id(), cfg, routes, generation_for(st, hashes))
  if len(report) > 60 KiB                    -> signed_incomplete(report_too_large)
  if hashes != st.hashes or boot_id != st.boot_id:
      next = st with generation + (hashes != st.hashes ? 1 : 0), new hashes, boot_id
      write_state_atomic(next)                     // fsync file, rename, fsync dir; Err -> signed_incomplete(state_write_failed)
  return sign(key, "domovoi-safety-attest-v1\n" + report)
```

Hub verify (steps of 5.7):

```text
fn verify(host, att, now) -> Outcome
  pk = decode32(att.public_key); rep = decode(att.report); sig = decode64(att.signature)  // else signature_invalid
  match pin(host): none -> key_not_pinned
                   != pk -> set_latest(host, mismatch); audit; key_mismatch
  if !verify_strict(pin, PREFIX + rep, sig)          -> signature_invalid
  r = parse_strict(rep)?                              -> report_incomplete(report_malformed)
  if r.complete: if recompute(r) != r.hashes          -> report_incomplete(hash_mismatch)
  else:          if !is_minimal_shape(r)              -> report_incomplete(report_malformed)
  issued = nonces.take(host, r.nonce, now)?           -> attestation_missing(nonce_*)   // consumed here
  if !r.complete: revoke(host); record(token, now); return SignedIncomplete
  if r.generation == 0                                -> report_incomplete(generation_zero)
  hw = high_water(pk)
  if r.generation < hw.gen                            -> generation_regressed
  if r.generation == hw.gen and (r.hashes != hw.hashes) -> generation_regressed(generation_repeated)
  txn { store(host, rep, identity(pk, r), issued); set_high_water(pk, r.generation, r.hashes); set_latest(host, ok) }
  return Verified
```

Apply-time guard:

```text
fn apply_contain(req)
  lock as in attest; key, state, cfg, routes as in attest   // any failure -> refused(safety_unknown, token)
  gen = advance(state, hashes, boot_id)                       // writes if changed, still under the lock
  cur = identity(fingerprint(key), boot_id, gen, hashes)
  exp = req.expected_safety ?? refuse(malformed_safety)
  if exp is Verified and exp != cur                           -> refused(safety_changed)
  target = canonicalise(req.target) ?? refuse(bad_target)
  if cfg.cidr.is_empty()                                      -> refused(no_management_cidrs)
  if overlaps(target, cfg.cidr + hub_addrs + link_peers + loopback + link_local) -> refused(exclusion_overlap)
  if protected_unit_or_container(target)                      -> refused
  if req.ttl > min(cfg.max_ttl_s, req.agent_max_ttl)          -> refused(ttl_above_maximum)
  if nft_active_count() >= min(cfg.max_active, req.agent_max_active) -> refused(max_active_reached)
  if !mutation_limiter.try_take(now)                          -> refused(mutation_limit)
  nft_add(target, ttl); return ok(gen)
```

## 6. Improvement ideas beyond the evidence

Each is an **[idea]**: not demonstrated, not required.

| Id | Idea | Benefit | Cost | How to test |
|---|---|---|---|---|
| I-1 | Attest every route that overlaps a management CIDR instead of only the lookup of its network address (G-9) | closes the "more specific route inside a CIDR" blind spot | larger reports; more generation churn on hosts with many bridge routes (S saw about twelve Docker bridge routes) | measure generation churn over a day on a Docker host; add a fixture for `192.168.1.128/25` expecting a hash change |
| I-2 | Re-observe the route state after the nft transaction; if `route_state_hash` moved, delete the element and report `safety_changed_during_apply` | narrows the accepted kernel race (G-20) | a second observation per apply; a revert path | netns test on a real host: change a route between the two observations |
| I-3 | `domovoi helper selftest`: modes and ownership, lock, `fsync` probe, netlink readability, `boot_id`; machine-readable result for the installer and the host page | catches M F-4 and F-6 classes before the first attestation | one command | run on the three environments; a unit test per failure injection |
| I-4 | Hub event when the generation advances by more than one between hub-requested attestations | exposes direct helper calls by the agent and churn | one comparison | harness extension: call the helper directly, then attest |
| I-5 | Helper-side rate limit on `safety.attest` | a compromised agent cannot hold the state lock by hammering | one counter | concurrency test under load; measure apply latency |
| I-6 | Hub-side display of `max_ttl_s` and `max_active` with an alarm above owner-set bounds | the only bound on a mistaken `helper.toml` (nothing bounds it today) | UI text | page test |
| I-7 | Report hub-observed clock skew from `computed_at` (display only, no gate) | diagnoses odd freshness | none | unit test |
| I-8 | Hardware-sealed counter for the generation | defeats snapshot restore | platform specific; out of scope | not proposed |

## 7. Test plan

### 7.1 RW-37 tests and the harness

Every name in the RW-37 test list exists in the harness under its own name (L §14), as a simulation. In an implementation each becomes a test of the real crate (adapted, not copied). **Two were excluded from the harness:**

- `helper_attestation_key_not_readable_by_agent_or_skill`: covered only as an adapted real-user probe on E1 (M §3 row (a), harness code behind a unit) and as an OS-level stand-in on E2 and E3. To cover: a privileged test that starts the real helper under two real accounts and repeats M's probe set (`chk.py`: stat, open, copy, create, `/proc`, `ptrace`, `process_vm_readv`); it needs root and `useradd`, so it is an opt-in job on a disposable host (a hosted Linux runner with `sudo` is a candidate; unverified), plus `selftest` (I-3) on every install.
- `monitoring_unaffected_by_missing_management_paths`: product behaviour. Coverable now in CI without a host: a hub/engine test with the existing always-unverifiable coverage (`UnavailableCoverage`) asserting that evaluation, alerts and human-approved non-containment work continue while containment is refused.

### 7.2 New tests mapped to findings

| Finding | Test name (proposed) |
|---|---|
| G-1, E2 | `incomplete_reason_empty_only_when_complete` |
| G-2 to G-4, E3 | `incomplete_report_has_minimal_shape`, `complete_report_with_zero_hash_rejected` |
| G-6 | `first_attestation_is_generation_one_after_init` |
| G-10, E4 | `selector_encoding_is_injective_for_names_with_separators`, `r4_digest_unchanged_by_inner_escaping` |
| G-12 | `freshness_measured_from_nonce_issue_time`, `computed_at_not_used_for_trust` |
| G-14 | `signed_incomplete_revokes_verified_state` |
| X-1 | `mismatch_cleared_by_next_verified_attestation`, `same_key_repin_clears_nothing_needed` |
| G-15 | `repeated_generation_with_different_hashes_rejected`, `high_water_persisted_across_hub_restart` |
| X-2 | `helper_refuses_missing_expected_safety` |
| G-16 | `ttl_above_maximum_refused_not_clamped` (rename of the RW-37 "capped" test), `mutation_limit_survives_helper_restart` |
| X-4 | `no_management_cidrs_refuses_containment` |
| X-5 | `helper_fails_closed_on_wrong_directory_mode`, `lock_open_error_has_its_own_token` |
| X-6 | `oversize_report_fails_closed` |
| G-8 | `blackhole_rule_or_route_is_incomplete_not_hashed` |
| G-18 | `expected_safety_refuses_extra_field`, `expected_safety_reason_is_closed_enum`, `fingerprint_rejects_uppercase_and_wrong_length` |

### 7.3 Gate tests

Keep and extend (never weaken): `ATTEST-1` (one hub module verifies, only after a pin lookup), `ATTEST-2` (identity stored only from the complete-report path), `ATTEST-3` (fingerprint never from an agent field). Proposed additions: `ATTEST-4` (the signing key type is constructed only in the helper's init, rotate and recover modules); `ATTEST-5` (no code path stores identity from a report with `complete=false`); `ATTEST-6` (the hub high-water mark is written in the transaction that stores the report). The three-layer rules (INV-1) are unchanged.

### 7.4 Golden files and vectors to freeze

- The eight `04b` vectors with their TOML inputs and the invalid-line file (already `fixtures/vectors`, `invalid_lines.txt`).
- New: a full complete report and a minimal incomplete report; the same two signed with a **published test-only seed** (Ed25519 signatures are deterministic, so the signature bytes can be frozen); state files valid and each corrupt variant; `expected_safety` JSON for both tags and each refused form; selector vectors with `;`, `=`, `%` in names; IPv6 corner vectors (single zero group, IPv4-mapped); semantic vectors (snapshot to lines) replacing reliance on R1 for the selection logic.
- Rendered text and wire forms use the repository's `insta` snapshots (R14).

### 7.5 Property and fuzz tests

- Parser: for any bytes, accept implies `render(parse(x)) == x` (canonical iff round trip); random single-byte mutations of a valid report are rejected or re-render identically.
- Field codec: `decode(encode(b)) == b` for all byte strings; `encode(decode(s)) == s` for every accepted `s`.
- Selector: `parse(render(sel)) == sel` for arbitrary interface-name bytes.
- State parser and `expected_safety` parser: the same two properties.
- Generation: any interleaving of changes, restarts and concurrent attests yields a strictly non-decreasing sequence without repeats, while state is not restored.
- Fuzz targets (cargo-fuzz or proptest) on the report parser and the state parser.

### 7.6 CI against real host

| Runs in CI (fixtures, no privileges) | Needs a real host |
|---|---|
| vectors, parser, signature, nonce, pinning, decision functions, state format, atomic write stages, `flock` with child processes, expected-safety parser, hub procedure | netlink observation with policy routing, VPN and multiple tables (needs root and network namespaces; AppArmor blocks unprivileged `unshare -Ur` on E3, S §3.3) |
| `monitoring_unaffected_by_missing_management_paths` | key custody across real accounts; root ownership; `SO_PEERCRED` |
| the ADR gate tests | power-loss durability (a kill of a VM or `dm-flakey`), `ENOSPC`, other filesystems |
| | the real `boot_id`; release-build cost; Pi-class storage latency |

## 8. Exact follow-up edits for the owner (list only; none is made)

### 8.1 Edits

- **`04b` §1:** the `incomplete_reason` exception (E2); the selector escaping (E4) and a sentence that R4 is unchanged if confirmed; the minimal incomplete shape (5.4); the grammars of 5.2 and 5.3; the timing rules of G-12; the note that vectors prove encoding only. **§4:** R1's `local_addr ... wg0` line and R4's kernel-illegal name; add the new vectors of 7.4.
- **`03` RW-37:** `init` hash content or generation 0 (G-6); retention and preconditions of `rotate-key` and `recover` (5.5); the key-change rule as latest-outcome (X-1); revocation (G-14); the hub high-water rule (G-15); "required on every `Contain`" versus "when present" (X-2); the `expected_safety` tag key `state` and hash encoding; loopback and link-local (E7); `no_management_cidrs`; rename the test `agent_limit_above_helper_is_capped`; record that two tests were not run in the harness. **RW-39:** the mutation limit's persistence and window.
- **`99` R-10:** no status change; optionally list the open decisions of section 9. D-28 text is not contradicted.
- **ADR 0032:** replace "run as an unprivileged user on WSL2" by the three environments; add the 785 us and the write costs; revise C3 (option B), C4 (R4 unchanged), C5 (add revoke on signed incomplete and the latest-outcome rule), C6 (add the equal-generation comparison and persistence), C7 (limiter persistence), C8 (hash encoding already enforced by `Sha256Bytes`; fingerprint and boot_id types); add X-1 to X-12 as questions.
- **Proto:** section 5.9. **Core:** `UnverifiableReason` plus two variants and the wire mapping.
- **Planning index:** the acceptance gates of `three-identities-helper-and-skill-runner` and `containment` name R-10 as an accepted input; this synthesis does not change that.

### 8.2 Contradictions found between sources

1. N-pi §3.10 says the harness was **not run** on the Pi (no toolchain); N-add says it **was** (user-local Rust 1.100.0, 95 passed). The raw output shows 95 `ok` lines: N-add supersedes.
2. ADR 0032 describes the harness as WSL-only and cites only 515 us; S adds a second x86_64 host (785 us) and the Pi run.
3. RW-37 "required on every `Contain`" against "compare `expected_safety` when present", and the helper accepts absence.
4. RW-37 test `agent_limit_above_helper_is_capped` against the harness (refuses).
5. `04b` "one or more fields" against RW-37's empty `incomplete_reason`; `04b` "no hash" against RW-37's 64-hex hashes.
6. RW-37 "never repeats" against the demonstrated undetectable repeat.
7. Core `UnverifiableReason` (eight variants, includes `NotAvailable`) against L's name `UnverifiedReason` (nine tokens).
8. ADR C4 says per-item escaping changes the R4 vector; by construction it does not for `br-lan` (to be confirmed by running).
9. ADR C7 says the limiter is persisted "per RW-39"; the harness keeps it in memory.
10. S says document M was not on its branch and not consulted; M exists and agrees with S on custody.
11. The harness invents tokens and refusals not in the contract: `key_corrupt`, `state_write_failed`, `rule_unrepresentable`, `config_unreadable`, `NoManagementCidrs`, `BadTarget`, `TtlAboveMaximum`, `MaxActiveReached`, `MutationLimit`.

## 9. Owner decisions

| # | Decision | Options and cost | Recommendation | ADR |
|---|---|---|---|---|
| D1 | empty `incomplete_reason` | exception; marker; anywhere | exception, with "empty iff complete" | C1 |
| D2 | shape of incomplete reports | minimal shape (small harness change, less diagnostic data); harness form (partial lines, mixed zeros) | minimal shape | C2 |
| D3 | generation at `init` | A hashes computed at init (needs observer and valid config at install and recover); B generation 0 and zero hashes (reworded RW-37) | B | C3 (ADR recommends A) |
| D4 | selector ambiguity | inner escaping (R4 unchanged); other separator; refuse names | inner escaping, before any report is signed in the field | C4 |
| D5 | revocation and key change | none; on key change only; on signed incomplete too; latest-outcome rule for mismatch | revoke on key mismatch, signed incomplete, regression; latest-outcome | C5, X-1 |
| D6 | rollback and repeat | accept; hub high-water with equal-generation comparison; helper-side counter | hub high-water, persisted | C6 |
| D7 | apply-time limits | refuse or clamp TTL; `0` meaning; limiter persistence | refuse; `0` is none; persist | C7 |
| D8 | wire types | list of 5.9; R14 depends on whether 1.2 shipped | adopt 5.9 | C8 |
| D9 | harness grammar readings | adopt with 5.2 amendments; specify afresh | adopt with amendments; verify the `\` and `:` beliefs on a host | C9 |
| D10 | timing | strict boundaries, receipt time or nonce issue time, no id cross-check, late nonce consumption | strict, issue time, none, late | C10 |
| D11 | tokens | widen the wire enum; keep nine and add internal detail | keep nine, detail internal; add the two missing core variants | C11 |
| D12 | UTF-8 step | keep; drop | keep | C12 |
| D13 | helper absence of `expected_safety` | accept; refuse | refuse | X-2 |
| D14 | retired private keys | keep at 0600; delete | delete | G-11 |
| D15 | non-lookup rules and route types | refuse as incomplete; add a type field | refuse as incomplete | G-8 |

## 10. What remains unproven and the cheapest proof

| Unproven | Cheapest proof |
|---|---|
| live route observation with policy rules, tunnels, several tables | run a read-only observer on the Pi and the Ubuntu host against real `ip rule`/`ip route get`, compare with the fixture lookup; then a netns test as root |
| power-loss and crash durability | boot a disposable VM, write state in a loop, kill the VM, check old-or-new and no repeated generation |
| custody through the real helper binary | the privileged real-user probe set (7.1) against the first helper build |
| `\` in unit names and `:` in interface names | `systemd-escape` and `ip link add 'a:b' type dummy` on a disposable host |
| single-zero-group and IPv4-mapped canonical forms | add vectors and run the harness formatter against RFC 5952 examples |
| release-build cost, netlink and socket round trip | run `cost` with `--release` plus a netlink observation timer on each environment; record the Pi cost |
| R4 unchanged under inner escaping | run the vector after the change |
| clearing of the mismatch flag (X-1) | one harness test: forge, then honest relay |
| report size limit (X-6) | generate a fixture with enough routes to pass 60 KiB |
| authenticated surface and audit persistence | hub service tests when the surface exists |
| Yama independence of custody | a run with `ptrace_scope=0` on a disposable host (needs owner consent) |

R-10 stays open: this document records evidence, analysis and proposals only.
