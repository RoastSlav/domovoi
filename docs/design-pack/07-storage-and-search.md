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
3. **Experience log:** append-only incident/phase/plan/operation/verification events and links to relevant entities/evidence.
4. **Documents:** existing Markdown notes/runbooks and their FTS index.
5. **Search structures:** ordinary indexes, FTS5 and (later, if justified) vector embeddings.

## Conceptual schema

Not final DDL; IDs, constraints, normalization and lifecycle fields require a later specification.

```sql
entities(
  id, kind, name, scope_type, scope_id,
  attributes_json, lifecycle, created_at, updated_at
)

relationships(
  id, source_entity_id, relation_type, target_entity_id,
  properties_json, source, verification_status,
  valid_from, valid_until, observed_at, last_verified_at
)

knowledge_facts(
  id, subject_entity_id, predicate, value_json,
  scope_type, scope_id, source_type, source_ref,
  confidence, status, valid_from, valid_until,
  observed_at, last_verified_at, supersedes_id,
  created_at, updated_at
)

experience_events(
  id, incident_id, event_type, phase, actor_type, actor_id,
  occurred_at, summary, payload_json, correlation_id
)

experience_links(
  event_id, entity_id, link_type
)
```

Use typed definitions/validation for entity kinds, relationship types and predicates. Do not create unconstrained EAV where every field is a free-form string. Keep stable/query-critical fields relational; put only variable optional properties in JSON. Store evidence references/hashes and bounded summaries rather than unlimited raw logs or secrets.

## Search strategy

Start with the simplest search that solves real user queries:

- Exact and filtered SQL search for host, type, status, source and dates.
- FTS5 for notes, titles, entity descriptions and experience summaries.
- Bounded recursive CTE traversal for relationships and small relevant subgraphs.
- Structured predicates for current facts, freshness and provenance.
- Later: local embeddings/vector similarity only when evaluation shows lexical/graph retrieval is insufficient.

Do not introduce a vector database just because the system is AI-powered. Do not introduce a dedicated graph database until measured multi-hop query complexity, scale, concurrency or required graph algorithms justify the operational cost.

## Retrieval contracts

Expose domain operations rather than raw database queries, for example:

- `search_knowledge(query, scope, filters)`
- `get_entity(entity_id)`
- `get_facts(entity_id, predicates, freshness)`
- `get_relationships(entity_id, relation_types, direction)`
- `traverse_graph(start_entity, relation_filter, max_depth, max_nodes)`
- `search_experience(query, scope, time_range)`
- `get_incident_experience(incident_id)`
- `get_notes(query, entity/host scope)`
- `propose_knowledge_change(...)`

Every traversal must be bounded by depth, node count, relationship filters and relevance. Context retrieval returns a compact, relevant subgraph; never serialize the whole graph into an LLM prompt. Mutation tools must be authenticated/authorized and schema-validated. The model can propose changes, but the service owns validation and confirmation workflow.

## Consistency and retention

- Hub is canonical for long-lived knowledge, policy and experience.
- Agent caches contain only the signed/versioned subset required for local enforcement or a permitted offline emergency action.
- Track policy/knowledge versions and reconcile cached data on reconnect.
- Keep experience events append-only for auditability, with explicit retention/redaction rules for sensitive payloads.
- Keep current facts mutable through superseding/retraction, not destructive overwrites that erase provenance.
- A deletion workflow must clean derived FTS/vector/cache data and prevent retrieval of the deleted content.

## Migration approach

1. Keep current relational operational tables unchanged initially.
2. Add a new knowledge repository/service alongside `NoteRepo`; do not overload the current `knowledge.rs` module that currently implements notes, baselines, acknowledgements and digest facts.
3. Add typed tables and indexes through append-only migrations.
4. Link existing hosts, skills, notes, incidents and signals to stable entity IDs incrementally.
5. Retain existing notes and `notes_fts`; add entity linkage without forcing Markdown into structured facts.
6. Add graph and semantic search only behind service interfaces.

## Reconsider a dedicated graph store only if

- Production queries require deep/many-hop traversal or graph algorithms that recursive SQL cannot serve efficiently.
- The knowledge graph grows beyond a practical embedded workload or needs independent scaling.
- Multiple services/clients need concurrent graph-native access.
- A graph query language materially simplifies maintenance and the operational trade-off is justified.

Until then, a SQLite-first hybrid model is simpler and sufficient for the target scale.
