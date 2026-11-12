# 06 — Knowledge and experience

## Keep three stores conceptually separate

| Store | Answers | Examples | Must not be used as |
|---|---|---|---|
| **Knowledge** | What is true or believed about this environment? | NAS mount location, service owner, app dependency, trusted VPN subnet, user preference, recovery path. | Permission to perform an operation. |
| **Policy** | What is Domovoi allowed to do, under which conditions? | Allow `restart_service` on `plex`; deny all file deletion; require approval for host reboot. | A fact about the environment or a historical observation. |
| **Experience** | What happened, what did Domovoi decide/do, and what was the outcome? | Incident timeline, evidence requests, plan attempts, approval/denial, execution, verification, operator correction. | A guaranteed current fact or an authorization grant. |

They inform one another but cannot be merged semantically. An observed fact may inform a policy proposal; a human must explicitly accept a policy change. An experience may suggest a knowledge update; it does not overwrite facts without provenance-aware validation.

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

Represent meaningful things as entities: host, service, container, database, filesystem, disk, storage pool, network, router, domain, integration, user-defined operational concept. Connect them with typed relationships such as `runs_on`, `depends_on`, `routes_to`, `stores_on`, `backs_up_to`, `exposed_through`, `managed_by` and `managed_from`.

Do not create graph nodes for every log line or metric. Those belong in event/measurement records; link only useful incidents, facts and experiences to relevant entities.

## Fact provenance and lifecycle

A fact/claim should carry, at minimum:

- Subject entity and typed predicate/value.
- Scope (global, host, entity or service).
- Source (user-confirmed, agent-discovered, integration-reported, inferred, imported).
- Confidence/verification status; never treat model confidence as calibrated probability.
- Observed/valid-from time and optional valid-until time.
- Last-verified time and freshness policy.
- Creator/updater and timestamps.
- Status (current, stale, disputed, superseded, retracted).
- Optional link to the fact it supersedes and supporting evidence/experience.

Missing or unreadable data remains `unknown`/`unavailable`; do not write false or empty values as confirmed facts. When sources conflict, preserve provenance and surface the conflict instead of silently selecting the newest text.

Lifecycle: discovered or supplied → validated → used in context → reverified/updated → superseded, expired or retracted. AI inference cannot become user-confirmed merely because it is repeated.

## Onboarding and discovery

Use two layers:

**Hub/environment onboarding:** overall infrastructure shape, host criticality, trusted networks and VPN, remote management paths, services that must never stop, backup locations, notification preferences and initial autonomy choices.

**Host onboarding:** host purpose and criticality, recovery access, storage layout, important services/dependencies, network exposure, backups, maintenance constraints and local restrictions.

Let discovery fill what it can from facts, checks and read capabilities. Ask the owner only about important unknowns or ambiguous discoveries. Permit “unknown / not sure”; do not force guesses. Store answers so the same question is not repeatedly asked. Allow later correction, deletion and scope changes.

## Knowledge proposals

At incident resolution or during a repeated pattern, Domovoi may propose a fact, relationship, preference or constraint to remember. Show the proposed statement, source/evidence, scope and confidence. User confirmation is required for preferences and operational constraints; automatically discovered facts may be saved with their unverified source and can later be corrected. No proposal can alter policy implicitly.

Deletion must remove the record from search indexes, graph edges, caches and retrieval paths. Audit may keep minimal deletion metadata but should not preserve deleted sensitive content.

## Notes remain useful

Keep freeform Markdown notes/runbooks. They are good for procedures and context that do not fit a structured fact. Allow notes to link to entities, incidents and capabilities. Do not replace notes with a brittle universal fact table.

## Management UI

Make knowledge a first-class, editable area:

- Search/browse by host, entity, category, source, status and freshness.
- Edit, confirm, dispute, supersede and delete records.
- View an entity's facts, relationships, notes, incidents, experiences and relevant policy.
- Provide a graph explorer for relationships and an experience explorer for timelines/cycles.
- Show provenance and confidence/status wherever a fact influences a diagnosis or action.

The UI should make it easy to fix incorrect knowledge; users should not need SQL or file edits.
