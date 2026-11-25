# 0035 The helper attestation boundary

- Status: proposed
- Date: 2026-11-25
- Extends [0032](0032-helper-safety-attestation.md) (the attestation contract), [0023](0023-capability-objects-one-checker-bounded-evidence.md) (capability objects over the link) and [0026](0026-use-case-layer.md) (the hub's use-case layer).
- Changes `xtask/src/arch/rules.rs`, which is why it exists.

This record is **proposed**. It decides where the hub's half of the attestation lives and what the machine checks about it. The contract itself (encodings, tokens, the latest-outcome rule, the high-water mark) is [0032](0032-helper-safety-attestation.md).

## Context

The hub must verify a signed safety report against a key the owner pinned, keep the latest outcome per host, and answer one question for the rest of the hub: is this host's helper verified and fresh, and does its effective set cover a path. Several places would like to reach part of that: the HTTP API, the chat surface, the knowledge service that confirms management paths, and later the containment code. The link to the agent can send any frame; the hub must not let a module that decides trust also be a module that can send arbitrary requests.

## Decision

1. **One hub module, `helper_trust`, holds the machinery; one service, `HelperTrust`, is the only way in.** The module holds the nonce store, the strict verifier, the acceptance and outcome commits, pinning, the schedule and the view assembly. It is added to `UNZONED_MODULES` as support code: it is neither a transport nor a service, and it is reached only through `services::HelperTrust` (view, re-check, pin, unpin, fresh), every command of which takes a `&Caller`. There is no handle to it on `ApiDeps`. Chat cannot pin or unpin: the service refuses any surface but the web UI.

2. **The module holds a capability, not the link.** `HelperRelay` (core, `ports/helper_trust.rs`) offers three things: ask for a signed report, read what the live session says about its helper, and say whether the host is connected. The composition root builds it over the link in `runtime/caps.rs`, next to the other capability objects. `attest_safety` joins the counted raw link operations (`RAW_OPS`) with one call site, in that file. The agent-server file that carries the request on the wire (`link_helper.rs`) joins the files allowed to name the link types.

3. **Trust is read through a port.** `HelperTrustRead` (core) returns the trust view the pure gate reads; the hub implements it over the store and the live session, and the engine's `AttestedCoverage` is the real `ManagementCoverage` over it. `HelperTrustRepo` (core) is the store's read side: the pin, the latest outcome with its verified state, and the high-water row. Writes go through the unit of work as `StateWrite::HelperTrust`.

4. **The helper crate gains `ed25519-dalek` and `rand`.** The crate that holds the helper key signs reports and creates keys; both dependencies already exist in the workspace and are added to the crate's external allowlist. Nothing else is added, and there is no new crate and no new crate edge.

## Rules

Each has a test that feeds it a failing input or scans the sources.

| Rule | What it checks |
|---|---|
| `ATTEST-1` | The signature of a safety report is checked in one hub file, once, in a function that takes the pinned key. No other hub file names the signature check or a verifying key. |
| `ATTEST-2` | `HelperTrustWrite::Accept` is built only in `helper_trust/accept.rs`, from a verified report that only the verifier builds. |
| `ATTEST-3` | The hub computes every helper fingerprint from key bytes. The wire types carry no fingerprint and no relay code reads one. |
| `ATTEST-4` | The verifier refuses an empty reason on an incomplete report, a non-empty one on a complete report and any non-minimal incomplete shape. |
| `ATTEST-5` | `helper_high_water` is written only by the acceptance and pin writes of the store's applier (and cleared for a removed host) and read only by the store's read module. |
| `ATTEST-6` | The helper's request decoder refuses a `Contain` request without a valid `expected_safety`. The function and its test exist; the scan over the `Contain` request types arrives with those types. |
| `ATTEST-7` | No per-host mismatch flag or set exists; the gate reads the latest outcome. |
| `ATTEST-8` | The gate reads the nonce issue time and nothing named for the time the hub received or accepted a report. |
| `ATTEST-9` | Rotation and recovery delete the old private key everywhere in the helper's directory tree. |
| `ATTEST-10` | The guard value and its reason refuse extra fields, unknown tags and unknown reasons; the reason is a closed enum. |

## Consequences

- Adding a second place that verifies a report, or a second writer of the high-water mark, fails the build.
- A containment request in a later change calls `HelperTrust::fresh` and the pure decision functions; it never holds the relay.
- The hub's tests drive the whole path through the router with a fake link that signs reports with a published test seed.

## Alternatives considered

- **Give the module the link directly.** Rejected: the same module that decides trust could then send any frame; the capability object is the repository's answer to that.
- **Put the verifier in the engine.** Rejected: the engine does no I/O, and the hub already holds the signature library; a pure verifier would still need the nonce store and the clock.
- **Pin on first contact with a comparison shown on screen.** Rejected in [0032](0032-helper-safety-attestation.md): nothing is pinned without the password and the fingerprint typed from the host.
