# 0023. Capability objects, one checker, bounded evidence

- Status: Accepted
- Date: 2026-11-17

## Context

[0022](0022-single-execution-boundary.md) puts the execution and read boundaries in named places. Three things keep that from being only a convention:

- An engine subsystem that holds the whole agent link can do anything with it, so whether a scheduler may start a skill would be a scanner's judgement, not a type.
- A boundary enforced by several line scanners that each count text drifts: a call split across lines, a renamed import or a macro defeats a line scan, and a file exempted by its name lets production code under a test-like path go unscanned.
- A read that can wait forever lets one hung host pin a caller. The hub's profile collector calls the read boundary under a 20 s deadline.

## Decision

- **Capability objects.** `crates/domovoi-core/src/ports/caps.rs` defines one narrow trait per operation family an engine subsystem needs: `AvailabilityProbe`, `HostObservation`, `HostRead`, `HandoffRunner`, `JournalStatus`, `CheckRerun` and `PackageRelay` ([0029](0029-package-relay-boundary.md)), plus the unit, containment and helper relays of later records. They are bundled per subsystem (`ActionCaps`, `EvidenceCaps`, `PackageCaps`). A subsystem's dependency struct carries those objects instead of the link, so a capability object cannot perform an operation it cannot name. The hub turns its one link into capabilities exactly once, in `crates/domovoi-hub/src/runtime/caps.rs`. The testkit wraps its fake the same way.
- **One checker.** `xtask` is a library plus its binary, and `domovoi-engine` and `domovoi-hub` take it as a dev-dependency (R5). `xtask::arch::ast::exec_violations` is the one implementation of the boundary rules: `cargo xtask arch-check` and both crates' gate tests call it, over the real tree and over fixture trees, and report `file:line`. Each file is parsed with `syn`; a file that parses neither as items nor as a function body is refused. Boundary operations are cards: an exact count per exact site file, default-deny elsewhere. Naming (`AgentLink`, `WsAgentLink`) and the accessors are a per-file allowlist. `use` renames and plain type aliases of `AgentServer` are resolved first. The checker library carries the print and `disallowed_methods` allowances the tool needs; the package keeps `[lints] workspace = true`.
- **Source classification by wiring.** Which files only tests compile is decided by module wiring, never by file name. `xtask::arch::reach::explore` follows `mod`, `#[path]` and `include!` from every Cargo target root in a production and a test context. A file is test-only exactly when the test context reaches it and the production context does not. A `cfg` predicate that cannot be true with `test` false moves its target to the test context; unreadable predicates count as production. A missing module file, a path that leaves the workspace or enters `target/` or `.git/`, a non-literal `#[path]` and a non-literal `include!` are refused as `SRC-1`. Rust loaded from outside `crates/` by production code is scanned like any other.
- **Text rules read through a lexer.** The line rules (`R2`, `R3`, `R4`, `INV-1`) read the source through `xtask::arch::lex`, which blanks comments and, for code rules, the contents of string and character literals while keeping line numbers.
- **Bounded asks.** `collect_evidence` keeps the whole ask inside the request's deadline. A `Clock::now()` already past it answers `Timeout` before anything is asked; otherwise the availability check races the deadline on the `Clock` port. The read is bounded by the `HostRead` contract: every await inside `read_host`, error classification and fallback included, ends by the deadline with `LinkError::Timeout`, and a request it registered is released however the call ends, by a guard that runs on drop. `Evidence` does not wrap the read in a second race, because dropping the read future at an arbitrary point is what the contract avoids. Tests advance a fake clock while a check or a lookup is pending; nothing sleeps.

## Consequences

- A subsystem's reach is its fields. Widening one is an explicit edit in `runtime/caps.rs`, which a reviewer sees.
- There is no second scanner to disagree: `arch-check` and the gate tests share `exec_violations`, so a checker bug fails both sides and the gate fixtures that push bypasses onto a fixture tree keep the implementation honest.
- The checker has limits. A macro that builds an `include!` path from a parameter or `concat!` is refused; a macro defined elsewhere that expands to an include through a name the checker cannot see is not recognised. Glob re-exports of the type or accessor are not resolved. A type alias is resolved only as a plain path. An attribute macro's arguments are not walked, and a `#[cfg(test)]` function (as opposed to a module) is not pruned. Code generated into `OUT_DIR` is not seen. The `xtask/` directory is not scanned. The cards' exact counts and the gate fixtures turn these limits into checked behaviour for the code that exists.
- The edges `domovoi-engine` to `xtask` and `domovoi-hub` to `xtask` are dev-only, so nothing in a product build links `xtask`.

## Alternatives considered

- A link newtype with the operations only on the newtype: the hub still needs the raw link for reconciliation and journal queries, and the scan is what catches a transport that names the type.
- Several line scanners patched against known bypasses: they drift at the next formatting trick, and exempting test-like file names is wrong in principle.
- Sealing the capability bundles so the compiler forbids any other construction: `async_trait` objects are open at their construction sites, so the seal needs an escape hatch; a default-deny naming rule plus the capability fields gets the same result for reviewable code.
- Checking the deadline only after the availability await returned: a hung check would pin the profile scheduler forever.
- Wrapping `read_host` in an outer `select!`: it drops the future at an arbitrary await and skips the link's cleanup.
- Treating every file named like a test as test-only: production code wearing a test-like name is production.
