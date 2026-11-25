# 0032 Helper safety attestation

- Status: proposed
- Date: 2026-11-22
- Extends [0007](0007-human-only-approvals.md) and [0029](0029-package-relay-boundary.md) with the proof that the hub relies on before it lets unattended containment run: a signed report of what the helper enforces. Uses the identities of [0030](0030-three-identities.md); the helper's delivery is [0031](0031-helper-and-skill-runner-delivery.md).
- Evidence: stated inline below. The encoding contract this record amends is [`04b`](../rework/04b-attestation-encoding.md), which already exists in the repository as a proposal; the amendments are decided here (Decision 10) and `04b` is updated with the implementation. Planning text referred to as RW-37 (the attestation proposal), RW-12 and RW-39.

This record is **proposed**. The open question on safety attestation stays open. The decisions below are taken; they rest on a harness that simulates the hub, the helper and the route reader with fixtures, run by an unprivileged user on three hosts (WSL 2, Ubuntu 26.04.1 with systemd 259.5, x86_64; a Raspberry Pi 5 running Debian 13, aarch64; Ubuntu 26.04, x86_64), plus OS-level custody probes with a stand-in helper on a fourth host (Ubuntu 24.04.5, x86_64). A green harness is not acceptance. No helper, hub verifier or key code exists. Acceptance needs the items under "What acceptance still needs".

## Context

The design decision on unattended containment says unattended containment on a host needs owner-confirmed, current management paths, and that a path may be confirmed only when a verified helper attestation shows it covered. RW-37 and `04b` give the report, its hashes and the trust rules as a proposal. A harness built from them (below) found about thirty places where the proposal is ambiguous or contradicts itself; this record decides each one.

Facts checked against the repository:

- **The wire is already ahead of the planning text.** `PROTOCOL_VERSION` is `1.2` (`crates/domovoi-proto/src/version.rs:17`). The proto crate has `AttestSafety`, `SafetyAttestation`, `HelperIdentity`, and `RunCapability` with `expected_safety: Option<ExpectedSafety>` marked `#[serde(default)]` (`crates/domovoi-proto/src/messages/actions.rs`).
- **`ExpectedSafety`** is internally tagged by `state` (`Verified { fingerprint: String, boot_id: String, generation: u64, config_hash: Sha256Bytes, route_state_hash: Sha256Bytes }`, `Unverified { reason: String }`). It has no `deny_unknown_fields`: an extra field is ignored. The reason is a free `String`, not the nine-token set of RW-37. The field is optional on the wire although RW-37 makes it required for `Contain`.
- `domovoi-core` has `safety_identity: Option<String>` on the authorization value and a coverage verdict whose unverifiable reasons are `UnverifiableReason` with eight variants, including `NotAvailable`; it lacks `path_uncovered` and `confirmation_stale`. The engine and the hub wire `UnavailableCoverage` only, which is always unverifiable.
- No `helper` subcommand, `safety.attest` operation, `SO_PEERCRED`, `/var/lib/domovoi-helper` code or attestation key code exists under `crates/`.

The harness work found three defects that this record turns into requirements: the hub's key-mismatch flag is sticky (one forged key blocks containment until key rotation), the helper accepts a missing `expected_safety` for `Contain`, and freshness is measured from hub receipt, so observed state can be up to 360 s old.

## Decision

1. **What is signed, by whom.** The helper computes the report and signs it with its Ed25519 key. The agent relays the bytes unchanged. The hub verifies. The signing input is the line `domovoi-safety-attest-v1` followed by LF, then the full report bytes, for complete and incomplete reports alike; the two hashes inside the report use their own prefixes (`domovoi-safety-config-v1`, `domovoi-safety-routes-v1`). Domain separation was demonstrated in the harness (`helper_attestation_signature_verifies`). When there is no usable key the helper returns an unsigned closed token (`uninitialized`, `key_missing`, `key_corrupt` and and the state and permission tokens of item 8); the agent relays it as a hint and the hub gates on `attestation_missing`.

2. **Verification.** The hub checks the signature over the received bytes **first** with `verify_strict` against the pinned key, then parses those same bytes with a strict parser that never re-serialises, then recomputes both hashes from the report's own lines and rejects a mismatch. Each step was demonstrated in the harness (`signature_is_checked_before_parse_and_parser_sees_the_received_bytes`, `signed_report_with_wrong_config_hash_is_rejected_by_recomputation`). The step order is: decode (public key, report at most 64 KiB, 64-byte signature), pin lookup, key comparison, signature, strict parse, hash recomputation or minimal-shape check, nonce, then the generation rules of item 7.

