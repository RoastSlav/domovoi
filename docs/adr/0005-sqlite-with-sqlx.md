# 0005. SQLite via sqlx, runtime-checked queries, append-only migrations

- Status: Accepted
- Date: 2026-10-31

## Context

Domovoi is self-hosted on small machines. It needs durable state, full-text search over notes and incidents, and zero operational overhead.

## Decision

- SQLite in WAL mode through `sqlx` with the bundled SQLite (FTS5 enabled).
- Queries are runtime-checked (`sqlx::query_as` with explicit row types), not compile-time macros, so building never needs a database or an offline cache. Every query is exercised by an integration test against a temporary database.
- Migrations are embedded with `sqlx::migrate!` and are append-only once merged (`xtask arch-check --base` enforces this).
- A `VACUUM INTO` backup runs before migrations and nightly.

## Consequences

- No database server to run, back up or upgrade.
- Query typos surface in tests rather than at compile time; the store's test coverage is therefore mandatory.
- Write throughput is bounded by a single writer, which matches the single-writer incident manager (ADR 0011).

## Alternatives considered

- PostgreSQL: an extra service for every user.
- sqlx compile-time macros: need `DATABASE_URL` or a committed `.sqlx` cache that agents forget to refresh.
