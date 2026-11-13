# 06 — Knowledge and experience

## Keep three stores conceptually separate

| Store | Answers | Examples | Must not be used as |
|---|---|---|---|
| **Knowledge** | What is true or believed about this environment? | NAS mount location, service owner, app dependency, trusted VPN subnet, user preference, recovery path. | Permission to perform an operation. |
| **Policy** | What is Domovoi allowed to do, under which conditions? | Allow `restart_service` on `plex`; deny all file deletion; require approval for host reboot. | A fact about the environment or a historical observation. |
| **Experience** | What happened, what did Domovoi decide/do, and what was the outcome? | Incident timeline, evidence requests, plan attempts, approval/denial, execution, verification, operator correction. | A guaranteed current fact or an authorization grant. |

They inform one another but cannot be merged semantically. They stay separate in meaning, not necessarily in storage: **the experience record is the operational journal** (one append-only table of typed events, RW-15), not a third event store, and the knowledge facts reference inventory that already exists rather than copying it. An observed fact may inform a policy proposal; a human must explicitly accept a policy change. An experience may suggest a knowledge update; it does not overwrite facts without provenance-aware validation.

## Knowledge categories

- **Environment facts:** host role, OS, mount points, service manager, storage pools, network interfaces, deployed applications.
- **Infrastructure relationships:** app depends on database; reverse proxy routes to service; container belongs to compose project; backup targets a separate device.
- **Operational constraints:** never interrupt a backup, keep VPN available, do not reboot during a migration.
- **User preferences:** preferred maintenance windows, notification style, known acceptable restarts.
- **Recovery information:** console access, alternate management route, backup/restore path, recovery commands/runbooks.
- **Historical decisions:** update rejected, migration deferred, incident accepted as expected.
- **Learned patterns:** recurring failure after a certain update, a known symptom/cause relationship (marked as inferred until confirmed).
- **Temporary state:** backup running, maintenance in progress, temporary IP block. Store as expiring operational state, not as durable environment truth.

## Entity and relationship model

Represent meaningful things as subjects: host, service, container, database, filesystem, disk, storage pool, network, router, domain, integration, user-defined operational concept. Connect them with typed relationships such as `runs_on`, `depends_on`, `routes_to`, `stores_on`, `backs_up_to`, `exposed_through`, `managed_by` and `managed_from`.

**Owner decision D-02 / [RW-14](../rework/03-architecture-decisions.md).** Subjects are **typed references** (a host, a container on a host, a unit on a host, or a user-defined entity), not a second copy of the inventory. The learned baselines remain the owner of observed containers, units and ports; facts and relations point at them. A small entities table exists only for subjects the user defines (for example "VPN" or "NAS pool"). The set of predicates is a closed, typed list in v1.

Do not create graph nodes for every log line or metric. Those belong in event/measurement records; link only useful incidents, facts and experiences to relevant entities.

## Fact provenance and lifecycle

A fact/claim should carry, at minimum:

- Subject entity and typed predicate/value.
- Scope (global, host, entity or service).
- Source (user-confirmed, agent-discovered, integration-reported, inferred, imported).
- Verification status (for example observed, user-confirmed, inferred). There is **no numeric confidence** field: model confidence is not a calibrated probability, and a number would suggest otherwise.
- Observed/valid-from time and optional valid-until time.
- Last-verified time and freshness policy.
- Creator/updater and timestamps.
- Status (current, stale, disputed, superseded, retracted).
- Optional link to the fact it supersedes and supporting evidence/experience.

Missing or unreadable data remains `unknown`/`unavailable`; do not write false or empty values as confirmed facts. When sources conflict, preserve provenance and surface the conflict instead of silently selecting the newest text.

Lifecycle: discovered or supplied → validated → used in context → reverified/updated → superseded, expired or retracted. AI inference cannot become user-confirmed merely because it is repeated.

## Management paths are safety-critical knowledge (owner decision D-28)

