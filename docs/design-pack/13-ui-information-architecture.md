# 13 — UI information architecture and navigation

**Status:** target proposal based on agreed product concepts. Page names and grouping are recommended defaults; page responsibilities are more important than the final labels.

## 1. Top-level model

The UI should organize around the user's work, not mirror internal Rust crates or service boundaries.

Recommended top-level navigation:

| Area | Main purpose | Primary entries |
|---|---|---|
| **Control Room** | Understand the environment and current attention needs. | Overview |
| **Operations** | See and manage all work Domovoi performs. | Operations, operation detail, operation history |
| **Incidents** | Understand problems detected in the environment. | Incident list, incident detail |
| **Infrastructure** | Understand and manage hosts and the things running on them. | Hosts, host detail, service/workload detail, topology |
| **Capabilities** | Discover, install, configure and manage what Domovoi can do. | Catalog, installed capabilities, deployment history, capability detail |
| **Knowledge** | Inspect and maintain what Domovoi knows about the environment. | Knowledge browser, graph explorer, notes, suggestions |
| **Policies & Autonomy** | Define authority and understand what Domovoi is allowed to do. | Policies, per-host policy, simulator, decision history |
| **Integrations** | Connect external systems and manage their providers. | Connected integrations, add integration, integration detail |
| **System** | Manage the hub and account-wide behavior. | Settings, models, notifications, audit, backups, updates, diagnostics |

Do not force the final sidebar to contain every possible subpage. Important detail views can be reached through the parent page, global search and contextual links. The hierarchy should be reviewed after full page specifications exist.

## 2. Global application shell

### Persistent shell elements

| Element | Priority | Space | Required behavior |
|---|---:|---:|---|
| Main navigation | P0 | Medium / collapsible | Clear grouping; active route; counts for attention states; keyboard and narrow-screen support. |
| Environment/host context | P0 | Compact | Makes it obvious whether the view covers all hosts or a specific host/environment. Contextual object pages can set it deliberately. |
| Live connection/system state | P1 | Compact | Truthful connection state with explanation and retry/status detail. |
| Global search / command palette | P1 | Contextual | Search destinations and objects; command actions are clearly separated from search results. |
| Operator launcher | P0 | Compact | Available from anywhere; opens the persistent side panel without losing current page state. |
| Notifications/attention center | P1 | Compact | Unread/urgent items, approvals, questions and completed operations; never rely only on toasts. |
| Account/user menu | P2 | Compact | Profile, preferences, theme and sign out. |

The shell may evolve from the current sidebar/topbar layout. Do not assume all actions must live in the top bar or that the current sidebar groups must remain.

### Global keyboard model

Recommended defaults:

- `Ctrl/Cmd+K`: global search and command palette (the current overhaul already uses this shortcut).
- `Ctrl/Cmd+J`: open/focus the Operator side panel.
- `Escape`: close the topmost overlay/panel; never cancel a running operation without a separate confirmation.
- `/` may focus the current page's search field only when focus is not already inside an editable control.
- Shortcuts must be discoverable and user-configurable where conflict is possible.

### Global context behavior

- A host/environment selector should be consistent across pages that support scoping.
- Opening a host-specific detail view may set the current context to that host; navigating away should follow a documented, predictable rule.
- Search results and deep links must preserve scope where needed.
- The Operator must state its current context (all hosts, selected host, selected incident/operation) and allow changing it explicitly.
- A contextual link should open the correct target and preserve a useful route back to the originating workflow.

## 3. Operator surfaces

The Operator is one product feature with two presentations over the same durable conversations and operation state.

### Global side panel

- Available from every signed-in page.
- Supports short questions, one-step requests, quick investigation starts and monitoring existing work.
- Has an explicit context selector: current page/object, selected host, all hosts or another chosen scope.
- Shows recent conversations and active operations without becoming a separate operations engine.
- Expands into the full Operator workspace while preserving conversation, message history, selected context and operation IDs.
- Collapsing the panel does not stop or detach work.

Recommended behavior: a non-modal, resizable panel on wide screens; a full-screen sheet/page-like panel on small screens. Initial width is a design choice to test; reserve enough space for real transcripts and operation cards.

### Full Operator workspace

- Dedicated route/area for long conversations, complex troubleshooting and user-guided tasks.
- Conversation and operation detail are linked, but they are not the same object.
- It can show a conversation transcript alongside the active operation graph, evidence and inspector.
- Users can move between conversation and operation views without losing either.
- Supports multiple conversations and long-lived operations.

## 4. Operations and incidents are separate first-class concepts

**Incident:** a detected abnormal condition.

**Operation:** work performed by Domovoi. An operation may be started by an incident, a user request, a schedule, a policy or another operation. Not all operations belong to an incident.

Relationships:

