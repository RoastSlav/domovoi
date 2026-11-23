# L: R-10 helper safety attestation, research evidence

Status: research evidence for R-10. **R-10 is not resolved by this report.** Nothing here is production-ready. D-28 and D-29 are unchanged. Acceptance is the owner's act, recorded in the decision register and ADRs.

Evidence labels used throughout: **demonstrated (harness)** = produced by the scratch harness, test name given; **observed (WSL2)** = seen on the live host, read-only; **not demonstrated** = untested or inconclusive, reason given; **needs Linux validation** = depends on a real host, kernel, filesystem or process boundary.

## 1. Environment and toolchain

| Item | Value |
|---|---|
| OS / kernel | Ubuntu on WSL2, `uname -r` = `6.18.58.1-microsoft-standard-WSL2` |
| CPU | AMD Ryzen 5 7600X, `nproc` = 4 in the VM |
| Toolchain | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `cargo 1.97.0 (c980f4866 2026-06-30)` |
| Scratch directory filesystem | `$HOME/r10-tmp`, `stat -f -c %T` = `ext2/ext3` (the ext4 magic number), root disk `/dev/sdd` |
| Source directory filesystem | `/mnt/c/...`, `v9fs` (9p). No test writes there; only `target/` (kept in `$HOME`) and `TMPDIR` are written |
| Privileges | `id -u` = 1000, no root, no sudo used |
| Tools present | `ip` yes, `nft` no |
| Harness dependencies (locked) | ed25519-dalek 3.0.0, sha2 0.11.1, hex 0.4.3, base64 0.23.1, toml 1.2.0 (manifest `1.1.8`), rand 0.10.3, serde_json 1.0.154, tempfile 3.27.0, thiserror 2.0.21. Versions mirror the repository's `[workspace.dependencies]`; `Cargo.lock` was seeded from the root lock |
| Fixtures vs live | Every matrix row except O1 used fixtures and temporary directories. Only O1 read the live host (read-only) |

