# 14 — Page specifications and screen-space planning

This document defines what each target page must contain and do, before visual layout is designed. It is intentionally not a pixel-level wireframe. Every page should be prototyped from these requirements, with the detailed experience as the starting point.

## 1. Priority and space legend

**Priority (category):** P0 safety, approval, audit and current state · P1 primary · P2 technical detail · P3 metadata. The category decides what each presentation profile shows (doc 12, section 5); P0 is shown in every profile.

**Space:** Dominant · Large · Medium · Compact · Contextual.

The space classification is the amount of useful screen area required, not a fixed pixel measurement. It must be validated at realistic viewport sizes and with real data volumes.

## 2. Page inventory at a glance

| Page / surface | Main purpose | Largest element | P0 content |
|---|---|---|---|
| Control Room / Overview | What is happening across the environment? | Active work visualization or attention workspace (Large) | Critical attention, active operations, environment health |
| Operations list | What work is Domovoi doing or has done? | Filterable operation list (Large) | Running, waiting, blocked and failed work |
| Operation detail | Exactly how did / is this work progressing? | Orchestration graph (Dominant) | Graph, current state, approval card when a decision is due, operation controls |
| Incidents list | What problems were detected? | Incident table/list (Dominant) | Severity, state, target, host, age, needed attention |
| Incident detail | What happened and what response is being taken? | Evidence/story with related operations (Large) | Problem statement, evidence, state, linked operations/actions |
| Operator side panel | Ask or request something from anywhere | Conversation and current operation summary (Large within panel) | Context, transcript, composer, active work |
| Operator workspace | Manage complex conversations and requested work | Conversation + operation workspace split (Dominant) | Transcript, operation state/graph, evidence, composer |
| Hosts list | Which machines are managed and in what state? | Host table/cards (Dominant) | Connectivity, role, health, active incidents, agent status |
| Host detail | Understand and manage one host | Overview dashboard (Large) | Health, workloads, active issues, capability/policy state |
| Capability Center | What can Domovoi do and where is it installed? | Catalog / installed inventory (Large) | Discover, install, enable, update, compatibility and trust |
| Capability detail | What does this capability do and what authority does it need? | Manifest/operation contract (Large) | Purpose, provider, effects, schema, targets, verification, permissions |
| Knowledge browser | What does Domovoi know? | Searchable entity/fact inventory (Large) | Fact, source, scope, freshness, confidence, edit controls |
| Knowledge graph | How are entities connected? | Graph canvas (Dominant) | Selected entity, relationships, provenance, filters |
| Policies & Autonomy | What is Domovoi allowed to do? | Policy matrix/editor (Large) | Inheritance, allow/approval/deny, constraints and effective result |
| Policy simulator | What would this policy allow in a real situation? | Scenario/result comparison (Large) | Selected scenario, policy versions, exact deterministic result |
| Integrations | Which external systems are connected? | Integration inventory (Large) | Connection health, scope, actions, reauthentication |
| Attention center | What needs a decision or a look? | Attention list (Large) | Approvals, questions, failures, manual interventions |
| Audit log | Who did what, and is the record intact? | Audit table (Large) | Actor, action, target, outcome, chain verification status |
| System settings | Configure hub-wide behavior | Settings sections/forms (Medium–Large) | Editable settings, source, validation, save/restart impact |

## 3. Control Room / Overview

### Purpose

Answer at a glance: “Is anything wrong?”, “What is Domovoi doing right now?”, and “What needs my attention?” It is a real control room, not merely four statistic tiles and a short list.

### Content plan

| Content | Priority | Space | Behavior |
|---|---:|---:|---|
| Critical attention queue | P0 | Large | Most urgent incidents, blocked operations, failed agents and security warnings; direct links to the right workspace. |
| Active operations overview | P0 | Large | Active operation count and, per operation, only what the engine reports: current phase name, waiting reason and elapsed time. Select one to open it. Do not squeeze a full complex graph into a tiny card. |
| Environment health | P0 | Medium | Hosts online/offline, services with issues, storage pressure and other high-value health indicators. |
| Waiting for you | P0 | Medium | The first items of the attention center: each approval is the shared approval card (compact), others link to their canonical page; age and expiry shown. |
| Recent activity | P1 | Medium | Chronological events across incidents and operations; filter by host/type. |
| Fleet/service summary | P1 | Medium–Large | Host groups, service status, key resource trends. |
| LLM/provider status and usage | P2 | Compact–Medium | Provider availability, usage, latency, tokens/cost and budget pressure. |
| Suggestions and maintenance reminders | P2 | Medium | Knowledge suggestions, capability updates, stale backups, maintenance recommendations. |
| Custom dashboard widgets | P1 | Variable | Add/remove/rearrange/resize; user layouts persist. Defaults remain excellent without customization. |