3. **Acceptance, revocation and the gate.** The hub accepts a report as the host's verified safety state only if the signature verifies with the pinned key, the nonce is valid, **and** `complete=true`. A correctly signed `complete=false` report is never accepted as verified; the implementation makes this a rule of the hub, not a property of the parser. Trust follows the **latest outcome** per host: a key mismatch, a validly signed incomplete report or a generation regression revokes the verified state, and one later good attestation restores it. The hub's sticky mismatch flag is replaced by this rule, so a forged key blocks containment only until the next honest attestation, not until rotation. The gate on every use (permit issuance, dispatch, confirmation) is: key pinned; latest outcome is not a mismatch; a stored verified state exists; `now - nonce_issued_at < 300 s` (strict) else `attestation_stale`; every confirmation current (fingerprint, generation and both hashes equal; `boot_id` may differ for confirmations only; age under 30 days) else `confirmation_stale`; every candidate path covered by the verified report else `path_uncovered`.

4. **Key lifecycle** (commands are proposals; none exists).
   - *Init*: `domovoi helper init`, run by the installer, creates the directory 0700, the key (exclusive create 0600, `fsync` of file and directory), the lock and a state file at **generation 0 with zero hashes**, only when neither key nor state exists, and refuses to overwrite either. It needs no valid configuration and no route observer. The first real attestation is generation 1. The helper never generates a key implicitly. A crash between key and state leaves a key without state; `recover` is the way out, and nothing is pinned yet.
   - *Rotate*: `domovoi helper rotate-key` makes a new key and a new state, restarting the sequence. The old **private key is deleted** once the new files are durable; the old public key, fingerprint and state go to `retired/<timestamp>/` for audit. A retained private key has no use, because an old key is never trusted again, and is a credential that stays valid against any pin the hub still holds.
   - *Recover*: `domovoi helper recover` handles lost or damaged state with the same retirement and always creates a new key; continuity cannot be proven after state loss.
   - *Retire*: an unpin on the hub, or deleting the host, removes the pin; containment stays blocked.
   - Start-up checks: the directory and files are root-owned, modes 0700 and 0600, regular files, opened without following symlinks, checked on descriptors; the lock opens. A wrong mode is refused and reported with each path, found and expected mode and owner, never repaired at start (0031 Decision 6). Rotation does not trigger `generation_regressed` because the high-water mark is per key. These behaved as specified in the harness (`helper_state_recovery_requires_explicit_local_action`, `key_rotation_blocks_until_reenrolled`, `unpin_blocks_containment`, `agent_reenrollment_keeps_helper_pin`).

5. **Pinning and fingerprint enrollment.** The hub computes the fingerprint (lowercase hex SHA-256 of the 32 raw key bytes) itself and ignores any value the agent sends; the key id (first 16 hex characters) is a label never used to verify or decide trust, and the hub does not cross-check it. Nothing is pinned on first contact. The maintainer compares the hub's fingerprint with the one `domovoi helper fingerprint` prints on the host, confirms with step-up, and the hub pins the exact 32 bytes (audit `helper.key_pinned`). A different key from the pinned one is a mismatch (item 3); the old pin is kept. Demonstrated in the harness: `helper_public_key_exchange_and_fingerprint`, `first_contact_does_not_pin_key`, `key_substitution_by_agent_is_detected`, `key_id_never_used_for_verification`.

6. **Nonce and freshness.** 32 random bytes, issued for one host, single use, valid for 60 seconds, with the issue time stored (RW-37). Boundaries are strict (an age of 60 s or 300 s is already stale). **Freshness is measured from the nonce issue time**, not from hub receipt, so observed state is bounded by the nonce lifetime and not up to 360 s old. The nonce is consumed late: only after the signature, parse and hashes pass, because consuming on first sight lets an unauthenticated message burn it; a forged reply does not burn the outstanding nonce. Demonstrated with an injected clock; the random draws are only property-tested.

