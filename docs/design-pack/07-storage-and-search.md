# 07 — Storage and search

## Recommendation

**Use the existing SQLite store as the initial home for structured knowledge and experience. Model relationships as a graph, but do not introduce a separate graph database yet.**

This is a recommended default, not a permanent technology commitment.

## Why this fits Domovoi

- The current product already uses one embedded SQLite database, WAL mode, migrations and automatic backups.
- The deployment goal is self-hosted and operationally small; a second database creates backup, upgrade, monitoring, security and consistency work.
- Relational tables are appropriate for policies, actions, incidents and audit. Typed relationship tables plus recursive CTEs support useful graph traversals. JSON attributes handle genuinely variable metadata. FTS5 covers lexical search.
- The decision can be hidden behind repository/service ports. If real workload later demands a graph engine, the storage implementation can evolve without exposing a database-specific API to the model. SQLite supports recursive CTEs for graph/tree traversal, JSON functions for flexible attributes, and FTS5 for full-text search ([recursive CTEs](https://www.sqlite.org/lang_with.html), [JSON functions](https://www.sqlite.org/json1.html), [FTS5](https://www.sqlite.org/fts5.html)).

A pure document/NoSQL store is not automatically more flexible: it can make integrity, typed relationships, policy evaluation and transactional action state harder. Keep the stable/query-critical structure relational and use JSON only for genuinely variable properties.

## Logical data areas

Keep these as logically distinct modules even if they share one SQLite file:

1. **Operational state:** hosts, agents, incidents, signals, actions, approvals, policies, settings, audit and orchestration state.
2. **Knowledge graph:** entities, typed relationships and sourced facts/claims.
3. **Experience = the journal:** one append-only table of typed events for incidents, operations, conversations and knowledge, with a global sequence number. It is also the change stream the live UI replays, so there is no separate experience event store (owner decision D-02, RW-15). Beside it sit the hash-chained **audit** record (the security record, written in the same transaction) and the **effects** outbox (pending side effects, not an event store).
4. **Documents:** existing Markdown notes/runbooks and their FTS index.
5. **Search structures:** ordinary indexes, FTS5 and (later, if justified) vector embeddings.

## Conceptual schema

Not final DDL; the authoritative schemas are in [`docs/rework/04 §10`](../rework/04-target-architecture.md) (RW-14, RW-15, RW-25). IDs, constraints, normalization and lifecycle fields require that specification.

```sql
-- Subjects are typed refs: host:<id>, container:<host>/<name>,
-- unit:<host>/<name>, entity:<uuid>. Observed inventory stays in the
-- baselines tables, which own it; facts and relations only point at it.
entities(                       -- user-defined subjects only
  id, kind, name, attributes_json, lifecycle, created_at, updated_at
)

relations(
  id, subject_ref, relation_type, object_ref,
  properties_json, source, status,
  valid_from, valid_until, observed_at, last_verified_at
)

facts(
  id, subject_ref, predicate, value_json,
  source, status, observed_at, valid_until, last_verified_at,
  supersedes_id, evidence_ref, created_at, updated_at
)                               -- no numeric confidence column

journal(                        -- the experience record and the SSE change stream
  seq, occurred_at, actor, kind, topic,
  operation_id, incident_id, subject_refs, payload_json
)

resolution_summaries(           -- one per terminal operation, full-text indexed
  operation_id, subjects, capabilities_executed, verification, outcome,
  problem, cause, fix, narrative_source, confirmed_by, updated_at
)
```

The earlier draft had `experience_events` and `experience_links` tables and a numeric `confidence`. Both are dropped: the journal is the experience record, links to subjects live on the journal entry as typed refs, and a fact carries a status and a source, not a number. The `digest_facts` table, which holds metric samples and collides with the word "facts", is renamed `metric_samples` first.

Use typed definitions/validation for relation types and predicates (a closed enum in v1). Do not create unconstrained EAV where every field is a free-form string. Keep stable/query-critical fields relational; put only variable optional properties in JSON. Store evidence references/hashes and bounded summaries rather than unlimited raw logs or secrets.

## Search strategy

Start with the simplest search that solves real user queries:

- Exact and filtered SQL search for host, type, status, source and dates.
- FTS5 for notes, titles, entity descriptions and experience summaries.
- Bounded recursive CTE traversal for relationships and small relevant subgraphs.
- Structured predicates for current facts, freshness and provenance.
- Later: local embeddings/vector similarity only when evaluation shows lexical/graph retrieval is insufficient. Vector search and a graph database are DEFERRED until a measured need (`docs/rework/06` "Deferred and after-v1").

Do not introduce a vector database just because the system is AI-powered. Do not introduce a dedicated graph database until measured multi-hop query complexity, scale, concurrency or required graph algorithms justify the operational cost.

## Retrieval contracts

Expose domain operations rather than raw database queries, for example:

- `search_knowledge(query, scope, filters)`
- `get_entity(entity_id)`
- `get_facts(entity_id, predicates, freshness)`
- `get_relationships(entity_id, relation_types, direction)`
- `traverse_graph(start_entity, relation_filter, max_depth, max_nodes)`
- `search_experience(query, scope, time_range)` — a host-scoped read capability over resolution summaries and journal entries; the model sees it as a meta-tool, and results carry operation id, date, verified flag and source
- `get_incident_experience(incident_id)`
- `get_notes(query, entity/host scope)`
- `propose_knowledge_change(...)`

Every traversal must be bounded by depth, node count, relationship filters and relevance. Context retrieval returns a compact, relevant subgraph; never serialize the whole graph into an LLM prompt. Mutation tools must be authenticated/authorized and schema-validated. The model can propose changes, but the service owns validation and confirmation workflow.

## Consistency and retention

- Hub is canonical for long-lived knowledge, policy and experience.
- Agent caches contain only the versioned subset required for local enforcement (offline emergency actions are DEFERRED past v1, `docs/rework/06` "Deferred and after-v1").
- Track policy/knowledge versions and reconcile cached data on reconnect.
- Keep journal entries append-only for auditability, with explicit retention/redaction rules for sensitive payloads.
- **All retention is configurable** (owner decisions D-09, D-21, RW-28). Raw data and durable resolution summaries are separate retention classes, and the interaction with full-text indexes, backups and the audit chain is defined in `docs/rework/04 §14`. The defaults are in the table below.
- Audit entries are hash-chained (owner decision D-14); pruning writes a chained checkpoint row, so retention never silently breaks verification. History that predates the chain is anchored once by a digest and verified separately from the chained part.
- Keep current facts mutable through superseding/retraction, not destructive overwrites that erase provenance.
- A deletion workflow must clean derived FTS/vector/cache data and prevent retrieval of the deleted content.

### Retention defaults (owner decision D-21; all configurable)

| Data | Default |
|---|---|
| Signals | 30 days |
| Incident timeline (raw) | 90 days |
| Operation journal (raw) | 180 days |
| Raw execution outputs | 30 days |
| Operations, steps and resolution summaries | retained |
| Audit | retained (checkpoint pruning only if configured) |
| Retracted fact content | purged after 30 days, tombstone kept |
| Conversations | 180 days |
| Notifications and notification state | 30 days |
| Nightly backups | 7 |
| Pre-migrate backups | last 3 |

**What deletion guarantees.** Deleted data is gone from the live database, the full-text index and model context at commit. It remains in backups until they rotate, and restoring a backup brings it back; the retention settings and the book say so.

## Migration approach

1. Keep current relational operational tables unchanged initially. New tables are additive migrations; existing action records are retained and shown through a read-only legacy projection rather than rewritten (RW-32).
2. Add a new knowledge repository/service alongside `NoteRepo`; do not overload the current `knowledge.rs` module that currently implements notes, baselines, acknowledgements and digest facts.
3. Add typed tables and indexes through append-only migrations.
4. Link existing hosts, skills, notes, incidents and signals to typed subject refs incrementally; do not copy baseline inventory into new tables.
5. Retain existing notes and `notes_fts`; add entity linkage without forcing Markdown into structured facts.
6. Add graph and semantic search only behind service interfaces.

## Reconsider a dedicated graph store only if

- Production queries require deep/many-hop traversal or graph algorithms that recursive SQL cannot serve efficiently.
- The knowledge graph grows beyond a practical embedded workload or needs independent scaling.
- Multiple services/clients need concurrent graph-native access.
- A graph query language materially simplifies maintenance and the operational trade-off is justified.

Until then, a SQLite-first hybrid model is simpler and sufficient for the target scale.