### Interaction requirements

- Clicking any status/count opens a correctly filtered destination.
- Active operation entries update live from reported state; do not claim a stage is active, and show no percentage, unless the engine reported it.
- Users can pin a favorite host, service or operation.
- Widget configuration is separate from operational permissions.
- The overview must not become a wall of charts. Each widget needs a user question it answers.
- A count appears once within a widget and once in the default layout. User-rearranged layouts may repeat a widget; that is the user's choice and not an acceptance failure.
- The autonomy state (paused, rule suspended) is part of environment health and links to Policies & Autonomy.

### Space decision

Start with a responsive multi-column composition. The active work area and attention queue need enough area to be useful. If a full orchestration graph is too large for the available width, show a meaningful active-operation list/preview and navigate to the dedicated operation page rather than rendering an unreadable mini graph.

## 4. Operations list

### Purpose

A unified index of every operation, independent of whether it came from an incident, a chat request, a schedule, an automation or a manual UI action.

### Primary controls

- Status filters: Running, Waiting, Scheduled, Completed, Failed, Cancelled/Stopped and All.
- Origin filters: Incident, User request, Schedule, Automation, System.
- Host/target, operation type, date, initiator, capability and outcome filters.
- Text search and saved views.
- Bulk actions only for safe management actions such as archive/label; never bulk-approve or execute high-impact changes without an explicit reviewed flow.

### List fields

Priority P0: name/goal, status, origin, target/host, started/updated time, current phase or waiting reason, next required action. Priority P1: initiator, related incident(s), cycle count, progress summary and duration. Priority P2: model usage, provider, capability count and historical outcome details.

### Space and navigation

- The list is Large/Dominant; use a table or dense grouped list at wide widths, responsive row cards at narrow widths.
- Selecting an operation opens a dedicated detail page, not a modal.
- Provide saved filters/views and deep links.
- Running/waiting items should not be hidden below completed history by default.

## 5. Operation detail

### Purpose

The primary workspace for understanding exactly what Domovoi is doing or did. Two representations of one trace are in v1: a **step timeline/tree** (built first) and the **interactive orchestration graph**, which is the primary visualization and receives dominant screen space. Both read the same trace schema, frozen with golden fixtures before either is built; neither invents data the other lacks. A non-canvas **outline** of the graph is always available and is the screen-reader path.

### Space allocation (1280 x 720 reference)

| Region | Content | Size |
|---|---|---|
| Header strip | Goal, status, origin, targets, phase/cycle, duration, budget use, what it waits for, operation controls | Compact-Medium, always above everything else |
| Approval card | When a decision is due: the structured approval card, directly under the header strip | Medium, only while a decision is pending |
| Graph canvas | Orchestration graph (timeline/tree selectable as the alternative view) | Dominant; at least 640 x 400 CSS px; below that, outline plus node sheet |
| Right slot | Node inspector or Operator panel (precedence rule, doc 13) | Contextual, 320-480 px |
| Below the graph | Tabs: Findings, Plan, Conversation, Capability calls, Evidence, Logs/output, Model usage, Policy decisions, Audit/events | P1 and P2 content; one tab visible at a time |

P0 content (header strip, approval card) is never in a tab. Findings and plan are P1 and sit in the first two tabs; on screens too small for graph and tabs together, the graph yields to outline plus sheet before P0 yields to anything.

### P0 / dominant

**Orchestration graph** (Dominant):

- Show actual phases, branch structure, parallel activity, loops/cycles, waits, retries/replans, execution and verification.
- Show active node(s), completed and failed nodes, skipped nodes and nodes waiting for external input/approval.
- Preserve individual event/node identity as new events arrive.
- Support zoom, pan, fit-to-view, minimap/overview for large graphs, cycle filter, branch filter and a live-follow toggle.
- Collapsed node groups must communicate count/status and be expandable into exact activity.
- Selecting a node opens an inspector without navigating away.
- Do not flatten a branched/parallel graph into a fake linear progress bar.
- Keyboard navigation and the outline are specified in doc 15, section 4.

