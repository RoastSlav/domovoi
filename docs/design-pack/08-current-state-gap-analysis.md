# 08 — Current-state gap analysis

> **Snapshot only:** verified code baseline `393694b` (UI overhaul merged). Existing architecture docs and ADRs may be outdated; they are navigation clues, not binding design authority.
>
> **Corrections (2026-11-13):** the repository investigation in [`docs/rework/01-repository-reality.md`](../rework/01-repository-reality.md) found four statements below to be wrong or imprecise. They are corrected in place: the agent link is not mutual TLS; the plan hash covers the manifest, not the script bytes, and the approve API carries no hash; verification and the triage queue are in memory and not restart-safe; and `HumanApproval` is restricted by a line-based scan, not sealed by the compiler. The rest of this table is unchanged and still a point-in-time inspection.

## Current state vs target

## Baseline inspected

- Repository: [`RoastSlav/domovoi`](https://github.com/RoastSlav/domovoi)
- Branch: `main`
- Commit: [`393694b0d16ad722961748744226e80b8cbac518`](https://github.com/RoastSlav/domovoi/commit/393694b0d16ad722961748744226e80b8cbac518)
- Commit time: 2026-11-12 17:25:41 UTC
- Project status in README: v0.1, before first release.

This is a point-in-time inspection. Re-check the repository before using this document for implementation.

## Executive comparison

Domovoi already has a strong safety-oriented foundation: hub/agent separation, typed read tools, local skill manifests and revalidation, deterministic action lifecycle, incident/action single-writer patterns, SQLite, notes search and host profiles. The target may reuse, refactor or replace these foundations. The current design documents are not assumed to accurately describe all current behavior.

The largest missing pieces are: a general capability contract beyond skills/read-tool enums; durable multi-phase orchestration with dynamic evidence requests; owner-defined autonomous authorization; structured, provenance-aware environment knowledge and relationships; a first-class experience model across cycles; and UI/workflows for managing these systems.

## Feature-by-feature map

| Area | Current `main` | Target | Gap |
|---|---|---|---|
| Product role | Monitoring, LLM triage, chat notifications and human-approved skill execution. | Adaptive operations agent with bounded autonomy. | **Major product expansion**, while preserving the current monitoring loop. |
| Deployment | One Rust binary with hub and agent roles; agent dials out over a TLS WebSocket. The agent pins the hub's certificate; the agent's identity is an ed25519 challenge bound to the TLS channel (this is **not** mutual TLS). | Same basic hub/agent split, with external capability providers as needed. | **Mostly reuse**; extend provider/protocol discovery. |
| Process safety | `domovoi-runner` is the only process spawner; argv-only, no shell; skills have typed manifests, limits and redaction. | Same invariant applies to legacy skills; general operations use typed capability providers. | **Preserve and generalize**, do not weaken. |
| Skill system | Local `skill.toml`; read/write/destructive risk; typed args; hosts/applies_to; timeout/output; optional verify/revert; hub per-host enabled state; agent local `allow_risk` and manifest/hash/arg revalidation. | Skills become one capability provider among native tools and integrations. | **Partial**: strong script execution boundary, but not a cross-provider capability catalogue. |
| Read tools | Closed `ReadTool` / `ReadToolCall` enum: Docker, host overview, journal, listening ports, SMART, incident history, notes search and read skills. | Discoverable typed observation and operation capabilities. | **Partial**: extensible only through code/schema changes; no general provider registry. |
| Discovery | Agent handshake shares host facts, check inventory and skill summaries/catalogue metadata. | Agent/provider announces typed operations, prerequisites, effects, verification, scope and version. | **Partial**: inventory exists, but capability semantics are incomplete. |
| Checks and signals | Built-in host/container/system/network/security checks; webhook and probe ingestion; incident correlation and deduplication. | Same detection layer feeds phase-based orchestration and richer context. | **Reuse**; likely add domain-specific checks and context sources incrementally. |
| Incident lifecycle | Durable incident state machine, signal history/timeline, confirmation, ack/mute/resolve/reopen, incident manager single writer. Incident, signal and timeline writes are separate statements, not one transaction. | Incident becomes durable work item with resumable phase/cycle state. | **Partial**: durable incident/timeline exists; orchestration phase state does not. |
| Triage | Bounded model conversation with read-only tools, token/tool/time budgets, prebuilt context, fallbacks and report schema. The triage queue and its deferred retries live in memory and are **not restart-safe**. | Dynamic request/response investigation, structured hypotheses/uncertainty, phase outputs and loopable plans. | **Partial**: bounded tool-use foundation; context and conversation are still one triage-oriented workflow. |
| Context collection | Current context assembles incident snapshot, signals, recent changes, similar incidents, host profile note, notes and skill catalogue. | Layered, domain-specific context builder with explicit missing/unknown/unavailable/failed/stale statuses and on-demand evidence requests. | **Partial**: context assembly exists; generalized evidence model and dynamic requests are missing. |
| Action lifecycle | Proposed → human approved → dispatched → success/fail → verify; plan hash; manifest revalidation; audit; loop guard; action writer single task. No automatic retry. The plan hash covers the skill manifest but **not the entry script's bytes**, and the approve API **carries no hash** (the plan hash is checked on the hub and the agent, but the approving human does not submit the one they saw). The audit record is appended after the state change as a separate statement whose failure is only logged. Post-action **verification timers are in memory** and are lost on restart. | Per-step authorization, policy permits, idempotency, resumable plans, re-plan after failure, postcondition and monitor windows. | **Partial**: robust approval/execution base; autonomy and multi-cycle plans are absent. |
| Autonomy | Human-only approvals; destructive typed confirmation; local risk allowlist is not an autonomous owner policy. | Granular per-host/category/capability policy; L2 and L3; offline emergency authorization. | **Missing**. Must add a separate owner-policy authorization path; the current human-only mechanism is a migration starting point, not the target. |
| Authorization invariants | `HumanApproval` construction is restricted to auth code by a **line-based text scan in `xtask arch-check`**, not by compile-time sealing (the type and its constructors are public in `domovoi-core`, so a renamed import or macro would evade the scan); `ReadTool` has no mutation/approval tool; agent revalidates every skill request. | Separate human approval and owner-granted autonomous permit types; deterministic policy evaluator. | **Preserve then extend via a deliberate authorization redesign**. Do not treat a `System` principal or allow-risk list as sufficient autonomy. |
| Knowledge — notes | Global/host Markdown notes; CRUD; tags; SQLite FTS5 search; triage can retrieve relevant notes. | Keep notes, link them to typed entities and structured knowledge. | **Present** for documents; add entity linkage and a structured store. |
| Knowledge — profiles | Hub collects host facts, overview, containers, listening ports, kernel messages and selected read-skill results; stores a generated host profile as a tagged note. | Editable, provenance-aware, structured facts with freshness/conflict lifecycle. | **Partial**: useful discovery/profile foundation, but stored as generated Markdown rather than a fact graph. |
| Knowledge — facts/graph | Host facts, baselines (containers/units/ports), digest facts, ack rules and maintenance windows exist as separate concepts/tables. No general entity/relationship/fact model found. | Typed entity graph and claim/fact records with source, confidence, validity and supersession. | **Missing as a general model**; reuse IDs and host/incident relationships where possible. |
| Experience | Incident events/timeline, actions, append-only audit, triage cache, prior incident search and follow-up context. | Explicit experience across phase transitions, hypotheses, evidence requests, plans, policies, actions, verification and learning. | **Partial**: core operational history exists; richer cross-cycle experience model and explorer are missing. |
| Persistence | SQLite + SQLx, WAL, append-only migrations, backups, FTS5 for notes/incidents. | Relational operational core + graph-shaped typed tables + experience events + lexical search; vector search optional later. | **Good fit already**; no reason to add a separate DB now. |
| UI | Hosts/profile, incidents/timelines, approvals, skills, notes, settings and service status. | Knowledge browser/entity detail/graph, experience explorer, autonomy policy editor and explainable policy outcomes. | **Partial / missing new views**. |
| Suggestions and learning | A design proposal in the existing docs describes deterministic suggestions from history (new skill, ack rule, check/threshold); verify whether any of it was implemented before relying on it. Suggestions should never silently change permissions. | Suggest knowledge improvements and possible policy changes without applying authority. | **Related foundation in design only**; preserve human review. |
| Workflows/playbooks | No general incident playbook runtime described as the core execution model. | Optional guidance that the reasoner can use or depart from. | **Not a blocker**; add later if recurring patterns demonstrate value. |

## Existing code anchors

These are starting points in the verified source tree, not prescribed future module boundaries. Future design work should inspect their current contents, tests and call sites before deciding what to retain:

| Concern | Current location |
|---|---|
| Enforced boundaries and security invariants | [`ARCHITECTURE.md`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/ARCHITECTURE.md) |
| Skill manifest and usage | [`docs/book/src/guide/skills.md`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/docs/book/src/guide/skills.md) |
| Action and approval lifecycle | [`docs/book/src/concepts/actions.md`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/docs/book/src/concepts/actions.md) |
| Closed set of model read tools | [`crates/domovoi-core/src/triage/tool_schema.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-core/src/triage/tool_schema.rs) and [`tools.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-engine/src/triage/tools.rs) |
| Triage context assembly | [`crates/domovoi-engine/src/triage/context.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-engine/src/triage/context.rs) |
| Agent-side skill revalidation | [`crates/domovoi-agent/src/requests/skills.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-agent/src/requests/skills.rs) |
| Host profile collection/UI API | [`crates/domovoi-hub/src/profile/collect.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-hub/src/profile/collect.rs), [`api/profile.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-hub/src/api/profile.rs) |
| Notes CRUD and search | [`crates/domovoi-hub/src/api/notes.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-hub/src/api/notes.rs), [`crates/domovoi-store/src/fts.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-store/src/fts.rs) |
| Database schema | [`crates/domovoi-store/migrations/0001_init.sql`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-store/migrations/0001_init.sql) |

## UI baseline — verified after the 2026-11-12 overhaul

The latest main commit merges PR [#111](https://github.com/RoastSlav/domovoi/pull/111), `feat(web): redesign the UI around what the operator needs to see and decide`, at `393694b0d16ad722961748744226e80b8cbac518`.

### What the overhaul currently provides

- A rewritten `web/DESIGN.md` and stronger semantic token system. The warm charcoal/sand dark theme, paper/ink light theme, moss accent and walnut brand mark are deliberate. Semantic tones cover danger, warning, success, pending, progress, info and neutral.
- A new app shell with grouped collapsible sidebar, top bar/breadcrumbs, host selector, live connection state, user/theme menu and Ctrl/Cmd+K command palette.
- Command palette results for pages, hosts, open incidents, settings sections and basic actions (theme, sidebar, logout). **This is a useful command palette foundation, not yet the complete cross-domain search** for operations, services, capabilities, knowledge and policies described by the target.
- A new Overview page with summary tiles, high-priority incidents, approvals waiting and hosts with problems.
- A denser incident list and detail page: severity stripes, responsive table/cards, triage summary, evidence, action groups, tools used, timeline and incident chat.
- A more usable Approvals page with compact waiting rows when the list is busy.
- A more information-rich Hosts page: cards/table, filters, health/fact overview, storage, containers, network/profile and SSH login details.
- Better settings rows, source labels, unsaved-change bar and safer field behavior.
- Shared UI components, design demos, tests for layout and component behavior, contrast/accessibility checks and a documented bundle budget.
- Browser checks reported by PR #111 at 375, 768 and 1440 px in both themes, with a busy sample fleet; PR validation reports 735 tests, axe checks, lint/typecheck/format/build and cargo fmt/clippy for the dev hub.

### Important current UI behavior and limits

| Area | Current implementation | Target direction |
|---|---|---|
| Navigation | Overview, Incidents, Approvals, Chat; Hosts, Skills, Logs; Known conditions, Notes, Digest; Settings. | Reorganize around Operations, Infrastructure, Capabilities, Knowledge, Policies and Integrations. Existing groupings are not binding. |
| Overview | Counts, attention list, approval list, problem hosts. | Configurable Control Room with active operation visibility, richer environment status, current work and attention center. |
| Incident detail | Human-readable triage result, evidence, proposed/running/earlier actions, tool-use summary, timeline and incident chat. | Keep incident as detected problem; link to one or more full Operations. Add graph/trace context without duplicating the entire operation workspace. |
| General Chat | Host-scoped read-only Q&A. Conversation is stored in `sessionStorage`, limited recent history sent to the hub; no mutating action interface. | Operator side panel + full workspace, durable conversations, task/operation creation and shared orchestration/policy/capability path. |
| Operations | No unified first-class Operations list/detail currently found in the web routes. Current `actions` are skill action records, not the full operation concept. | First-class Operations index and detail with dominant orchestration graph, history, plan, evidence, calls, policy, execution and verification. |
| Orchestration visualization | No live phase/branch/cycle graph currently found. Incident page has a timeline and tools-used summary. | Interactive graph with phases, branches, cycles, waits, parallel calls, node inspector and historical playback/list alternative. |
| Hosts | Current host detail tabs are Overview, Profile, Checks, Logs and SSH logins. | Redesign from scratch around Overview, Workloads, Capabilities, Monitoring, Network/Security, Knowledge/Dependencies and Activity (subject to prototypes). |
| Skills/capabilities | Per-host skill list; enable/disable; details open in a dialog; scripts/manifest are installed on agent. | Capability Center catalog, installed inventory, deployments, capability detail and guided installation to agents from the UI. Skills become one provider. |
| Knowledge | Markdown Notes, FTS search, host profiles, learned baselines, known-condition rules. | Structured entities/facts/relationships with provenance/freshness/confidence, graph explorer, editable experience and knowledge suggestions. |
| Policies/autonomy | Current skill risk/local allow-risk and human approval path; no complete granular owner policy UI or historical what-if simulator identified. | Granular policy editor/matrix and read-only historical/current/proposed policy simulation using the same evaluator as live execution. |
| Search | Ctrl/Cmd+K command palette covers a useful but limited set. Page-specific searches exist for incidents, hosts and notes. | Permission-filtered global search across all important entities and command actions, grouped by object type. |
| UI preferences | Light/dark/system theme, sidebar rail, cards/table view on Hosts and some saved URL filters. | Detailed/Standard/Essential/Custom presentation profiles, per-page density, customizable dashboards and graph preferences. |

### Current visual/design anchors to reuse selectively

- `web/DESIGN.md` is the current design rulebook; its principles include crisp/quiet/dense/precise presentation, a 4 px grid, semantic tone/color, contextual help, truthful loading/error/offline states, same-origin assets and short motion.
- `web/src/styles/tokens.css` contains palette and semantic tokens, spacing, radii, type, control sizes, layout sizes, layers and motion tokens.
- `web/src/design/tone.ts`, vocabulary files and `icons.tsx` define consistent statuses, terminology and icon names.
- `web/src/components/` contains reusable controls, dialogs, menus, tables, tabs, timelines, feedback, loading/error and chat building blocks.
- `web/src/dev/DesignPage.tsx` and the `/dev/design` page provide an interactive component/design-system catalogue.
- `web/src/test/` and feature tests cover accessibility, contrast, UI behavior and performance/guardrail contracts.

These are potential foundations, not a command to reuse existing page layout. A new orchestration canvas, policy simulator, capability deployment flow or Operator workspace should get components designed for its actual needs.

### UI gap summary

The current UI is now a credible v0.1 operator interface, but it is still centered on the existing incident/approval/host/skill model. The target UI is broader: it must manage durable operations, show the orchestration engine working, provide a full conversational Operator, install/manage capabilities, edit structured knowledge, configure granular autonomy and simulate policy outcomes. The main missing work is not cosmetic polish; it is new product surfaces and a new information architecture.

## Migration implications

- Do not replace working components wholesale without understanding their invariants and migration implications; the target may still justify substantial redesign where the current structure limits the product.
- Wrap existing read tools and skills as initial capability providers where useful; decide which manifest/protocol contracts should be generalized, versioned or replaced.
- Reuse incident timeline and audit data where it fits, while designing a complete orchestration experience/event model rather than forcing all history into the current timeline schema.
- Add structured knowledge and experience models; migrate or link notes, profiles, baselines and incident data where their semantics fit.
- Introduce autonomy only after its policy and authorization model, migration, UI, agent-side enforcement tests and failure drills are designed and validated together.
- Preserve a tested compatibility path during migration where useful. New autonomous permissions should default off until explicitly configured and validated.