- One incident may have multiple operations (investigate, repair, monitor, follow-up).
- One operation may relate to one or more incidents, hosts, services or knowledge entities.
- A user-requested operation may exist without any incident.
- An operation may discover a new incident as a result, but these are still distinct records.
- A conversation can initiate or discuss an operation; the conversation is not the operation itself.

The Operations page is a unified view of work. The Incidents page remains a view of detected problems.

## 5. Page map

```text
Domovoi
├── Control Room
│   └── Overview
├── Operations
│   ├── All operations
│   ├── Running / waiting / scheduled / completed views
│   └── Operation detail
│       ├── Orchestration graph
│       ├── Findings and plan
│       ├── Evidence and capability calls
│       ├── Execution, verification and monitoring
│       └── Timeline / technical trace
├── Incidents
│   ├── Incident list / filters
│   └── Incident detail
│       ├── Summary and evidence
│       ├── Related operations
│       ├── Current status and recommended response
│       └── History
├── Infrastructure
│   ├── Hosts
│   ├── Host detail
│   │   ├── Overview
│   │   ├── Services and workloads
│   │   ├── Capabilities
│   │   ├── Monitoring and checks
│   │   ├── Network and security
│   │   ├── Knowledge and dependencies
│   │   └── Activity
│   ├── Service/workload detail
│   └── Infrastructure topology
├── Capabilities
│   ├── Discover catalog
│   ├── Installed
│   ├── Deployments / updates
│   ├── Capability detail
│   └── Install / configure flow
├── Knowledge
│   ├── Knowledge browser
│   ├── Entity detail
│   ├── Relationship / graph explorer
│   ├── Notes and runbooks
│   └── Suggested knowledge
├── Policies & Autonomy
│   ├── Policy overview
│   ├── Policy detail / editor
│   ├── Host policy
│   ├── What-if simulator
│   └── Decision history
├── Integrations
│   ├── Connected integrations
│   ├── Catalog / add integration
│   └── Integration detail
└── System
    ├── Settings and models
    ├── Notifications
    ├── Audit log
    ├── Backups / restore
    ├── Updates
    └── Diagnostics
```

Not every item must be a permanent sidebar entry. Some are nested routes or tabs on the relevant object.

## 6. Navigation principles

- Distinct work areas deserve distinct destinations; do not overload one generic “Dashboard” page.
- Detail views should be deep-linkable and restore the selected tab, filter and meaningful view state when safe.
- Avoid very deep navigation chains. A user should normally reach any object in a few steps through navigation, contextual links or search.
- The same entity should have one canonical detail page. Other pages can preview it but should not create competing copies of its configuration.
- Use tabs for peer views of one entity, not as a substitute for information architecture.
- Large graph workspaces, long-running operation workspaces and complex installers should receive the space of a dedicated page.
- Provide breadcrumbs or a clear back path for nested object pages.
- Do not use a modal as the only way to access a detailed capability manifest, full policy editor or long diagnostic workflow.

## 7. Universal search versus command palette

The current `Ctrl/Cmd+K` palette is an early foundation, not the completed search product.

Target search should find, where the user is authorized:

- Pages and commands.
- Hosts, agents, services, containers and other infrastructure entities.
- Active and historical incidents and operations.
- Capabilities and installed providers.
- Knowledge facts, entities, relationships, notes and runbooks.
- Policies and relevant decision records.
- Integrations and settings.

Search results should be grouped by type, show distinguishing metadata and status, support keyboard navigation, and open the canonical detail page. Commands (for example “open Operator”, “add host” or “install capability”) must be clearly distinguished from objects returned by search.

Do not imply a full search index exists until the data sources, permission filtering, freshness, ranking and indexing behavior are implemented. Keep search provider-independent so semantic search may be added later without changing the UI contract.

## 8. Cross-page entity links

Important objects should form a navigable network:

- Host ↔ agent ↔ capabilities ↔ services/workloads ↔ incidents ↔ operations.
- Operation ↔ incident(s) ↔ evidence ↔ capabilities ↔ policy decisions ↔ experience records.
- Knowledge entity ↔ facts ↔ relationships ↔ notes ↔ incidents/operations.
- Capability ↔ provider/package ↔ installed agents ↔ policies ↔ runs/operations.
- Policy rule ↔ affected capability/host ↔ simulation ↔ historical decision.

This interconnectedness is a core usability feature, not merely a graph visualization requirement.

## 9. Open architecture choices for later design

- Whether the Control Room supports multiple saved workspaces per user or one customizable layout initially.
- Whether infrastructure topology is a subpage of Infrastructure or a global visualization entry point.
- Whether Experience gets a dedicated global explorer or remains accessible through Operations, Incidents and Knowledge.
- The final sidebar grouping and whether “Incidents” is grouped under “Respond” or has its own top-level section.
- Whether Integrations and Capabilities are separate top-level sections (recommended) or combined in a catalog with clear subtypes.