**Current state and next action** (Compact-Medium):

- Operation goal, status, origin, target(s) and the pinned subjects they came from, start time/duration, current phase/cycle, budget use and what it is waiting for.
- A step outside the operation's scope is flagged "outside operation scope" and can only be approved, never run automatically.
- If user input/approval is required, present the exact request and controls at the point of need.

**Structured approval card** (one component, used everywhere an approval appears; canonical home is this page):

- Renders from deterministic data: contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, and the policy decision with its winning rule.
- Model-written text is secondary and labelled as model text.
- One primary action per context; destructive approvals name the target and consequence and ask for confirmation.
- Embedded or linked from incident detail, the Control Room, the Operator and the attention center; they never reimplement it.

### P1 / large or medium

- Findings/hypotheses: facts, inferred hypotheses, uncertainties, evidence links and discarded hypotheses with reasons.
- Current/approved plan: actions, intended effects, preconditions, dependencies, risk, policy outcome, verification and rollback/expiry.
- Related incidents, hosts, services, capabilities and knowledge.
- **Similar past operations:** earlier operations with their resolution summaries (outcome, what was done, verified or not, source of each narrative field). Each is marked "historical, not authorization"; past approvals and successes never count toward the current decision.
- Operation conversation and user messages.

### P2 / tabs or expandable sections

Technical views: Timeline, Capability calls, Evidence, Logs/output, Model usage, Policy decisions, Audit/events. Large raw data lives in tabs and the inspector rather than around the graph.

### Node inspector

Contextual panel; P1 when a node is selected, otherwise closed. It shows:

- Node type/name/status and why it exists.
- Owner component (orchestrator, reasoner, context builder, policy engine, executor, provider, verifier, user).
- Start/end/duration and correlation IDs (P3).
- Inputs and outputs with evidence/provenance references.
- Capability ID/version, target and typed arguments where applicable.
- Result, error, retry/replan relation and next node(s).
- LLM provider/model/token/latency data when relevant.
- Policy matched rules and constraint result when relevant.

The user may inspect any completed or running node; opening an inspector must never change the operation.

## 6. Incidents list and detail

### Incident list

Retain the current ability to filter/group by status/severity and quickly find an incident, but plan the future around links to operations.

P0 fields: severity, title/problem, host/target, current incident status, last firing/opened time, whether action is needed, related active operation. P1: check/source, recurrence count, acknowledged state, last update. P2: detailed signal/evidence count.

A dense table/list should support saved filters, grouping, sort, pagination/virtualization and responsive rendering. Avoid repeated counts/status labels in multiple places.

### Incident detail

The incident describes the problem, not the work engine.

P0: problem statement, severity/state, target/host, current measured facts, key evidence, whether problem persists, related active operation and user attention request.

P1: related operations (one or several), incident timeline, recurrence, similar past operations with resolution summaries (marked historical, not authorization), relevant knowledge and recommended next step. A pending approval for any related operation is shown as the shared approval card, embedded or linked to its operation.

P2: raw signal payloads, complete logs, check metadata and detailed source information.

The page should show the operation graph as a substantial linked preview or embedded workspace only when the operation is central to resolving the incident. Do not duplicate the entire operation page inside incident detail. Selecting “Open operation” leads to the canonical operation workspace.

Keep incident-specific actions such as acknowledge/mute/dismiss clearly separate from operation controls such as stop/cancel/retry. Distinct concepts must not share ambiguous buttons.

## 7. Operator side panel

### Purpose

Fast conversational access from any page, without replacing the dedicated Operator workspace.

P0: current conversation, composer, selected context, request/stop control, active operation status and approvals/questions relevant to this conversation (approvals render the shared approval card).

P1: recent conversations, recent operations, starter prompts, scope selector and “open full workspace”.

P2: model/provider metadata and technical tool details, accessible by expanding the relevant message/operation.

### Behavior