7. **Generation and the hub high-water mark.** A u64 counting changes of the effective safety set (the config lines and the route lines): incremented by exactly one when the freshly computed hashes differ from the persisted ones; unchanged by a containment membership change or by a new `boot_id` with identical content; per key. A change followed by its revert advances it by two, so an earlier permit is refused. The hub stores, per helper and in the **same transaction that accepts the report**, the highest accepted `(generation, config_hash, route_state_hash)`, and rejects a lower generation, and an equal generation with different hashes, as `generation_regressed` (internal detail `generation_repeated` for the second). This closes the rollback-and-repeat gap: the repeat was a detection gap rather than an unsafe containment, because permits and confirmations bind the generation and both hashes, and it needs write access to the helper directory, which also reads the key (F-CUS-7). It needs a small hub table and a migration. A complete report with generation 0 is rejected.

8. **Fail-closed state conditions** (RW-37): key and state valid and matching is the only normal condition. State missing with a key present, state corrupt (checksum, parse, unknown format), state's key fingerprint not matching the key, key missing with state present, both missing, and an interrupted write each refuse all containment and are never reset automatically; recovery needs a local root action. Tokens demonstrated in the harness: `state_missing`, `state_corrupt`, `state_key_mismatch`, `key_missing`, `uninitialized`, plus `key_corrupt` and `state_write_failed`, which the harness added and this record adopts. An unopenable lock or a wrong mode is a distinct condition from `state_corrupt`. Settled (E2): the token is `helper_state_permissions`, the name [0031](0031-helper-and-skill-runner-delivery.md) gives the start-check fault; the attestation work's `permissions_unsafe` is not used. The write is ordered before signing: temp file, `fsync`, `rename`, directory `fsync`, all inside the lock, so no signed `complete=true` report names a generation that is not durable; a leftover temp file is deleted under the lock; a failure yields `complete=false` with `state_write_failed` and no visible generation change.

9. **State, lock, atomic write.** Files `attest.key`, `state` (with a SHA-256 checksum of the preceding bytes) and `state.lock`, as in 0031 Decision 6 and RW-37. Every helper instance takes `flock(LOCK_EX)` and holds it across read, recompute, compare, increment, write and sign or apply. In the harness, eight concurrent processes reported generations 2 to 9 with no duplicate; this shows the lock is sufficient for the scenario, not that it is necessary. A `SIGKILL` mid-write left the old file. Durability across power loss is not demonstrated. A write happens only when the hashes changed or `boot_id` differs from the persisted one.

