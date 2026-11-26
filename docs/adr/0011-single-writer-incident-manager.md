# 0011. Incident state has a single writer

- Status: Accepted
- Date: 2026-10-31

## Context

Signals, triage results, acks, button presses, operation results and timers all change incidents concurrently. Races here cause duplicate messages, lost resolutions and double-dispatched work.

## Decision

- The incident manager is one tokio task that owns every incident state transition. Everything else sends it commands over an `mpsc` channel.
- Each command is one `UnitOfWork` commit: the signal, the incident row, its timeline entries and the mirrored journal rows land together or not at all. The manager then publishes a `DomainEvent` on a `broadcast` bus. Consumers (dispatcher, SSE, investigations) react to events and never mutate incidents directly.
- Each incident is read through `VersionedRead`, which returns body and version together. A write names the version it read. A conflict reloads the incident and decides once more; a second conflict fails the command and publishes nothing.
- The manager does no slow I/O besides the store.

## Consequences

- Transition logic is a pure function `(state, command, now) -> (state, events)`, tested exhaustively with the fake clock.
- Throughput is bounded by one task, which is ample for homelab volumes.

## Alternatives considered

- Database row locking per incident: harder to test, and it spreads logic across handlers.