The facts that say how the owner reaches a host (management networks, VPN endpoints, remote management routes, recovery access) decide whether unattended containment may run on it. They must be owner-confirmed and current: unknown, stale or conflicting entries mean unattended containment is refused on that host. A person can still approve work there and monitoring is unaffected. Each path shows its provenance, its freshness and what it blocks, and the root-owned helper keeps the exclusion set (`[safety]` in `helper.toml`) that neither the hub nor the agent can empty. The owner edits `[safety]` on the host as root and presses Re-check; the hub then shows which discovered paths the helper's set covers and which it does not. The helper computes and signs its effective set over a fresh hub nonce and the agent relays it unchanged, so the hub never relies on an agent-reported summary. The owner can confirm only covered paths; an uncovered path shows what to add to `helper.toml` on the host, and the hub never writes that file. A mismatch, an unverified or stale attestation, or any network or exclusion change blocks unattended containment until the owner re-checks and re-confirms. A human approval never makes an uncovered path protected (RW-37, `docs/rework/04 §7` and `§9`).

## Experience: finding past operations (owner decision D-02, RW-25)

Past incidents and how they were solved must be easy to query.

- The journal holds typed events linked to subjects, so an operator can ask "what happened to this container" and get a precise answer, not a text search over prose.
- Every finished operation gets a **resolution summary**: deterministic fields filled by code (subjects, capabilities executed, verification results, outcome) and narrative fields (problem, cause, fix) drafted by the model, marked as model-sourced and editable or confirmable by the owner. Summaries belong to the Operation aggregate and the operation writer is their only writer: the Knowledge UI sends edit commands to it, and edits to code-owned fields are rejected (RW-41). Summaries are full-text indexed.
- The model has a host-scoped `search_experience` read capability. The UI shows **similar past operations** on incident and operation pages.
- When experience is supplied to the reasoner, each item carries its operation id, date, verified flag and source, inside a block labelled historical evidence, not authorization. **Past approvals and past successes are evidence and never satisfy policy.**
- Retention of raw step output and of durable summaries are separate, both configurable (owner decisions D-09, D-21; defaults in `07`).

## Onboarding and discovery

Use two layers:

**Hub/environment onboarding:** overall infrastructure shape, host criticality, trusted networks and VPN, remote management paths, services that must never stop, backup locations, notification preferences and initial autonomy choices.

**Host onboarding:** host purpose and criticality, recovery access, storage layout, important services/dependencies, network exposure, backups, maintenance constraints and local restrictions.

Let discovery fill what it can from facts, checks and read capabilities. Ask the owner only about important unknowns or ambiguous discoveries. Permit “unknown / not sure”; do not force guesses. Store answers so the same question is not repeatedly asked. Allow later correction, deletion and scope changes.

## Knowledge proposals

At incident resolution or during a repeated pattern, Domovoi may propose a fact, relationship, preference or constraint to remember. Show the proposed statement, source/evidence, scope and confidence. User confirmation is required for preferences and operational constraints; automatically discovered facts may be saved with their unverified source and can later be corrected. No proposal can alter policy implicitly.

Deletion must remove the record from search indexes, graph edges, caches and retrieval paths. Audit may keep minimal deletion metadata but should not preserve deleted sensitive content. A deleted or retracted fact leaves the live database, the full-text index and model context at commit (retracted content is purged after 30 days, leaving a tombstone), but it remains in backups until they rotate (up to 7 nightly and the last 3 pre-migrate copies); restoring a backup brings it back, and the UI and book say so (owner decision D-21, `docs/rework/04 §14`).

## Notes remain useful

Keep freeform Markdown notes/runbooks. They are good for procedures and context that do not fit a structured fact. Allow notes to link to entities, incidents and capabilities. Do not replace notes with a brittle universal fact table.

## Management UI

Make knowledge a first-class, editable area:

- Search/browse by host, entity, category, source, status and freshness.
- Edit, confirm, dispute, supersede and delete records.
- View an entity's facts, relationships, notes, incidents, experiences and relevant policy.
- Provide a graph explorer for relationships and an experience view of past operations (timelines, cycles, resolution summaries, similar operations).
- Show provenance and status wherever a fact influences a diagnosis or action.

The UI should make it easy to fix incorrect knowledge; users should not need SQL or file edits.