10. **The encoding is `04b`, with these decisions**. The harness reproduced all eight vectors (C1 to C4, R1 to R4) byte for byte and passed 95 of 95 tests on three hosts and two architectures. That says nothing about live netlink, policy routing or VPN behaviour.
    - **Empty values.** `incomplete_reason` is the only key whose value may be empty, and it is empty if and only if `complete=true`. `04b` §1 gets a one-line exception; the verifier rejects an empty reason on an incomplete report and a non-empty reason on a complete one.
    - **Incomplete reports.** A `complete=false` report has one fixed minimal shape: `format=1`, `helper_key_id`, `nonce`, `boot_id`, `generation=0`, both hashes 64 zeros, `complete=false`, `incomplete_reason=<token>`, `computed_at`, `max_ttl_s=0`, `max_active=0`, no repeated lines. The hub requires exactly that shape, skips hash recomputation and uses nothing from it but the token and the receipt time. A complete report has non-zero hashes equal to the recomputation, `generation >= 1` and an empty reason. There is one parser path.
    - **Selector ambiguity.** Interface names in `iif` and `oif` items are encoded with the strict field encoder **before** the items are joined by `;`, and the joined selector is then encoded once. This was checked by running the vectors in a scratch copy of the harness: **R4 and the other seven vectors are unchanged**, byte for byte and by digest, because R4's selector name `br-lan` is made of literal characters. Only names with a byte outside the literal set change on the wire (the `%` of the inner encoding becomes `%25`); the ambiguity (`a;not` against `iif=a` plus `not`) was reproduced before and is gone after. No such report has been signed in the field, so the change is free until the first release and a format change after it (R14). The 15-byte limit applies to the decoded name; the encoded size is bounded only by the 64 KiB report cap.
    - **Grammars.** The harness readings are adopted with amendments as `04b` text: interface names, ports 1 to 65535, table, priority and metric as canonical `u32`, `max_ttl` as digits plus one of `s m h d` with 0 refused, unit names with a known type suffix, container names, unknown `[safety]` keys refused, `max_active` a non-negative integer defaulting to 0, ranges with `start <= end`. Two beliefs were checked on a host: the kernel refuses `:` and `%` in an interface name but accepts `;`, `|`, `=`, `,` and a backslash; systemd accepts a backslash in a unit name, so the unit-name grammar must allow `\` (the harness grammar would refuse every escaped name). The `04b` field encoding handles `\` as `%5C` unchanged.
    - **UTF-8 step.** Kept: the contract's own rule list stays complete and testable.
    - Vectors C1 to C4 and R1 to R4 are frozen as golden files, and new vectors are added (one per amendment above: an empty `incomplete_reason`, the minimal incomplete shape, an interface name with `;` and `=`, a backslash in a unit name, an `oif` selector, a report at the size cap). R1 contains `local_addr=10.8.0.2/24|wg0`, which no `endpoint_route` names, and R4 uses an interface name the kernel would refuse; the vectors prove encoding, sorting and hashing, not the selection logic.

11. **`expected_safety` and the unverified human path.** `expected_safety` is a consistency guard for the honest path, not proof of a hub permit. **The helper requires it on every `Contain` request and refuses a missing or malformed value** (`malformed_safety`): RW-37's "required on every `Contain`" wins over "compare when present". With `verified{...}` the helper recomputes and compares the full identity: unequal is `safety_changed`, uncomputable `safety_unknown`. With `unverified{reason}` only the comparison is skipped; overlap, maxima, mutation limits and fail-closed behaviour still apply. The hub stores report bytes and the safety identity only from an accepted report and never trusts an agent's description of the helper; an `Unverified` value carries only `state` and `reason`. The unverified human path needs step-up and typed confirmation, shows "not protected" paths, and is audited `containment.unverified_approved`; no policy permit is ever issued with `unverified`. Its decision functions were demonstrated; the authenticated surface was not. A compromised agent can call the helper with `expected_safety` unverified; it still cannot forge an attestation, widen the helper's set or exceed its maxima.

12. **Wire and core types**. Adopted: `Unverified.reason` becomes a closed enum of the nine tokens; `Verified.fingerprint` and `boot_id` become validated types (the hashes already use `Sha256Bytes`); an extra field, an unknown tag and an unknown reason are each refused (`deny_unknown_fields` or equivalent); `RunCapability.expected_safety` stays optional on the wire and is required for `Contain` in one validating constructor used by the agent and the helper's request decoder; a report above 64 KiB is refused before signing; core gains `PathUncovered` and `ConfirmationStale` and maps `NotAvailable` to `attestation_missing` on the wire. Whether this bumps the protocol major version depends on whether 1.2 has shipped; no source says, and R14 applies if it has (E1).

13. **Refusal tokens**. The wire `unverified.reason` stays the closed nine: `key_not_pinned`, `key_mismatch`, `attestation_missing`, `attestation_stale`, `signature_invalid`, `report_incomplete`, `generation_regressed`, `path_uncovered`, `confirmation_stale`. Richer detail (`bad_encoding`, `hash_mismatch`, `nonce_expired`, `generation_repeated`, `helper_incomplete:<token>` and others) stays internal, in audit and logs; there is no wire enum growth.

14. **Apply-time limits**. A requested TTL above the effective maximum is **refused** (`ttl_above_maximum`), not clamped; the effective maximum is the smaller of the helper's `max_ttl_s` and the agent's limit. `max_active = 0` means no containment is allowed, so the default configuration refuses everything (`max_ttl_s` 3600 and `max_active` 0 by default); the active count comes from the helper's own view of the nft set, never from the request. The mutation limit is persisted under the helper state directory with a boot-aware age rule, so a helper restart or reboot never reopens the window. The RW-37 test `agent_limit_above_helper_is_capped` is renamed, because the behaviour is a refusal.

15. **Route-state observation limits and unrepresentable routing**. Only the network address of a configured CIDR is looked up, so routing inside a CIDR changes nothing in the report; loopback and link-local sit in the effective set of the target architecture but have no report line, are excluded by helper code, and the source of `link_peer` knowledge in the helper is not specified (E5). A policy rule whose action is not lookup, and a selected route whose type is not unicast, are **refused as incomplete**: the helper reports `complete=false` with `route_unrepresentable` and refuses containment, never a hash that omits the state. Hosts whose routing the observation cannot represent get no containment.

16. **Races.** A change of configuration, of relevant network state, of `boot_id` or of generation between attestation, permit and apply is refused `safety_changed`; a comment-only edit is not. The lock protects helper state, not the kernel: a route change between recomputation and apply is the accepted race, and the next attestation sees it.

17. **Cost.** Computing the report is cheap: a median of 43.8 us to 113.9 us for the config and route lines, and 0.8 ms (515 us on WSL 2, 785 us on Ubuntu 26.04) for a full attest without write, in a development build on fixtures. A changing attest adds one state write inside the lock, whose cost is set by the device: a median of 1.2 ms on NVMe and 67 ms (p90 212, maximum 722) on a USB HDD, the device mattering and the payload not. The first attest after every reboot pays one write. Release builds, netlink, a process start and a socket round trip were not measured; none of this is a threshold.

## What the checker enforces

Nothing in this record is enforced yet. Today the repository only has the proto types and the always-unverifiable `UnavailableCoverage`. Proposed for the implementation, with gate tests:

- `ATTEST-1`: the verify call is made in one hub module and only after a pinned-key lookup; no other code calls a signature verifier for a safety report.
- `ATTEST-2`: the hub stores a safety identity only from the function that accepts `complete=true`; Decision 3 is pinned by a gate test.
- `ATTEST-3`: the hub never constructs a fingerprint from an agent-supplied field.
- `ATTEST-4`: the verifier rejects an empty `incomplete_reason` on an incomplete report, a non-empty one on a complete report, and an incomplete report whose shape is not the minimal shape (Decision 10).
- `ATTEST-5`: the hub's high-water mark is written in the same transaction as the accepted report and is the only place the highest generation is read from (Decision 7).
- `ATTEST-6`: the helper's request decoder refuses a `Contain` request without a valid `expected_safety` (Decision 11).
- `ATTEST-7`: the hub's gate reads the latest attestation outcome; no code path keeps a per-host flag that only key rotation clears (Decision 3).
- `ATTEST-8`: freshness is computed from the stored nonce issue time; nothing reads hub receipt time for it (Decision 6).
- `ATTEST-9`: rotation and recovery delete the old private key (Decision 4).
- `ATTEST-10`: `ExpectedSafety` and the unverified reason refuse extra fields, unknown tags and unknown reasons, and the reason is a closed enum (Decision 12).
- The `04b` vectors, the new vectors and the invalid inputs run as tests in the crate that owns the parser (R14-style snapshots for the report text), with the selector tests `selector_encoding_is_injective_for_names_with_separators` and `r4_digest_unchanged_by_inner_escaping`.

## What the checker cannot see

- Whether the helper actually computes its report from the live kernel (the harness used fixtures; netlink, `ip route get` semantics with `iif`, `fwmark` and `uidrange`, route types and `nft` set accounting need Linux validation).
- Whether `attest.key` is unreadable by the agent and the skill runner on a real host (0030; custody held at the OS level on four hosts with a stand-in).
- Whether the clock and `boot_id` seen on a host are the kernel's.
- Whether the maintainer really compared the fingerprint out of band.
- Whether a restored state file is a rollback on a host where the hub has no high-water mark yet.

## Unproven

- Live route observation: policy routing, multiple tables, tunnels and gateways (fixtures only; one read-only live observation on WSL 2).
- Real filesystem crash behaviour: power loss, `ENOSPC`, the window between `rename` and the directory `fsync`; ext4 on WSL 2, NVMe and a USB HDD only. `flock` on other filesystems.
- The production helper: no `helper` subcommand, `safety.attest`, `SO_PEERCRED` check or key code exists; custody through the real binary and a sandboxed helper unit are not demonstrated. The tests `helper_attestation_key_not_readable_by_agent_or_skill` (adapted only) and `monitoring_unaffected_by_missing_management_paths` (not run) remain.
- The authenticated surface: step-up, typed confirmation, audit persistence, the hub's persisted `safety_assurance`.
- Release-build cost with netlink and a socket round trip.
- Whether Yama or AppArmor are needed for custody (they were not separated; the Pi has neither).
- The clearing of the hub's mismatch flag under the latest-outcome rule, the 64 KiB report cap (a fixture past 60 KiB), single-zero-group and IPv4-mapped canonical forms (no vectors), and `oif` rendered under inner escaping (only `iif` was exercised).
- Non-UTF-8 interface names (legal in the kernel) need the selector fields to be bytes (E3); the harness carries them as strings.
- Systemd versions older than 255, non-Ubuntu and non-Debian distributions, SELinux.

## Open questions

Decided items are in the Decision section. Settled by the implementation record [0034](0034-helper-attestation-boundary.md), without changing this record's status (the maintainer accepts the open question on safety attestation, not these choices):

| # | Question | Notes |
|---|---|---|
| E1 | Does the protocol major version change (Decision 12)? | Settled: no. 1.2 has not shipped, so the type changes are folded into 1.2 and no 1.3 exists. |
| E2 | One name for the permission-fault token (Decision 8) | Settled: `helper_state_permissions`. |
| E3 | Bytes for interface names in the wire types | Settled: names are bytes in the core types; a non-UTF-8 name is `route_unrepresentable` because the `ip -j` output cannot carry one. |
| E4 | The report size cap | Settled: one cap, `MAX_REPORT_BYTES`, equal to the wire's base64 decode cap (65 536 bytes). |
| E5 | The source of `link_peer` knowledge in the helper and routing inside a configured CIDR | Default taken, awaiting the maintainer's confirmation: `hub_addr` lines come from the literal IP of the hub address in the agent configuration (a name yields none), and there are no `link_peer` lines in this release. |

## What acceptance still needs

This record does not settle the open question on safety attestation. Before this ADR is accepted:

- a real-installer pass with the production helper, hub verifier and installer, including the custody test with the real helper binary;
- the amended grammars written into `04b` and the golden vectors frozen, with the new vectors added;
- live route observation on hosts with policy rules, a tunnel or several tables, replacing the fixture routes;
- the authenticated surface (step-up, typed confirmation, audit persistence) built and tested, and the two excluded RW-37 tests run;
- E1 to E5 settled, and the remaining unproven items run or explicitly accepted as residual.

## Follow-up edits

None of these is made by this change.

- `04b` §1: the `incomplete_reason` exception, the minimal incomplete shape, the inner selector escaping with the statement that R4 is unchanged, the grammars (including `\` in unit names), the timing rules and the note that the vectors prove encoding only. `04b` §4: R1's `local_addr ... wg0` line and R4's kernel-illegal name; add the new vectors.
- The architecture-decisions plan, RW-37: generation 0 at `init`; deletion of retired private keys and the preconditions of `rotate-key` and `recover`; the latest-outcome rule and revocation; the hub high-water rule; "required on every `Contain`"; the `expected_safety` tag name `state` and hash encoding; `no_management_cidrs`; loopback and link-local in the effective set but not the report; rename `agent_limit_above_helper_is_capped`; record that two tests were not run. RW-39: the mutation limit's persistence and window.
- The target-architecture text: the effective set and the helper limit semantics; the attestation key is protected from the agent by file permissions, not by a mask.
- The proto crate and `domovoi-core`: the changes of Decision 12, made with the implementation.

## Alternatives considered

- **A hub-signed permit the helper verifies.** Out of scope: it needs a helper-side trust root, hub signing-key custody and rotation, clock or nonce semantics and a protocol change (RW-37).
- **Trust the agent's description of the helper.** Rejected in the design decision on containment: a compromised agent could then declare any path covered.
- **Generate the helper key implicitly on first start.** Rejected in RW-37: a missing key must fail closed so an attacker cannot force a fresh, unpinned identity.
- **Compute real hashes at `init` (generation 1).** Rejected: init would need a valid configuration and a working observer at install and recovery; generation 0 with zero hashes does not.
- **Keep retired private keys at 0600.** Rejected: no use, and a live credential against any remaining pin.
- **A different selector separator, or refusing interface names that contain `;` or `=`.** Rejected: inner escaping removes the ambiguity without changing any vector, and the kernel accepts such names.
- **Clamp an over-long TTL, or treat `max_active = 0` as unlimited.** Rejected: clamping hides an over-long request and the unlimited default is unsafe.
- **Widen the wire reason enum to carry every detail.** Rejected: detail stays internal; the nine tokens stay.

## Revision (2026-11-22)

Brought up to date with the harness results and the maintainer's decisions. The evidence is stated inline with its hosts and scope. Decided and stated as decisions: the empty-reason exception, the minimal incomplete shape, inner selector escaping (checked by running the vectors: R4 and the other seven are unchanged), revocation by latest outcome, the hub high-water mark persisted in the acceptance transaction, refusal above the maximum TTL and `max_active` 0 meaning none, required `expected_safety` for `Contain`, the amended grammars, freshness from nonce issue time, the nine wire tokens with internal detail, deletion of retired keys, generation 0 at `init`, refusal of unrepresentable routing, and the wire-type changes. Cost figures: 0.8 ms to compute, 1.2 ms on NVMe to 67 ms on a USB hard disk per state write. Added: proposed rules `ATTEST-4` to `ATTEST-10`, open questions E1 to E5, and "What acceptance still needs". Status stays proposed.