- Opening the panel does not navigate away or discard current page state.
- Starting a request creates/attaches to a durable operation when work is required; pure informational conversation need not create an operation.
- User can start a task, then continue browsing; the operation continues independently.
- Expand into full workspace with same conversation/task IDs and scroll position where sensible.
- Side panel can be resized on desktop; mobile uses full-screen sheet/workspace behavior.
- It must communicate the active scope. “All hosts” is not the same as one host or selected incident. Scope is explicit per conversation.
- Conversations are durable. A stale response for a superseded request is discarded, and the composer draft is kept until the send is committed.
- It shares the right-hand slot with the inspector under the precedence rule in doc 13.

## 8. Operator full workspace

### Purpose

A full environment-assistant workspace for extended investigation, guided configuration and multi-step operations.

P0/Dominant: conversation transcript/composer and operation view. Recommended split: conversation column plus operation/graph workspace, with a user-resizable divider. If there is no active operation, conversation receives the available area and contextual suggestions are shown.

P1: operation list/context selector, evidence panel, selected object context, findings and next requested user input.

P2: conversation metadata, model usage, complete tool call traces and export/share controls.

Tabs or view switch: Conversation, Operation, Evidence/Results. Avoid making the user choose between seeing the conversation and understanding active work; wide layout should support both when a task is active.

## 9. Hosts list and host detail

### Hosts list

P0: name, agent/connectivity state, host role/criticality, health summary, active incidents and update/version warning. P1: resource summary, tags/groups, recently active capability/operation. P2: inventory details.

Support cards and dense table as alternative user views; filters by state, role, tag, agent version, incident state and capability coverage. Adding/enrolling a host remains an obvious primary action.

### Host detail

One canonical page per host. Recommended high-level tabs (exact grouping to validate in prototype):

1. **Overview** — P0 health, host identity, live agent state, active incidents and operations, resource/health highlights, important services, storage and network summary.
2. **Workloads** — services, containers, processes and their dependencies; actions link to capability/operation surfaces.
3. **Capabilities** — installed, enabled, compatible and available capabilities; install/configure/update/uninstall paths.
4. **Monitoring** — checks, schedules, thresholds, alert sources and health history.
5. **Network & security** — interfaces, ports, SSH activity, firewall/security capabilities, trusted management paths and security events.
6. **Knowledge & dependencies** — confirmed facts, assumptions, upstream/downstream relationships, notes and recovery instructions.
7. **Activity** — operations, incidents, capability runs, configuration changes and agent events.

Logs may remain a standalone global view with host filter and also be reachable from Workloads/Activity. Do not create a separate tab solely to expose a large raw log dump unless user workflows justify it.

The host page shows the **host-local unattended ceiling** read-only (the most the host itself allows to run without a person), with the explanation that it is edited in the agent's configuration on the host, not here. Policy can only narrow it.

Host actions (rename, enroll/revoke, maintenance, update agent) should be discoverable in page-level actions, with destructive actions contextual and confirmed.

## 10. Capability Center

### Main page

P0: Discover catalog, Installed inventory, deployment/installation status and capability health. These can be tabs/views under one Capability Center because they are peer views of the capability inventory.

Recommended tabs:

- **Discover** — searchable catalog with categories, trust/provenance, compatibility, requested permissions, version and available targets.
- **Installed** — capabilities installed on agents/providers, status, version, enabled state, last used, updates and policy coverage.
- **Deployments** — install/update/uninstall attempts, target agents, progress, failures and rollback history.

P1: capability groups, filters by host/platform/provider/effect/risk, compatibility warnings, dependency graph and update channel.

### Capability detail page

Dedicated page for non-trivial capabilities; not just a dialog.

P0: purpose, provider/source, trust/signature, version, supported targets, prerequisites, effects/risk, typed inputs/outputs, privileges/permissions, timeouts, verification and rollback/expiry.

P1: compatibility matrix, dependencies, configuration, example invocation, documentation, installed targets and policy mapping.

P2: raw manifest, source files/package provenance, developer diagnostics and version history.

### Install/configure flow

A guided wizard or dedicated deployment workspace:

1. Select capability/package and version/channel.
2. Review provenance, signature, requested privileges/effects, data access and network needs.
3. Select targets (one/many agents or integrations) and rollout strategy.
4. Preflight compatibility, dependencies, resources and existing versions.
5. Show exact planned changes and possible interruption/side effects.
6. Install, verify package integrity and register locally.
7. Configure parameters and enable/disable separately.
8. Set policy permissions separately from installation and enabled state.
9. Run a safe test/health check.
10. Show result and allow rollback/uninstall where supported.

