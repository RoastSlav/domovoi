# 0011. Incident state has a single writer

- Status: Accepted
- Date: 2026-10-31

## Context

Signals, triage results, acks, button presses, action results and timers all change incidents concurrently. Races here cause duplicate messages, lost resolutions and double-dispatched actions.

## Decision

- The incident manager is one tokio task that owns all incident state transitions. Everything else sends it commands over an `mpsc` channel.
- It applies a command, persists the result, then publishes a `DomainEvent` on a `broadcast` bus. Consumers (dispatcher, SSE, triage queue) react to events and never mutate incidents directly.
- Commands carry the data they need; the manager does no slow I/O besides the store.

## Consequences

- Transition logic is a pure function `(state, command, now) -> (state, events)` tested exhaustively with the fake clock.
- Throughput is bounded by one task, which is ample for homelab volumes (hundreds of signals per minute).

## Alternatives considered

- Database row locking per incident: harder to test, spreads logic across handlers.
