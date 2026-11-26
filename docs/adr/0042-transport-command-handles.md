# 0042. Transports may not name the operation or package command handles

- Status: Accepted
- Date: 2026-11-26

## Context

Rule `MOD-4` ([0026](0026-use-case-layer.md)) stops HTTP and chat handlers from holding a command handle: a command is a service method that takes a `&Caller`, so each command has one implementation and the services never mint an approval ([0007](0007-human-only-approvals.md)). The rule named three handles: `ActionHandle`, `OperationHandle` and `PackageHandle`.

`ActionHandle` belonged to the action path, which no longer exists: nothing in the codebase defines or constructs it. A rule that names a type that cannot exist checks nothing, and its fixtures tested the rule against a name no one can write.

## Decision

`MOD-4` forbids a transport from naming `OperationHandle` and `PackageHandle`. `TRANSPORT_FORBIDDEN_NAMES` in `xtask/src/arch/rules.rs` lists exactly those two, and the rule's tests use them. `ARCHITECTURE.md`, ADR 0026 and `AGENTS.md` state the same two names.

The deny-lists in the engine and conversation gate tests that include `ActionHandle` stay. They guard against the executor-shaped names coming back into code that must never approve or run anything, and they cost nothing while the type is absent.

## Consequences

- The module-edge rules describe only types that exist, so a violation message always points at something real.
- A new command handle that transports must not hold is added to the list in the same change that adds the type, with an ADR as for any rule change.
- Nothing is weakened: no transport names any of the three handles today, and `OperationHandle` and `PackageHandle` stay forbidden.

## Alternatives considered

- Keep `ActionHandle` in the list: it documents a type that is gone and misleads a reader into looking for it.
- Delete the engine and conversation gate entries as well: they are absence guards, and removing them lowers the bar for no gain.