Environment incident: the WSL root disk was at 100% (another working copy's `domovoi-target` holds 5.1 GB) and the linker (`zig cc`) aborted. Only this harness's own `target` directory and a scratch hello-world were deleted; the rest was left alone. The harness now builds with `CARGO_PROFILE_DEV_DEBUG=0` (about 100 MB). This is an environment limit, not a harness finding.

### Differences between the brief and the repository (recorded before building, none changes what was built)

- `PROTOCOL_VERSION` is already `1.2`, not `1.1`. The proto crate already contains `AttestSafety { nonce }`, `SafetyAttestation { public_key, report, signature }`, `HelperIdentity { public_key }`, `RunCapability` with `expected_safety: Option<ExpectedSafety>`, and `ExpectedSafety` (`serde(tag = "state")`, `Verified{fingerprint, boot_id, generation, config_hash, route_state_hash}`, `Unverified{reason: String}`). Read from source, not executed. Observations against the contract: `reason` is a free `String` rather than the nine-token set; `expected_safety` is `Option` on the wire (the contract makes it required for `Contain`, so a layer above must enforce); no `deny_unknown_fields` appears on the enum in the lines read, so an extra field would be ignored where RW-37 says refuse; the wire encoding of `Sha256Bytes` was not checked. The harness's K1 shows the strict behaviour the contract asks for.
- `domovoi-core` already has a `safety_identity: Option<String>` field on authorization, a `ManagementCoverage` port, and `UnverifiedReason` with eight variants (including `NotAvailable`, lacking `path_uncovered` and `confirmation_stale` from the RW-37 list). The engine wires `UnavailableCoverage` (always unverifiable).
- No `helper` subcommand, `safety.attest` operation, `SO_PEERCRED` use, `/var/lib/domovoi-helper` code or attestation key code exists. `domovoi-agent` uses `nix::Flock` for its journal (the repository's flock pattern; unrelated to the helper).
- `xtask` source scans walk `crates/` only, so the harness under `docs/` is not subject to the 500-line, `unwrap` or process rules. The harness is nevertheless kept free of `#[allow]`.

## 2. Safety statement

- Every key, state file and lock file lives in a `tempfile::TempDir` created under `$HOME/r10-tmp` and deleted at the end of each test. `run.sh` removes `$HOME/r10-tmp` on exit. After the full run `$HOME/r10-tmp` did not exist and `pgrep -fa r10_child` found nothing.
- `/var/lib/domovoi-helper` and `/etc/domovoi` do not exist on this host; none was created or read.
- Child processes (`r10_child`) were spawned only by the concurrency tests, each wrapped in a guard that kills and waits on drop; one is deliberately killed with SIGKILL.
- Live reads (O1) were `ip -4/-6 rule show`, `ip -4/-6 route show table all`, `ip -br addr show`. Nothing was added, replaced or deleted.
- No network access, systemd, VPN, NIC or container runtime was needed. Dependencies were built with `--offline` from the local registry cache.
- Root workspace unchanged: `cargo metadata --no-deps` at the root lists 14 members (13 crates plus `xtask`), none is the harness; `git status` shows only `docs/rework/investigations/` additions, and the root `Cargo.toml` and `Cargo.lock` are unmodified. From the harness directory `cargo metadata` reports its own workspace root with the single member `r10`. (A pre-harness metadata baseline was not captured; the comparison rests on `git status` and the manifest's `members = ["crates/*", "xtask"]`.)

## 3. Harness design

Location: `docs/rework/investigations/probes/r10/` (own `[workspace]`, `.gitignore` for `target/`, `.gitattributes` keeping fixtures byte-exact).

Run: `wsl.exe -d Ubuntu -- bash /mnt/c/Programming/domovoi-overhaul/docs/rework/investigations/probes/r10/scripts/run.sh` (sets PATH, target dir, `TMPDIR`; runs `cargo test --offline`). `scripts/live_observe.sh` is the optional read-only O1 probe.

| Module (`src/`) | Area |
|---|---|
| `encoding.rs`, `lines.rs` | strict percent-encoding, typed fields (canonical iff `render(parse(x)) == x`), selector, typed route and config lines |
| `config.rs`, `hash.rs` | effective configuration, `config_hash`, domain prefixes |
| `routes.rs`, `fixture.rs` | `RouteSource` trait (what a netlink reader would implement), `observe()` selection logic, synthetic snapshots with injected failures |
| `report.rs` | report model, renderer, strict parser, hash recomputation |
| `keys.rs` | Ed25519, fingerprint, key id, domain-separated signing, `verify_strict` |
| `hub.rs` | nonce store, pins and audit, verification pipeline, regression, freshness gate |
| `state.rs` | state file and checksum, atomic write with stage hook, `flock`, `init`/`recover`/`rotate-key` |
| `helper.rs` | simulated `safety.attest` and `Contain` apply under the lock |
| `safety.rs`, `coverage.rs`, `decision.rs` | identity, tagged `expected_safety` and strict parser, coverage and confirmation rules, hub and agent decision functions |
| `bin/r10_child.rs` | child process for interprocess tests |

Test files (`tests/`): `encoding_vectors`, `parser_strictness`, `signature`, `enrollment_nonce`, `config_routes`, `state_lifecycle`, `concurrency`, `expected_safety`, `cost`; shared support in `tests/common/`. Vectors are data files in `fixtures/vectors/` (extracted verbatim from `04b`, plus the C1-C4 TOML inputs) and `fixtures/invalid_lines.txt`. Deviation: synthetic route snapshots are Rust builders in `fixture.rs` and `tests/common`, not data files.

Vector discipline: expected digests appear only as assertion constants. Bytes are built from typed structs and the documented algorithm and compared with the fixture text and digest; a mismatch prints lengths and the first differing byte.

Totals from the final full run: **95 tests passed, 0 failed, 0 ignored** (concurrency 3, config_routes 11, cost 1, encoding_vectors 5, enrollment_nonce 14, expected_safety 30, parser_strictness 8, signature 10, state_lifecycle 13; library and binary unit tests 0).

## 4. Vector results (demonstrated, `encoding_vectors`)

Config hash vectors (`config_hash_matches_vectors`, built from the TOML inputs):

| Vector | Expected len | Actual len | Expected SHA-256 | Actual SHA-256 | Result |
|---|---|---|---|---|---|
| C1 | 320 | 320 | `96f1584f...cbf4b9ea` | identical | PASS |
| C2 | 320 | 320 | `96f1584f...cbf4b9ea` (equal to C1) | identical | PASS |
| C3 | 320 | 320 | `494a37a8...3d42dc4f` | identical | PASS |
| C4 | 198 | 198 | `7f1db0e0...06556baa` | identical | PASS |

Route-state vectors (`route_state_hash_matches_vectors`, built from typed structs, lines given in scrambled order so the sort is exercised):

| Vector | Expected len | Actual len | Expected SHA-256 | Actual SHA-256 | Result |
|---|---|---|---|---|---|
| R1 | 583 | 583 | `082d1afd...d4d1515` | identical | PASS |
| R2 | 585 | 585 | `7eb46fcc...ca1777ad9` | identical | PASS |
| R3 | 25 | 25 | `7947b31b...21996ec8` | identical | PASS |
| R4 | 123 | 123 | `530d9a9e...7ac925a` | identical | PASS |

Full digests are asserted in `tests/encoding_vectors.rs` and printed on `--nocapture` as `VECTOR ...` lines. All eight reproduced on the first attempt; no vector was adjusted. Every vector line also passes the strict line grammar (`every_vector_line_passes_the_strict_line_grammar`).

Invalid-input vectors (`report_rejects_invalid_encoding_vectors`; exact rejection text):

| Input | Rejection |
|---|---|
| `local_addr=192.168.1.20/24\|eth\|0` | `local_addr: expected 2 fields, found 3` |
| `local_addr=192.168.1.20/24\|eth%7c0` | `local_addr: lowercase hex in percent escape: %7c` |
| `local_addr=192.168.1.20/24\|%65th0` | `local_addr: escape %65 encodes a byte of the literal set` |
| `cidr=192.168.1.7/24` | `cidr: not a canonical cidr: "192.168.1.7/24"` |
| `hub_addr=2001:DB8::10` | `hub_addr: not a canonical ip: "2001:DB8::10"` |
| two identical `route=` lines | `duplicate line "route=inet6\|254\|2001:db8::/64\|-\|eth0\|256"` |
| `route=` lines out of order | `unsorted line "route=inet6\|254\|2001:db8::/64\|-\|eth0\|256"` |

## 5. Encoding and parser findings

Demonstrated (`parser_strictness`): the parser works on the original bytes and rejects, with distinct errors: not UTF-8 (own step, first), BOM, CR, missing final LF, blank line, trailing space, line without `=`, unknown key, duplicate single key, out-of-order key, missing key, duplicate repeated line, unsorted repeated line, `format` other than `1`, wrong field count, truncated escape (`local_addr: truncated percent escape`), raw non-ASCII valid UTF-8 (`local_addr: raw byte 0xC3 outside the literal set`), leading-zero integers and addresses, and empty values everywhere except `incomplete_reason`. `render(parse(bytes)) == bytes` holds for a complete report.

A7: `0xFF` and the overlong `0xC0 0x80` give `not valid UTF-8: invalid utf-8 sequence of 1 bytes from index 568`, a different path from the A6 raw-`é` rejection. Observation: every byte the grammar admits is ASCII (literal set, `%`, `|`, `=`, LF), so UTF-8 validity is logically subsumed by the raw-byte rule; the separate step only yields a distinct error.

Cases the contract does not settle are in section 15 (G-1 to G-20). Two parser findings worth stating here:

- **Selector ambiguity** (`selector_encoding_is_ambiguous_for_interface_names_containing_separators`, demonstrated): selector items are joined by `;` and the whole string is percent-encoded once, so an interface named `a;not` and the pair `iif=a` plus `not` produce identical bytes. Kernel interface names may contain `;` and `=`. R4 contains `iif%3Dbr-lan%3Bnot` and cannot distinguish the cases either.
- **UTF-8 subsumption** as above.

## 6. The `incomplete_reason` conflict

Sources: `04b` section 1 says a line value is "one or more fields", and RW-37's report layout has `incomplete_reason=<token or empty>`. A complete report therefore must carry the line `incomplete_reason=` with an empty value.

Two readings exist: (R-A) an empty string is one empty field, so the line is grammatical and every typed key rejects empties individually; (R-B) a value must be non-empty, which contradicts RW-37.

What the harness does (`EmptyReasonPolicy`, default `EmptyIffComplete`): empty is accepted only for `incomplete_reason`, and only when `complete=true`; `complete=false` requires a token `[a-z][a-z0-9_]*`; a token with `complete=true` is refused. Demonstrated in `incomplete_reason_empty_value_conflict_is_visible_in_three_readings` and `no_other_key_may_carry_an_empty_value` (36 other keys of a real report each rejected when emptied as `empty value for <key>`):

| Option | Effect | Result in harness |
|---|---|---|
| Accept empty only when `complete=true` (default) | RW-37 layout works; one key is a documented exception to "one or more fields" | all valid reports parse; round trip byte-exact |
| Reject all empty values (04b literal) | **every `complete=true` report is rejected**, so nothing could ever be verified | `empty value for incomplete_reason` on a complete report |
| Accept empty anywhere | no consistency check between `complete` and the reason | accepted; `complete=false` with empty reason and `complete=true` with a reason become possible |
| Replace the empty value by a marker such as `-` or `none` | would change RW-37's text and every complete report's signed bytes | not run; stated as an alternative only |

Hashes and vectors are unaffected: `incomplete_reason` is excluded from both hashes (`empty_incomplete_reason_does_not_move_the_hashes`) and no C or R vector contains it. Only signed bytes and round-tripping depend on the choice. Decision needed from the owner: which reading is normative, and whether `04b` section 1 should name the exception.

Beyond the named conflict: the same "no value exists" problem appears for `config_hash` and `route_state_hash` when no hash can be computed (G-2), `max_ttl_s`/`max_active` in an incomplete report (G-3) and `generation` when state is unusable (G-4).

## 7. Signature, key and nonce findings

All demonstrated (harness):

- `helper_attestation_signature_verifies`: signature is 64 bytes; signing input starts with `domovoi-safety-attest-v1\n`; the same key's signature does not verify over the bare report bytes (domain separation).
- `key_id_never_used_for_verification`: key B presenting key A's label is a key mismatch against the pin; under the pinned key a report with a wrong `helper_key_id` still verifies. The contract says the id is a label; it does not say whether the hub should cross-check it (G-12).
- `attestation_rejected_for_wrong_key`, `helper_attestation_rejects_forged_report`: altered report byte, altered signature byte, 63-byte signature, invalid base64, 31/33-byte keys and unpadded base64 are all refused, and none of them consumes the nonce.
- `signature_is_checked_before_parse_and_parser_sees_the_received_bytes`: a valid signature over junk reaches the parser, which receives the received bytes verbatim; an invalid signature over a valid report stops at `verify_signature` and the parser never runs; failing both reports the signature failure; a report with one extra trailing LF is rejected, not normalised.
- `signed_report_with_wrong_config_hash_is_rejected_by_recomputation`: a correctly signed report whose `config_hash` differs from its own lines is rejected (`04b` section 3 hub check).
- **Acceptance boundary** (`signed_incomplete_report_not_accepted_as_verified`, `hub_rejects_unverified_safety_report`): a `complete=false` report with reason `config_invalid`, correct pinned-key signature and a valid nonce passes the signature and nonce steps (asserted from the pipeline trace), the nonce is spent, and the result is `SignedIncomplete`. The hub stores nothing: `stored(host)` is `None`, the highest-generation record is untouched, and the unattended gate returns `attestation_missing`; a policy decision is `Refused`. `complete=true` is an independent third acceptance condition.
- Enrollment: `helper_public_key_exchange_and_fingerprint` (44-character padded base64, 64 lowercase hex fingerprint, 16 groups of four, key id is the first 16 hex characters, the hub's `observe` accepts no agent-supplied fingerprint), `first_contact_does_not_pin_key`, `key_substitution_by_agent_is_detected` (pin refused without step-up or with a wrong typed fingerprint; old pin kept).
- Nonce (`nonce_is_32_random_bytes_with_expected_properties`, `nonce_lifetime_and_single_use_with_injected_clock`, `helper_attestation_rejects_replayed_nonce`, `attestation_rejected_for_other_host`, `expired_attestation_nonce_is_rejected_and_an_unauthentic_message_does_not_burn_a_nonce`): property tests only (64 draws unique, not constant, bit balance within 600 of 8192); single use; host binding; expiry with an injected clock and no sleeping; a forged reply does not burn the outstanding nonce.

Findings that the contract leaves open or that the harness had to add:

- **Boundaries.** "Valid for 60 s" and "under 5 minutes": the harness treats age `>= 60` and `>= 300` as expired (stricter reading). The freshness reference clock is hub receipt time; `computed_at` and nonce issue time are alternatives (G-12).
- **When a nonce is consumed.** The harness consumes it only after signature, parse and hash checks pass. Consuming on first sight lets an unauthenticated message burn it; the contract is silent.
- **Stored verified state is not revoked by later events** (`later_incomplete_report_does_not_revoke_earlier_verified_state`, and the rejected report in `hub_rejects_generation_regression`): RW-37 says an incomplete report is not accepted, not that it invalidates the previous verified state. In the harness the hub gate stays open on the older verified report for up to five minutes. The helper itself still refuses at apply time (`safety_unknown`). For key change, the contract says containment is blocked once a different key is seen; the first harness version did not do that (the gate stayed open until the stored report aged out) and was fixed by recording a mismatch flag per host (`key_rotation_blocks_until_reenrolled`). Either way this must be an explicit hub rule.
- **Lifecycle** (`helper_state_recovery_requires_explicit_local_action`, `key_rotation_blocks_until_reenrolled`, `key_rotation_invalidates_previous_pin`, `unpin_blocks_containment`, `agent_reenrollment_keeps_helper_pin`): `init` refuses to overwrite if either file exists; `recover` retires and restarts at generation 1; `rotate-key` restarts at generation 1 and does not trigger `generation_regressed` because highest generation is tracked per key; unpin blocks; re-enrollment replaces the pin and audits `helper.key_rotated`. Open: the old private key moved to `retired/<ts>/` stays on disk with mode 0600 (RW-37 does not say whether a rotated key is retained or destroyed), and `rotate-key` and `recover` preconditions are not specified (G-11).

## 8. Route-state findings

All demonstrated on synthetic fixtures (`config_routes`); not a live kernel observation.

| Change | Fixture delta | Effect |
|---|---|---|
| metric of the route selected for an endpoint | `192.168.1.0/24` metric 100 to 50 | route hash and generation change (`nondefault_route_change_for_endpoint_changes_generation`) |
| unrelated route and address | `172.30.0.0/16` via docker0 plus an address on docker0; a route in table 200 that no rule references | no change (`irrelevant_route_change_keeps_generation`) |
| replacement | main-table IPv6 default gateway replaced | change (`route_replacement_and_deletion_detected`) |
| deletion of the selected on-link route | lookup falls back to the default route | change |
| deletion of everything a lookup needs | `routes.clear()` | `complete=false`, `lookup_failed`, no hash |
| more specific route inside a configured CIDR | `192.168.1.128/25 via 192.168.1.99` | **no change**: only the CIDR's network address is looked up (asserted inside `irrelevant_route_change_keeps_generation`) |

- `multiple_tables_and_rules_observed`: the fwmark rule to table 100 and the table-100 default route appear; five rule lines (v4: 0, 100, 32766; v6: 0, 32766); three `endpoint_route` lines (hub, each CIDR network address).
- `multiple_interfaces_and_gateways_observed`: a link peer reached via eth1 and gateway 10.20.0.1 yields its `endpoint_route`, route and `local_addr`; the unrelated docker0 address is not attested.
- `vpn_tunnel_route_to_hub_observed`: a hub reachable only via `wg0` yields `endpoint_route=10.99.0.1|254|-|wg0|10.8.0.2`, the tunnel address becomes a `local_addr` line (because an endpoint route names `wg0`), and re-pointing the tunnel route changes the hash.
- `incomplete_route_data_fails_closed`: unreadable rules, an unreadable table, a partial table read, unreadable addresses, a failed lookup and a non-`lookup` rule action each give `complete=false` with a reason token and the zero-hash sentinel, never a hash.
- **R1 is an encoding vector, not a semantically valid output** (`r1_vector_is_an_encoding_vector_not_a_semantic_output_of_the_selection_rule`): reproducing R1's rules, routes and addresses through `observe()` with endpoints `192.168.1.10`, `2001:db8::10` and the link peer yields every R1 line except `local_addr=10.8.0.2/24|wg0`, which no `endpoint_route` names (the default route in table 100 names wg0, but the rule says `local_addr` covers interfaces named by `endpoint_route`). R1 does contain `rule=inet|0|all|255` although no table-255 route is attested, which shows the vector reads `rule` as every lookup rule. R4's `local_addr` has no `endpoint_route` and uses the interface name `eth|0 x`, which the kernel would refuse (it forbids whitespace and `/`). The vectors prove the encoding, sorting and hashing, not the selection logic.
- Observed gaps: policy rules with actions other than lookup (blackhole, unreachable, prohibit, goto) and route types (local, broadcast, blackhole, unreachable) have no field in the report. The harness refuses a non-lookup rule with `rule_unrepresentable` (a harness choice); route type is not modelled at all (G-8).

## 9. Generation and identity findings

Demonstrated (`state_lifecycle`):

- Relevant change increments by exactly one (`generation_never_decreases_under_same_key`: initial 1, three distinct contents give 4; repeated identical contents do not increment).
- Nft membership change keeps the generation (`containment_membership_change_keeps_generation`: apply, clear, attest).
- A new `boot_id` with identical content keeps the generation but the new boot id is persisted (`boot_only_change_keeps_generation_but_updates_boot_id`).
- Rotation and recover restart at 1 (section 7).
- `hub_rejects_generation_regression`: after verified generation 5, restoring a valid generation-1 state file makes the helper report 2, which the hub rejects (`GenerationRegressed{highest: 5, got: 2}`).
- **Repeat is undetectable** (`restored_state_can_repeat_a_generation_with_different_content_and_the_hub_accepts_it`): restoring a state file one step behind makes the helper issue generation 3 again for different content (different `route_state_hash`); the hub accepts it because only strictly lower generations are rejected. "Never repeats under one key" holds only while state is not rolled back; neither side can detect a one-step rollback. Possible mitigation (reject the same generation with a different identity) is a design change and is not applied.
- **First generation is ambiguous** (`init_with_sentinel_hashes_makes_the_first_attestation_generation_two`): `init` writes `generation=1` with `config_hash` and `route_state_hash` fields, but the contract does not say from what. With hashes computed at init the first report is generation 1; with a zero sentinel it is generation 2, contradicting "starts at 1".
- Identity comparison covers all five fields (`identity_comparison_covers_all_five_fields`). A change followed by its revert leaves both hashes equal but advances the generation by two, so a permit made before it is refused (`generation_change_between_permit_and_apply_refused`).

## 10. State, atomic write and locking findings

Demonstrated:

- `state_file_has_the_exact_format_checksum_and_permissions`: key order is `format, key_fingerprint, generation, config_hash, route_state_hash, boot_id, checksum`; checksum is SHA-256 over all preceding bytes; directory 0700, `attest.key`, `state`, `state.lock` 0600; the lock file is empty; no `state.tmp` remains. Ownership by root was not testable (uid 1000): needs Linux validation.
- Fail-closed reasons, each with its own exact token, for attestation and for apply: `state_missing`, `state_corrupt` (truncated, bad checksum, unknown format, missing field, empty file, leading-zero generation), `state_key_mismatch`, `key_missing` and `uninitialized` (neither can be signed at all, see G-5), plus `key_corrupt` (not in the contract). None is reset automatically; reading an uninitialized directory creates nothing.
- Atomic write: `write_hook_sees_every_stage_in_order` (tmp written, tmp fsynced, renamed, directory fsynced); `leftover_state_tmp_is_deleted_under_the_lock_at_start`; `interrupted_state_write_never_reuses_generation` (directory made unwritable: the report is `complete=false`, `state_write_failed`, reports the old generation, the old file is byte-identical, and the next request advances exactly once).
- Interprocess `flock` (`concurrency`, children with their own descriptors, synchronised over pipes):
  - `concurrent_attestations_serialize_generation_updates`: 8 processes, each with a different config, reported generations `[2, 3, 4, 5, 6, 7, 8, 9]` and the final state is 9, with no duplicates or lost updates. No negative control without the lock was run, so this shows the lock is sufficient for this scenario, not that it is necessary.
  - `state_cannot_change_between_comparison_and_apply_within_the_held_lock`: while a child sits between compare and apply, a fresh descriptor's `try_lock` fails with `WouldBlock`, and a second helper instance's attestation produced generation `g+1` only after the first finished. The assertion that the second instance had not yet changed the state is not race-free by construction (it may simply not have run yet); the `try_lock` result and the final ordering are the sound evidence.
  - `interrupted_state_write_kill_9_leaves_old_file_and_next_request_advances_once`: a child blocks after `state.tmp` is written, is killed with SIGKILL; the lock is released, the old `state` is byte-identical, `state.tmp` is left over, and the next instance deletes it under the lock and advances exactly once.

**Limits of the process-kill evidence.** A SIGKILL leaves the kernel page cache intact, so it shows that the rename protocol leaves either the old or the new file and that the kernel releases the lock on process death. It does not show that `fsync` of the file or of the directory made anything durable, nor behaviour under power loss, a different filesystem, `ENOSPC` mid-write or a crash between `rename` and the directory `fsync`. The scratch directory was on ext4 in a WSL2 VM, not a conventional host. Crash durability: needs Linux validation.

## 11. Adversarial and expected-safety findings

Demonstrated (`expected_safety`, `signature`):

- `agent_cannot_fabricate_effective_safety_set`: an agent that removes exclusions from the helper's report and re-signs with its own key is refused both as the pinned key (signature invalid) and as itself (key mismatch); altering the helper's relayed bytes under the helper's signature is refused; nothing is stored.
- `compromised_agent_cannot_widen_helper_enforcement_but_can_omit_the_guard` (the documented residual risk, demonstrated, not repaired): a fabricated `verified` identity is refused (`safety_changed`); omitting `expected_safety` or sending `unverified` skips only the identity comparison and a non-overlapping target is applied; a TTL above the helper's maximum is refused even when the agent claims `max_ttl = u64::MAX`.
- Enforcement independent of the request: `helper_refuses_target_overlapping_exclusion_even_if_agent_requests` (targets inside, containing and equal to a CIDR, a hub address, `0.0.0.0/0`), `helper_refuses_overlap_without_expected_safety`, `exclusion_overlap_refused_even_on_unverified_human_path` (all nine reasons).
- Races (J1): `config_change_between_attestation_and_apply_refused`, `network_change_between_attestation_and_apply_refused`, `reboot_between_permit_and_apply_refused`, `permit_from_before_reboot_refused`, `generation_change_between_permit_and_apply_refused` all give `safety_changed`; a comment-only edit does not.
- `helper_fails_closed_when_safety_state_unknown`: unreadable config, invalid config and a failed lookup each give `safety_unknown` with the underlying token.
- `malformed_expected_safety_refused`: both tagged forms parse and round-trip; unknown or missing tag, unknown reason, extra field, missing field, short hash, wrong type and non-object are refused, as is a missing `expected_safety` on the agent path.
- Limits (M1): `agent_limit_stricter_than_helper_applies`, `agent_limit_above_helper_is_capped` (helper 3600 wins), `helper_active_count_is_authoritative` (the agent-reported count is ignored; helper nft count rules; a stricter agent limit applies), `mutation_limit_and_defaults_are_enforced`.

What a compromised agent cannot do here: forge an attestation, make the hub believe a path is covered, or push the helper past its overlap rule, TTL and active maxima, mutation limit or fail-closed behaviour. What it can do: call the helper directly with `expected_safety` omitted or `unverified`. `expected_safety` is a consistency guard for the honest path, not proof of a hub permit; a helper-verifiable permit would be a separate architectural change with its own trust root and is not proposed here.

## 12. Unverified human path findings

Demonstrated (decision functions only; the authenticated surface is not demonstrated):

- Each of the four reasons yields `UnverifiedHuman` with the right reason: `human_containment_with_no_pinned_key_uses_unverified_path` (key_not_pinned), `..._with_expired_attestation_...` (attestation_stale at exactly 300 s), `..._with_invalid_signature_...` (signature_invalid), `..._with_incomplete_routes_...` (report_incomplete). The remaining reasons arise through `path_uncovered` and `confirmation_stale` in `unattended_containment_refused_in_every_unverified_case`.
- `unattended_containment_refused_in_every_unverified_case`: all nine reasons refuse a policy request; a verified gate with an uncovered path or a stale confirmation also refuses; with a clean state the verified path is offered and carries the full identity.
- `agent_refuses_unverified_with_policy_label`: `unverified_requires_human`.
- `human_approved_containment_shows_uncovered_paths`: the card states that coverage cannot be verified, gives the reason token and lists `Not protected: 10.50.0.0/16` (and with no verified data, every discovered path); the decision carries audit event `containment.unverified_approved`.
- `unverified_value_never_carries_an_identity`: every `Unverified` value serialises to exactly two keys (`state`, `reason`).
- Not demonstrated: step-up, typed confirmation prompts and audit persistence (modelled as booleans), and the hub persisting `safety_assurance`.

## 13. Cost measurement (N1)

Method: `tests/cost.rs`, fixture inputs, dev profile (unoptimised, no debuginfo), WSL2 VM on an AMD Ryzen 5 7600X (4 vCPUs), release build not measured. Each row is N timed repetitions; microseconds.

| Operation | N | Median | p90 | Min | Max |
|---|---|---|---|---|---|
| compute config and route lines and hashes (5 rules, 5 routes) | 2000 | 43.8 | 55.2 | 42.7 | 174.6 |
| hub strict parse and hash recomputation of one report | 2000 | 75.5 | 93.9 | 74.0 | 196.8 |
| full attest: lock, read and validate state, compute, sign (no write) | 200 | 515.1 | 573.2 | 487.4 | 821.5 |
| compute with 2000 extra unrelated routes in the fixture | 200 | 476.0 | 697.3 | 443.3 | 2462.2 |

Limits: no netlink, no `fsync` (state unchanged), no process start or socket round trip, no kernel `ip route get`, fixture lookup is a linear scan, single run on one machine. Not a production threshold or dispatch budget.

## 14. Test matrix status

All tests are in `docs/rework/investigations/probes/r10/tests/`. "Pass" means the named test passed in the final run.

| Row | Test(s) | Status |
|---|---|---|
| A1 | `config_hash_matches_vectors` | pass |
| A2 | `equivalent_toml_same_config_hash` | pass |
| A3 | `narrowing_a_cidr_changes_the_hash_and_defaults_apply` | pass |
| A4 | `route_state_hash_matches_vectors`, `every_vector_line_passes_the_strict_line_grammar` | pass |
| A5 | `report_rejects_invalid_encoding_vectors`, `report_parser_rejects_duplicates_unsorted_and_noncanonical` | pass |
| A6 | `parser_is_strict_on_boundaries_the_vectors_do_not_list` | pass |
| A7 | `invalid_utf8_is_rejected_as_its_own_step_before_any_grammar` | pass |
| B1 | `helper_attestation_signature_verifies` | pass |
| B2 | `key_id_never_used_for_verification` | pass |
| B3 | `attestation_rejected_for_wrong_key`, `helper_attestation_rejects_forged_report` | pass |
| B4 | `signature_is_checked_before_parse_and_parser_sees_the_received_bytes` | pass |
| T1 | `signed_incomplete_report_not_accepted_as_verified`, `hub_rejects_unverified_safety_report` | pass |
| C1 | `helper_public_key_exchange_and_fingerprint` | pass |
| C2 | `first_contact_does_not_pin_key`, `key_id_never_used_for_verification`, `key_substitution_by_agent_is_detected` | pass |
| C3 | `helper_state_recovery_requires_explicit_local_action`, `key_rotation_blocks_until_reenrolled`, `key_rotation_invalidates_previous_pin`, `unpin_blocks_containment`, `agent_reenrollment_keeps_helper_pin` | pass |
| D1 | `nonce_is_32_random_bytes_with_expected_properties`, `nonce_lifetime_and_single_use_with_injected_clock` | pass |
| D2 | `helper_attestation_rejects_replayed_nonce`, `attestation_rejected_for_other_host`, `expired_attestation_nonce_is_rejected_and_an_unauthentic_message_does_not_burn_a_nonce` | pass |
| E1 | `effective_configuration_is_normalised_deduplicated_and_defaulted` | pass |
| E2 | `invalid_configuration_yields_no_hash_and_config_invalid` | pass |
| F1 | `multiple_tables_and_rules_observed` (plus the R1 semantic test) | pass |
| F2 | `nondefault_route_change_for_endpoint_changes_generation`, `irrelevant_route_change_keeps_generation`, `route_replacement_and_deletion_detected` | pass |
| F3 | `multiple_interfaces_and_gateways_observed`, `vpn_tunnel_route_to_hub_observed`; live read see O1 | pass (fixtures) |
| F4 | `incomplete_route_data_fails_closed` | pass |
| G1 | `containment_membership_change_keeps_generation`, `boot_only_change_keeps_generation_but_updates_boot_id` | pass |
| G2 | `generation_never_decreases_under_same_key`, `hub_rejects_generation_regression`, `restored_state_can_repeat_a_generation_with_different_content_and_the_hub_accepts_it` | pass (the last documents a gap) |
| H1 | `state_file_has_the_exact_format_checksum_and_permissions` | pass; root ownership needs Linux validation |
| H2 | `generation_state_missing_with_key_fails_closed`, `generation_state_corrupt_with_key_fails_closed`, `generation_state_key_mismatch_fails_closed`, `key_missing_and_uninitialized_cannot_be_signed` | pass |
| H3 | `write_hook_sees_every_stage_in_order`, `leftover_state_tmp_is_deleted_under_the_lock_at_start`, `interrupted_state_write_never_reuses_generation`, `interrupted_state_write_kill_9_leaves_old_file_and_next_request_advances_once` | pass; durability not demonstrated (see 10) |
| I1 | `concurrent_attestations_serialize_generation_updates` | pass |
| I2 | `state_cannot_change_between_comparison_and_apply_within_the_held_lock` | pass (see the caveat in section 10) |
| J1 | `config_change_...`, `network_change_...`, `reboot_between_permit_and_apply_refused`, `generation_change_between_permit_and_apply_refused` | pass |
| J2 | `helper_fails_closed_when_safety_state_unknown` | pass |
| K1 | `malformed_expected_safety_refused`, `agent_refuses_unverified_with_policy_label` | pass |
| K2 | `identity_comparison_covers_all_five_fields` | pass |
| K3 | `helper_refuses_target_overlapping_exclusion_even_if_agent_requests`, `helper_refuses_overlap_without_expected_safety`, `exclusion_overlap_refused_even_on_unverified_human_path`, `compromised_agent_cannot_widen_helper_enforcement_but_can_omit_the_guard` | pass |
| K4 | `agent_cannot_fabricate_effective_safety_set` | pass |
| L1 | the four `human_containment_with_..._uses_unverified_path` tests, `unattended_containment_refused_in_every_unverified_case`, `agent_refuses_unverified_with_policy_label` | pass |
| L2 | `human_approved_containment_shows_uncovered_paths`, `unverified_value_never_carries_an_identity` | pass |
| P1 | `owner_cannot_confirm_uncovered_management_path`, `new_management_path_requires_local_helper_configuration`, `changed_helper_safety_set_invalidates_confirmation` | pass (pure rules); UI, step-up and audit trail **not demonstrated** |
| M1 | `agent_limit_stricter_than_helper_applies`, `agent_limit_above_helper_is_capped`, `helper_active_count_is_authoritative`, `mutation_limit_and_defaults_are_enforced` | pass |
| M2 | `stale_attestation_blocks_unattended_containment`, `confirmation_survives_reboot_only_when_identity_otherwise_equal`, `permit_from_before_reboot_refused` | pass |
| N1 | `cost_of_recomputing_configuration_and_route_state` | pass (measurement, section 13) |
| O1 (optional) | `scripts/live_observe.sh` | observed on WSL2, read-only (below); **not a validation** of the contract |

O1 observation (read-only, uid 1000): IPv4 rules `0 local`, `32766 main`, `32767 default`; IPv6 rules `0 local`, `32766 main`; one interface `eth0` 172.20.120.5/20 with a default route via 172.20.112.1; loopback with an extra `10.255.255.254/32`; no VPN, no policy routing, no IPv6 default route. It shows what a host looks like (including `broadcast` and `local` route types and a rule referencing table `default`), nothing more. It cannot demonstrate policy routing, VPNs or multiple tables, which stay on synthetic fixtures.

RW-37 tests: reproduced under their own names (as simulations of the logic) are every name in the RW-37 list except the two below; where the harness is a simulation (for example the hub, the helper process and the route reader) the test is **adapted**, not an end-to-end test. Names reproduced include `helper_public_key_exchange_and_fingerprint`, `key_id_never_used_for_verification`, `first_contact_does_not_pin_key`, `key_substitution_by_agent_is_detected`, `key_rotation_blocks_until_reenrolled`, `unpin_blocks_containment`, `agent_reenrollment_keeps_helper_pin`, the signature and parsing group, `agent_cannot_fabricate_effective_safety_set`, `hub_rejects_unverified_safety_report`, the confirmation and coverage group, the route-state group, the four race tests, the helper-enforcement group other than the key-readability test, the unverified-human group, the helper-state group, the encoding group, the freshness and limits groups and `human_approved_containment_shows_uncovered_paths`. **Deferred (section 8.1 of the brief, not demonstrated):** `helper_attestation_key_not_readable_by_agent_or_skill` (one process tree, one identity, no real unit separation) and `monitoring_unaffected_by_missing_management_paths` (evaluation and product behaviour).

## 15. Unresolved ambiguities and contradictions

Each is reported, not resolved; the harness's reading is stated, and the owner decision needed follows.

- **G-1 `incomplete_reason` empty value** (section 6). Decision: normative reading of 04b section 1 versus RW-37.
- **G-2 Hash keys when no hash exists.** RW-37 makes `config_hash` and `route_state_hash` 64 hex characters, while 04b says an invalid configuration "yields no hash". Harness: 64 zeros, accepted only in `complete=false` reports and never trusted. Alternatives: empty value (extends G-1), omit the key (breaks "missing key rejected"), or send no signed report. Decision needed.
- **G-3 Singular keys in incomplete reports.** `max_ttl_s` and `max_active` are required exactly once. Harness: `0`/`0` and empty lists when the config is unknown. Decision needed.
- **G-4 `generation` in fail-closed reports.** Harness: `0` (never valid for a complete report; the hub rejects a complete report with generation 0, a harness rule) or the persisted value for config and route failures.
- **G-5 `key_missing` and `uninitialized` cannot be signed.** No key exists, so the contract's `incomplete_reason` cannot be signed. Harness: the helper returns an unsigned error and the hub's nearest reason is `attestation_missing`. How the agent relays this, and the hub-side mapping, are unspecified. Likewise `key_corrupt` (invented token).
- **G-6 `init` hash content** decides whether the first attestation is generation 1 or 2 (section 9).
- **G-7 `rule` selection and the R1/R4 vectors** (section 8): "every policy rule that can select one of those tables" is read as every lookup rule; R1's `local_addr ... wg0` contradicts the `local_addr` rule.
- **G-8 Unrepresentable routing state:** rule actions other than lookup, route types, scope and protocol. Harness refuses non-lookup rules (`rule_unrepresentable`) and ignores route type.
- **G-9 Only the CIDR network address is looked up**, so routing inside a configured CIDR is invisible (section 8).
- **G-10 Selector ambiguity** (section 5). Needs per-item escaping or a different separator; that changes 04b.
- **G-11 Unspecified grammars and behaviours, with the harness reading:** interface names (non-empty, at most 15 bytes; R4's `eth|0 x` is not a kernel-legal name); port 1 to 65535; table, priority and metric as `u32`; `max_ttl` grammar `<digits><s|m|h|d>` (only `24h` and `1440m` are given; the repository has no helper-side parser); unit-type suffix list (service, socket, target, mount, automount, swap, timer, path, slice, scope, device; so `foo.bar` becomes `foo.bar.service`); unit name characters `[A-Za-z0-9:_.@-]` up to 255; container names Docker grammar; unknown keys under `[safety]` refused; missing `[safety]` equals empty; CIDR without `/prefix` refused; `attest.key` is the raw 32-byte seed; family consistency across a line's addresses enforced; rotated or recovered keys are retained under `retired/`; `rotate-key` needs both files, `recover` needs at least one.
- **G-12 Timing and labels:** nonce and freshness boundaries, freshness reference clock, whether the hub cross-checks `helper_key_id` (section 7).
- **G-13 Missing `unverified` reasons** for a validly signed but unparseable report and for nonce failures; harness maps to `report_incomplete` and `attestation_missing`.
- **G-14 No revocation of stored verified state** by an incomplete report, a rejected report, or (without the added mismatch flag) a key change (section 7).
- **G-15 Generation repeats under a rolled-back state are undetectable** (section 9). Decision: accept, or add a rule on the hub or helper.
- **G-16 Apply-time behaviour the contract leaves open:** a requested TTL above the maximum is refused rather than clamped (the test names say "capped"); `max_active = 0` is read as "no containment" (so the default refuses everything), not "unlimited"; the helper's mutation limit has no stated configuration, window or persistence (harness: in memory, one hour); the active count is a plain set size.
- **G-17 Effective set versus report:** 04 section 9.1 lists loopback and link-local in the effective set, but the report has no line for them; the source of `link_peer` knowledge inside the helper is not specified (harness: injected).
- **G-18 Wire drift:** `ExpectedSafety` in the proto crate has a free-string reason, no unknown-field refusal and an optional field on the wire (section 1). The tag key name and the hash encoding inside `expected_safety` are not named in RW-37 (harness: `state`, lowercase hex).
- **G-19 UTF-8 subsumption** (section 5): the contract requires UTF-8 validity as a rule although it follows from the raw-byte rule.
- **G-20 Observation versus contract on the lock scope.** The lock protects helper state, not the kernel; a route change between recompute and apply is the accepted race and is not testable here.

## 16. Items requiring conventional Linux validation

- Live netlink observation on a real host with policy routing, multiple tables, a VPN or tunnel and multiple gateways (`ip route get` semantics including `iif`, `fwmark`, `uidrange`; the fixture lookup is a small emulation); route types, `nft` set accounting and the real active count.
- Behaviour across systemd versions (socket activation, `Accept=yes` per connection, masking of the key in the unit sandboxes).
- A real filesystem's crash behaviour: power loss, `ENOSPC`, rename and directory `fsync` ordering, a different mount (ext4 on WSL2 here; a SIGKILL does not prove durability).
- Root-only visibility: ownership and mode of `/var/lib/domovoi-helper` as root, `SO_PEERCRED`, root-only netlink views; the harness ran as uid 1000.
- Helper key custody across a real process boundary (agent and skill-runner cannot read `attest.key`); the RW-37 test `helper_attestation_key_not_readable_by_agent_or_skill`.
- Interprocess `flock` on other filesystems or network mounts (the lock directory is local in the design; not tested elsewhere).
- Clock and boot id from the real kernel (`/proc/sys/kernel/random/boot_id` was injected).

## 17. Non-acceptance statement

R-10 stays open. This report is evidence and a list of gaps, not owner acceptance, and a green scratch suite is not acceptance. Nothing here is production-ready, no helper, protocol message, endpoint, migration or config key was added, and no design document (`03`, `04`, `04b`, `06`, `99`) was edited. D-28 and D-29 are unchanged. Where the harness made a reading (the zero-hash sentinel, `rule_unrepresentable`, nonce consumption timing, the mismatch flag, the tokens listed in G-5 and G-13) it is a scratch decision for owner review, not a proposed contract.
