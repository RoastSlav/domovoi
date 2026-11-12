# 12 — UI product principles and experience goals

**Status:** agreed product direction plus recommended design rules. This is a target specification, not a requirement to preserve the current page layout.

## 1. The UI is a product surface, not a status dashboard

Domovoi's web UI is its primary operational control plane. It must let a user:

- Understand the state of the environment and what Domovoi is doing.
- Inspect evidence, hypotheses, decisions, capability calls, policy evaluations and results.
- Initiate investigations and operations from the UI or Operator.
- Manage hosts, capabilities, policies, knowledge, integrations and settings.
- Review history and learn why something happened.
- Configure almost every supported product behavior without editing files or using a CLI.

The UI should feel like a polished, information-rich operations environment: fast, responsive, modern and pleasant to use. It must not resemble a generic SaaS admin template or hide the system's real behavior behind vague summaries.

## 2. Design from requirements, not from existing markup

The current UI overhaul is a useful reference for brand, design tokens, current functionality, component quality and implementation constraints. It is **not** the target layout and must not anchor new information architecture.

For future design work:

1. Start from the user's goals, the product model and the information that must be visible.
2. Decide page boundaries, priority and real-estate needs before choosing components or CSS.
3. Consider new layouts, navigation, visualizations and interaction patterns without assuming the current page structure is correct.
4. Reuse current components only when they genuinely fit the new interaction and information hierarchy.
5. Preserve valued visual identity deliberately; do not preserve layouts merely because they already exist.

This is intentional. Incrementally polishing an unsuitable page structure is not the goal.

## 3. Full transparency, configurable presentation

**Agreed direction:** full transparency, configurable presentation and progressive disclosure.

The detailed experience is the design baseline. It is not a hidden expert mode that must be discovered to access useful diagnostic information.

On operational pages, the detailed baseline exposes:

- Actual orchestration phases and transitions.
- Individual reasoning requests at the level of request/response metadata and structured conclusions.
- Capability calls, arguments, targets, prerequisites, results, errors and timings.
- Evidence, provenance, freshness, hypotheses and uncertainty.
- Policy evaluation, matched rules, constraints and authorization outcome.
- Operation plans, execution states, verification and monitoring.
- Model/provider metadata, latency, token usage and cost where available.
- Relevant history, audit records and links to related entities.

A user may choose to collapse, summarize or hide secondary information in their own presentation. The system must not omit underlying events from durable history merely because the current view is compact.

Do not expose hidden model chain-of-thought. Expose useful and auditable information instead: model request metadata, tool calls, evidence returned, structured hypotheses/conclusions, decisions, explanations, policy results and execution traces.

### Presentation profiles

Recommended starting profiles:

| Profile | Behavior | Intended use |
|---|---|---|
| **Detailed** | Full operational trace and technical metadata expanded where the page can support it. | Reference design; operators who want to know exactly what is happening. |
| **Standard** | Phase/step progress and key findings visible; low-level tool outputs and metadata are grouped but one click away. | Balanced everyday use. |
| **Essential** | Primary state, consequence and next action emphasized; technical detail remains accessible. | People who prefer a quieter surface. |
| **Custom** | Independent visibility/expanded-state preferences by page and information category. | Users who want a compact dashboard and detailed operations, or another combination. |

These profiles are display preferences only. They must never change authorization, tool availability, audit retention, execution behavior or security controls. A setting that affects authority belongs in the policy system, not the display profile.

## 4. Understandable terminology without dumbing down

The product should be approachable to someone new to self-hosting without concealing technical precision from an experienced operator.

Rules:

- Use consistent terms and explain unfamiliar concepts where they appear.
- Prefer a clear label plus a concise explanation over replacing a precise term with a vague euphemism.
- Keep domain-standard technical terms when they add precision: orchestration, capability, policy, verification, agent, host, container, TLS and so on.
- Explain terms contextually in page descriptions, inline hints, inspectors and the user guide. Do not force users through a glossary before they can operate the product.
- Never replace meaningful orchestration phases with a single generic “working” status in the detailed view.
- Keep the same term for the same concept across navigation, API-facing labels, chat, notifications and documentation.

### Product vocabulary

| Term | Meaning | Notes |
|---|---|---|
| **Host** | A machine or managed endpoint known to Domovoi. | The UI may explain it as “server or machine”; keep `host` in technical detail where accurate. |
| **Agent** | The local Domovoi process that reports from and executes permitted operations on a host. | Do not conflate a host with its agent. |
| **Service / workload** | An application, container, system service, database or other running component. | Show the concrete type when known. |
| **Incident** | A detected abnormal condition that needs investigation, monitoring or attention. | Existing project concept; not every user request creates an incident. |
| **Operation** | A durable unit of work Domovoi performs. It may be triggered by an incident, user request, schedule or automation. | The broad execution/history concept. The main page is **Operations**. |
| **Task** | A plain-language word for work Domovoi has been asked to do. | May be used in friendly copy; do not create a second backend abstraction unless needed. |
| **Operator** | The conversational interface for asking questions, initiating operations and following their progress. | Available as a global side panel and a full-page workspace. |
| **Capability** | A typed operation Domovoi can discover and invoke, such as inspect logs or restart a service. | Skills are one implementation/provider, not the whole capability model. |
| **Policy** | Owner-defined rules that decide what Domovoi may do and under which constraints. | Policy is configuration/authority; a permission is the result or grant. |
| **Knowledge** | Current or persistent information about the environment, with provenance and freshness. | Not the same as notes, history or policy. |
| **Experience** | Historical records of investigations, decisions, operations and outcomes. | Evidence of what happened, not automatically current truth or permission. |
| **Integration** | A connected external system or service that provides data or operations. | For example router API, monitoring system or notification channel. |