**Critical distinctions:** installed ≠ enabled; enabled ≠ authorized; authorized ≠ skipped local validation. No control installs and enables in one step; each is its own action, and enabling and starting an installation each ask for password re-entry.

**Trust model shown in the UI:** packages are signed archives. The hub only relays the bytes; the **agent** verifies the signature against keys in its own local trusted-key list and installs only if the host has opted in (off by default) and the package is compatible. The UI shows, per host: opted in or not (with where to change it), which key signed the package, and the verification result reported by the agent. A host that has not opted in is shown as "does not accept installs", not as a failure. v1 scope (proposed): first-party signed packages plus packages signed with a key the owner added locally; no third-party registry.

**Progress and recovery:** the deployment view shows one row per host (queued, delivered, verifying, installed, failed with the agent's reason, pending because offline), the overall result as "N of M hosts", retry for failed hosts only, and uninstall/rollback where the package declares it.

AI-generated capability drafts may be reviewed and tested, but the model cannot self-install, grant itself new privileges or alter its own trust roots.

## 11. Knowledge browser, entity detail and graph explorer

### Knowledge browser

P0: searchable list of entities/facts, source/provenance, host/scope, verification/freshness, confidence/status and direct edit/correct/delete controls.

P1: filters by category, entity type, source, scope, freshness, inferred/confirmed/disputed state and related incident/operation.

P2: bulk review, merge suggestions and data lifecycle tools.

The UI must clearly distinguish confirmed environment fact, automatically discovered fact, inferred hypothesis, user preference, operational constraint and temporary observation.

### Entity detail

P0: entity identity and type, current known facts, source, freshness, verification and relationships. P1: linked notes, incidents, operations, capabilities, policy references and history. P2: raw metadata/attributes and edit history.

### Knowledge graph explorer

Dominant canvas plus contextual inspector. Support bounded traversal, entity-type and relationship filters, search, layout controls, focus mode and selection. Never render every log line/metric as graph nodes. The graph is for meaningful entities and relationships.

### Notes/runbooks

Keep Markdown notes as a distinct document type. Add entity/host references, tags, full-text search, editing, revision awareness and links from incidents/operations/capabilities. Do not force unstructured runbook text into fact records.

### Suggested knowledge

Present proposed learnings with evidence and suggested scope. User can accept, edit, reject or defer. Accepting a fact must never grant operational permission.

## 12. Policies & Autonomy

### Policy overview/editor

P0: effective policy by host and capability/operation, clear Allow automatically / Ask for approval / Deny semantics, inherited versus explicit values, rule precedence and a "why" explanation (below); the autonomy pause switch and any rule suspended by its circuit breaker; rules in shadow mode (new rules start there) clearly marked as not yet acting.

P1: conditions, target restrictions, time windows, TTL, rate limits, blast-radius caps, trusted paths, recovery requirements and policy versioning. Publishing a policy version asks for password re-entry.

P2: raw rule representation, import/export and policy-as-code support later.

A matrix view is recommended for host-by-capability review, but complex condition editing needs a dedicated policy detail/editor rather than an unreadable mega-table.

### Effective-policy explanation

For any host, capability and context the page answers "why this result?" using the production evaluator, never a re-implementation. It shows:

- **All matching rules**, each with its effect (Allow, Ask, Deny) and specificity.
- **The winning rule** and **why it won**, as one of: strictly more specific than the others; equal specificity where Deny wins; incomparable specificity resolved by the fallback.
- **Which broader rules it overrides.**
- **Hard limits shown separately from rules**: the host-local ceiling (read-only, edited on the host), the global pause, rate limits and target binding. A hard limit is never presented as a rule that could be edited here.
- **Dangerous exceptions made obvious**: a specific Allow that overrides a broad Deny is marked with a warning tone, an icon and the text "Allow overrides a broader Deny", in the matrix, the explanation and the editor's diff before publishing.
- Missing or unavailable context is named; the result may be "cannot determine".

### What-if policy simulator

Dedicated page/panel with:

- Scenario selector: historical incident/operation, saved scenario or manually defined context.
- Policy selector: original policy at the time, current policy, draft/proposed policy.
- Reconstructed context: target, evidence, available capabilities, policy version, relevant host/operation metadata and missing data.
- Deterministic evaluation output for each proposed operation: allow automatically, require approval, deny, unavailable or cannot determine, with the full effective-policy explanation above.
- Side-by-side differences and consequence summary.
- Save/apply draft policy after review, not from the simulator result alone.

The simulator calls the production evaluator through a side-effect-free path. It is read-only and must never execute a capability. A historical scenario needs the stored context snapshot and policy version recorded when the decision was made (a backend requirement, doc 17 section 11); missing historical facts are labelled, and the page never silently simulates with today's environment and claims it was historically exact.

## 13. Integrations

P0: connected state, provider type, configured scope, last successful access, errors and primary actions (configure, test, reconnect, disconnect).

P1: catalog/add integration flow, permissions/scopes, credentials set/replace without exposing stored secret values, capability mapping and health history.

P2: detailed provider logs, rate limits, API quotas and advanced auth settings.

Integrations are providers, not a second execution engine. Their capabilities feed the common capability registry and operations system.

## 14. System and settings

Settings should be grouped by the user's goal, not by implementation crate:

- General hub behavior and URL/listener/network configuration.
- Models and AI providers (primary/fallback, context limits, budgets, cost controls).
- Notifications and channels.
- Agent enrollment/trust and update policy.
- Data, retention, backups and restore. Every retention value is editable (raw data and durable summaries are separate classes); the page states what each change deletes and when, and changing retention asks for password re-entry.
- Account/security/session management.
- UI preferences (theme, density, presentation profile, dashboard layouts, shortcuts).
- Updates, diagnostics and system health.

P0: UI-editable fields with validation, current value/source, defaults and save feedback. P1: advanced controls and safe restart/roll-back flows. P2: raw config export and advanced diagnostics.

A value should be read-only in the UI only for a documented safety/trust reason or because config-as-code mode explicitly owns that value. When possible, expose safe UI lifecycle operations even for protected secrets/certificates (set, replace, test, rotate, revoke) without displaying the private material.

## 15. Logs, audit and history

### Logs

P0: source/host, severity, time range, search, live/pause, clear stale indicator, and safe presentation of redacted output. P1: operation/incident correlation and structured field filters. P2: raw structured event export and advanced queries.

### Audit log

P0: who/what/when/target/outcome, exact policy/action transition, and the **hash-chain verification status** (last verified time, result, head hash, and any checkpoint left by pruning). A failed verification is a page-level danger banner naming the first bad entry. P1: filters by actor, host, operation, capability, policy and date; links back to the original object. P2: export, retention and a "verify now" control. The page states what the chain detects (accidental corruption, edits that do not recompute it, deletion mid-chain) and what it does not (an attacker who rewrites the whole database).

The audit view is not the same as the user-facing operation timeline. Operation timeline explains work; audit captures security-sensitive changes and authority decisions. They should link to each other.

## 16. Notifications and attention center

A dedicated attention center is the single list of pending approvals, questions, manual interventions, warnings and important completion/failure notices. The Control Room "Waiting for you" widget and the Approvals page are views over this same list, not separate queries with separate rules. An approval item renders the shared approval card or links to its operation; every other item links to its canonical object. Users can configure channels, priority, grouping, quiet hours and preferences. A toast is transient feedback, not the only durable record of something requiring action.

## 17. Page design deliverable template

Every implementation-ready page specification must include:

1. Page purpose and user questions.
2. Entry points, exit points, canonical route and related objects.
3. Page sections/controls with P0–P3 priority and space class.
4. Recommended containment: page, subpage, tabs, section, expansion, inspector, dialog or popover—with rationale.
5. Interaction matrix: action, trigger, result, confirmation, state change and navigation.
6. Live update contract and what may update without user action.
7. The states from the canonical taxonomy (doc 15, section 10) that apply. Permission-denied states are not part of v1 (single owner); name the hook for future roles instead.
8. Responsive behavior and presentation-profile behavior: the P0-P3 category of every element, and what Standard and Essential collapse or hide.
9. Keyboard, focus, screen-reader and reduced-motion behavior.
10. Animation/motion references from the global motion specification.
11. Data needed, freshness and source of truth.
12. Performance constraints and acceptance criteria.
13. Open questions and explicit decisions.
