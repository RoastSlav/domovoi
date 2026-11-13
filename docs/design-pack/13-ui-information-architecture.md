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
| **Integrations** | DEFERRED past v1 (hub-side providers need hub-held credentials; `docs/rework/06` "Deferred and after-v1"). Connect external systems and manage their providers. | Connected integrations, add integration, integration detail |
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
| Attention center | P0 | Compact | The one durable list of approvals, questions, manual interventions, failures and completed operations, with unread state; never rely only on toasts. Control Room "Waiting for you" and the Approvals page are views over this same list. |
| Autonomy state | P0 | Compact | Shows when autonomy is paused or any rule is suspended by its circuit breaker; links to Policies & Autonomy. The pause switch is human-only. |
| Incompatible-agent banner | P0 | Compact | Appears while any host is `Incompatible` or "expected incompatible" after a hub upgrade (owner decision D-20, RW-17): names the host(s), the exact required protocol and the agent's version, and says to upgrade the hub first and then each agent through the install procedure. Links to the host; history stays readable and the host is never shown as merely offline. |
| Root-agent warning banner | P1 | Compact | Appears while any host's agent runs as root (owner decision D-29, RW-39): the privilege boundary is not in effect, unattended execution is off on that host, and the owner can rerun the installer. Links to the host, where the skill runner's status and a missing or failing helper are shown too. |
| Account/user menu | P2 | Compact | Profile, preferences, theme and sign out. |

The shell may evolve from the current sidebar/topbar layout. Do not assume all actions must live in the top bar or that the current sidebar groups must remain.

### Global keyboard model

Recommended defaults:

- `Ctrl/Cmd+K`: global search and command palette (the current overhaul already uses this shortcut).
- `Ctrl/Cmd+J`: open/focus the Operator side panel.
- `Escape` closes one thing per press, in this order: tooltip, popover or menu; dialog or palette; then the panel or inspector that has focus (if none has focus, the one opened most recently); then the graph selection. Escape never stops or cancels an operation; those need their own control and confirmation.
- `Ctrl/Cmd+K` and `Ctrl/Cmd+J` open separate launchers (search and Operator); neither replaces the other.
- `/` may focus the current page's search field only when focus is not already inside an editable control.
- Shortcuts must be discoverable and user-configurable where conflict is possible.

### Global context behavior

- A host/environment selector should be consistent across pages that support scoping.
- **Host scope rule.** Pages follow the host selector, or say why they do not ("all hosts" note). Opening a page about one host shows that host and sets the selector to it; leaving restores the choice the person had before. Changing the selector while on a host page navigates to the chosen host's page.
- Search results and deep links must preserve scope where needed.
- The Operator must state its current context (all hosts, selected host, selected incident/operation) and allow changing it explicitly.
- A contextual link should open the correct target and preserve a useful route back to the originating workflow.

## 3. Operator surfaces

The Operator is one product feature with two presentations over the same durable conversations and operation state.

### Right-hand slot precedence

The shell has one right-hand slot, shared by the Operator panel and the contextual inspector. It never overlaps page content and never shifts it while animating (panels move by transform; content is padded, not reflowed mid-transition).

- **Wide screens** (the page keeps at least 640 px of content width after the sidebar and slot): the Operator panel and the inspector can sit side by side.
- **Narrower screens:** the surface opened most recently wins the slot; the other collapses to a tab in the slot's header and keeps its state. Opening the collapsed one swaps them.
- **Phones:** the slot is a full-screen sheet, one surface at a time.
- Closing the surface in front reveals the collapsed one; neither is destroyed by the other opening.

### Global side panel

- Available from every signed-in page.
- Supports short questions, one-step requests, quick investigation starts and monitoring existing work.
- Has an explicit context selector: current page/object, selected host, all hosts or another chosen scope.
- Shows recent conversations and active operations without becoming a separate operations engine.
- Expands into the full Operator workspace while preserving conversation, message history, selected context and operation IDs.
- Collapsing the panel does not stop or detach work.
- Conversations are durable and stored on the hub. Host or global scope is explicit per conversation and shown in the header. A response that arrives for a superseded request is discarded, not shown. The composer draft is kept locally until the send is committed, so a failed send or a reload loses nothing.

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

The Operations page is a unified view of work. The Incidents page remains a view of detected problems. Pre-existing actions appear in the Operations list as read-only **Legacy** entries; no steps or approvals are invented for them.

### One home for approvals