The exact navigation labels can evolve after page planning, but this vocabulary should remain coherent.

## 5. Information hierarchy: priority and space are separate

Every meaningful page element receives two independent classifications.

### Display priority

| Priority | Definition |
|---|---|
| **P0 — Essential** | Core to the page's purpose or urgent to a user's decision. Immediately visible in the detailed baseline. |
| **P1 — High** | Frequently used or important contextual information. Visible without navigating away; may share space with P0. |
| **P2 — Supporting** | Useful diagnostics, history or secondary actions. May be in a tab, secondary section or expandable group. |
| **P3 — On demand** | Rarely needed, verbose or low-frequency technical metadata. Available through an inspector, details drawer or advanced section. |

### Space requirement

| Size | Definition |
|---|---|
| **Dominant** | Needs a major share of the page to be usable, e.g. a complex orchestration graph. |
| **Large** | Needs substantial room, e.g. a detailed evidence table, history grid or rich editor. |
| **Medium** | A standard page section or side panel. |
| **Compact** | A small summary, status, toolbar or focused control. |
| **Contextual** | Appears only in response to selection/action: inspector, popover, dialog or side drawer. |

High priority does not imply large size. For example, “approval required” may be P0 but compact; the orchestration graph may be P0 and dominant. Page design must budget for both classifications.

## 6. Decide the right container, don't default to cards or tabs

Use the interaction and content to choose the presentation:

- **Dedicated page:** the content has a distinct purpose, several workflows or a dominant visualization.
- **Subpage:** a selected object needs a focused workspace of its own.
- **Tabs:** several peer views describe the same object, and users are likely to switch among them.
- **Sections:** related information belongs together and should be scanned as one page.
- **Expandable group:** information is useful but secondary on most visits; keep a clear count/status and preserve a discoverable entry point.
- **Inspector/drawer:** details must be examined without losing the user's current position or graph context.
- **Popover/menu:** a compact choice or small group of contextual actions.
- **Dialog:** a focused decision, destructive confirmation or short multi-step interaction. Do not put a large workflow in a modal merely to avoid page planning.

Tabs must not be used as a hiding place for P0 information. If the user needs two large areas simultaneously, use a split view, separate page or linked workspace instead.

## 7. The UI should be user-configurable, but not chaotic

Recommended customization:

- Detailed/Standard/Essential/Custom presentation profiles.
- Overview dashboard widgets can be added, removed, rearranged, resized and saved per user.
- Per-page preferences for density and default expanded sections.
- Persist filters, table columns, sort order and selected views where it helps repeat workflows.
- A user may reset to a strong default layout at any time.

Do not require the user to customize the UI before it becomes useful. Defaults must be carefully designed and coherent.

## 8. Control-plane principle

The UI should be the normal place to configure product behavior. If a supported setting can be safely changed in the UI, expose it there.

A control may remain outside ordinary UI management only for a deliberate security or recovery reason (for example initial root-of-trust establishment or recovery from a compromised hub). Even then, the UI should explain the reason and offer safe lifecycle actions where possible. Secret values and private key material may remain write-only/non-readable while still supporting set, replace, test, rotate or revoke actions.

## 9. Product quality is functional, not decorative

A beautiful screen that has stale state, lost context, confusing actions or flaky interactions is not a good UI.

Every important control must have predictable behavior and feedback. Every data view must distinguish loading, empty, failed, unavailable, stale, offline and partially available states. Live updates must not reorder or replace content in a way that loses user focus or an inspector selection.

The UI should feel dynamic because the product is genuinely active, not because it constantly animates. Motion must communicate state changes, direction, hierarchy or spatial relationships.

## 10. Design review questions

Before approving a page specification, answer:

1. What user question does this page answer?
2. What is P0, P1, P2 and P3 here?
3. What is dominant/large/medium/compact/contextual?
4. Is there enough screen area for every dominant item?
5. Should content be a page, subpage, tab, section, expansion or inspector—and why?
6. What can the user do here, and what happens after each action?
7. What changes live while the page is open?
8. What are all meaningful page states?
9. What must survive navigation, reload, collapse/expand and reconnect?
10. How does the same page work at different densities and viewport sizes?
11. What is the acceptance test for a correct, polished implementation?