An approval is a step of an operation, so **operation detail is its canonical home**. It is rendered by one structured approval card component (doc 14, section 5) that shows contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, whether the approval binds the v1 or the v2 plan hash (labelled during the migration, RW-11) and the policy decision; model text is secondary and labelled. Incident detail, the Control Room, the Operator and the attention center embed that card or link to the operation; none builds its own version. Incident controls (acknowledge, mute, dismiss) stay separate from operation controls (stop, cancel, retry) and from approval controls.

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
│       ├── Step timeline / tree (built first) and orchestration graph
│       ├── Similar past operations
│       ├── Historical playback (the last U2 deliverable)
│       └── Timeline / technical trace
├── Incidents
│   ├── Incident list / filters
│   └── Incident detail
│       ├── Summary and evidence
│       ├── Related operations
│       ├── Current status and recommended response
│       ├── Similar past operations
│       └── History
├── Infrastructure
│   ├── Hosts
│   ├── Host detail
│   │   ├── Overview
│   │   ├── Services and workloads
│   │   ├── Capabilities
│   │   ├── Monitoring and checks
│   │   ├── Network and security (management paths: provenance, freshness, covered or uncovered by the helper's verified set, what is blocked)
│   │   ├── Knowledge and dependencies
│   │   └── Activity
│   ├── Service/workload detail
│   └── Infrastructure topology
├── Capabilities
│   ├── Discover catalog (first-party and owner-signed packages; third-party registries DEFERRED)
│   ├── Import package (owner-signed archive and signature)
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
│   ├── Policy detail / editor (locked rules, publication validation report)
│   ├── Host policy
│   ├── What-if simulator
│   └── Decision history
├── Attention
│   └── Approvals, questions, failures, completions
├── Integrations
│   ├── Connected integrations
│   ├── Catalog / add integration
│   └── Integration detail
└── System
    ├── Settings and models
    ├── Notifications
    ├── Audit log (legacy and chained history verified separately)
    ├── Data retention
    ├── Backups / restore
    ├── Updates (versions and upgrade-required status; no agent self-update)
    └── Diagnostics
```

Not every item must be a permanent sidebar entry. Some are nested routes or tabs on the relevant object.

## 6. Navigation principles

- Distinct work areas deserve distinct destinations; do not overload one generic “Dashboard” page.
- Detail views should be deep-linkable and restore the selected tab, filter and meaningful view state when safe.
- **Depth rule:** at most three levels: area, object, tab. Anything deeper is a section on the object page, an inspector or a linked page, not a fourth level. A user should normally reach any object in a few steps through navigation, contextual links or search.
- Tabs on an object page never hold P0 content that a different question needs at the same time (for example an approval request or current state); that stays above the tabs.
- The same entity should have one canonical detail page. Other pages can preview it but should not create competing copies of its configuration.
- Use tabs for peer views of one entity, not as a substitute for information architecture.
- Large graph workspaces, long-running operation workspaces and complex installers should receive the space of a dedicated page.
- Provide breadcrumbs or a clear back path for nested object pages.
- Do not use a modal as the only way to access a detailed capability manifest, full policy editor or long diagnostic workflow.

## 7. Universal search versus command palette

The current `Ctrl/Cmd+K` palette is an early foundation, not the completed search product.

Target search should find:

- Pages and commands.
- Hosts, agents, services, containers and other infrastructure entities.
- Active and historical incidents and operations.
- Capabilities and installed providers.
- Knowledge facts, entities, relationships, notes and runbooks.
- Policies and relevant decision records.
- Integrations and settings.

Search results should be grouped by type, show distinguishing metadata and status, support keyboard navigation, and open the canonical detail page. Commands (for example “open Operator”, “add host” or “install capability”) must be clearly distinguished from objects returned by search.

Do not imply a full search index exists until the data sources, freshness, ranking and indexing behavior are implemented; each result group says what it covers. With one owner there is no permission filtering in v1; the search layer keeps a single filter point as a hook for future roles. The palette needs an asynchronous result source (loading, no results, failure) in addition to its current static groups. Keep search provider-independent so semantic search may be added later without changing the UI contract.

## 8. Cross-page entity links

Important objects should form a navigable network:

- Host ↔ agent ↔ capabilities ↔ services/workloads ↔ incidents ↔ operations.
- Operation ↔ incident(s) ↔ evidence ↔ capabilities ↔ policy decisions ↔ experience records.
- Knowledge entity ↔ facts ↔ relationships ↔ notes ↔ incidents/operations.
- Capability ↔ provider/package ↔ installed agents ↔ policies ↔ runs/operations.
- Policy rule ↔ affected capability/host ↔ simulation ↔ historical decision.

This interconnectedness is a core usability feature, not merely a graph visualization requirement.

## 9. Open architecture choices for later design

- Control Room customization (saved workspaces or a customizable layout) is DEFERRED to v1.x (demand unvalidated; `docs/rework/06` "Deferred and after-v1"); v1 ships one default layout.
- Whether infrastructure topology is a subpage of Infrastructure or a global visualization entry point.
- Whether Experience gets a dedicated global explorer. v1 surfaces it as "similar past operations" on incident and operation pages, with resolution summaries, marked historical and never an authorization; a global explorer is not in v1.
- The final sidebar grouping and whether “Incidents” is grouped under “Respond” or has its own top-level section.
- Whether Integrations and Capabilities are separate top-level sections (recommended) or combined in a catalog with clear subtypes.
