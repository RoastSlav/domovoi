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

Three profiles ship in v1, plus Custom. They are one set of components with a category-to-visibility table (section 5), not three layouts. The profiles, the table and the shared visibility mechanism are defined before any page is implemented.

| Profile | Behavior | Intended use |
|---|---|---|
| **Detailed** | Everything shown. The default for a new owner. | Reference design; operators who want to know exactly what is happening. |
| **Standard** | P0 and P1 shown; P2 and P3 collapsed (one click away, with a count). | Balanced everyday use. |
| **Essential** | P0 and P1 shown; P2 collapsed; P3 hidden, with a visible "+N details" indicator that reveals them. | People who prefer a quieter surface. |
| **Custom** | Per-page overrides of one of the profiles above. | Users who want a compact dashboard and detailed operations, or another combination. |

Rules:

- **P0 is never hidden or collapsed in any profile**, including Custom. A test renders each page in every profile with the same fixture and asserts that every P0 element is present and expanded.
- Hidden or collapsed content stays reachable and is not rendered until revealed; profiles never delete or omit durable history.
- These profiles are display preferences only. They must never change authorization, tool availability, audit retention, execution behavior or security controls. A setting that affects authority belongs in the policy system, not the display profile.
- The UI reads the profile through one provider. Whether it is stored on the hub or only in the browser is a backend decision (see doc 17, section 11); the behavior above does not depend on it.

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
| **Integration** | A connected external system or service that provides data or operations. | For example router API, monitoring system or notification channel. Hub-side integrations are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1"). |
| **Incompatible** | A host state: its agent speaks a different protocol minor than the hub. | Shown with an upgrade-required explanation, the required protocol and the agent's version; its history is kept, also after a hub restart. After a hub upgrade, a host that has not reconnected shows "expected incompatible". Never presented as offline. |
| **Helper** | The root-owned first-party process through which the unprivileged agent performs privileged host operations (owner decision D-29, RW-39). | Installed only by the owner-controlled OS install path, never by a package. Where it is missing or in error, affected checks and capabilities say "helper unavailable", not "failed". A host whose agent still runs as root is labelled "privilege boundary not in effect".
| **Locked Deny** | An owner-declared prohibition (default off) that no narrower policy rule overrides. | Shown distinctly from ordinary rules; changing it needs step-up. A policy Deny is never offered for approval. |

The exact navigation labels can evolve after page planning, but this vocabulary should remain coherent.

## 5. Information hierarchy: priority and space are separate

Every meaningful page element receives two independent classifications.

### Display priority

| Priority | Definition |
|---|---|
| **P0 — Safety and state** | Safety, approval, audit and current-state content: what is happening now, what needs a decision, what authority applies, what was recorded. Never hidden or collapsed in any profile. |
| **P1 — Primary** | The page's main content and frequent actions. Visible without navigating away; may share space with P0. |
| **P2 — Technical detail** | Useful diagnostics, raw inputs and outputs, history and secondary actions. Collapsed in Standard and Essential. |
| **P3 — Metadata** | Rarely needed, verbose or low-frequency metadata (ids, timings, model parameters). Collapsed in Standard; hidden behind "+N details" in Essential. |

Every page element carries exactly one category, written in its page specification and in the code through the shared visibility primitive. Visibility per profile follows from the category alone:

| Category | Detailed | Standard | Essential |
|---|---|---|---|
| P0 | shown | shown | shown |
| P1 | shown | shown | shown |
| P2 | shown | collapsed | collapsed |
| P3 | shown | collapsed | hidden, "+N details" indicator |

A page that needs a different result for one element changes the element's category, not the table.

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

- Detailed/Standard/Essential presentation profiles, with Custom as per-page overrides of one of them.
- Overview dashboard widgets can be added, removed, rearranged, resized and saved per user. DEFERRED to v1.x (demand unvalidated; `docs/rework/06` "Deferred and after-v1"): v1 ships one carefully designed default layout.
- Per-page preferences for density and default expanded sections.
- Persist filters, table columns, sort order and selected views where it helps repeat workflows.
- A user may reset to a strong default layout at any time.

Do not require the user to customize the UI before it becomes useful. Defaults must be carefully designed and coherent. The default profile for a new owner is Detailed; Standard and Essential are one setting away and remembered. Because the default shows everything, the Detailed layouts of every page are designed first and tested on a phone and a laptop, not only on a wide desktop.

## 8. Control-plane principle

The UI should be the normal place to configure product behavior. If a supported setting can be safely changed in the UI, expose it there.

A control may remain outside ordinary UI management only for a deliberate security or recovery reason (for example initial root-of-trust establishment or recovery from a compromised hub). Even then, the UI should explain the reason and offer safe lifecycle actions where possible. Secret values and private key material may remain write-only/non-readable while still supporting set, replace, test, rotate or revoke actions.

## 9. Product quality is functional, not decorative

A beautiful screen that has stale state, lost context, confusing actions or flaky interactions is not a good UI.

Every important control must have predictable behavior and feedback. Every data view must distinguish the states in the canonical taxonomy (doc 15, section 10): loading, empty, first-load error, refresh error, stale, offline, unavailable, partial and unknown value (a host also has `Incompatible`). Live updates must not reorder or replace content in a way that loses user focus or an inspector selection.

The UI should feel dynamic because the product is genuinely active, not because it constantly animates. Motion must communicate state changes, direction, hierarchy or spatial relationships.

## 10. One owner in v1

Through v1 Domovoi has a single owner and no roles. The UI therefore has no permission-denied states and does not filter search or lists by permission. Where a future role would change what is shown, the specification names the place as a hook "for future roles" (for example the single function every action check goes through) and builds nothing more. Actor identity is still shown wherever an action is attributed (owner, system component, agent, policy rule).

## 11. Design review questions

Before approving a page specification, answer:

1. What user question does this page answer?
2. What is P0, P1, P2 and P3 here (category, not size)?
3. What is dominant/large/medium/compact/contextual?
4. Is there enough screen area for every dominant item at the minimum sizes in doc 16, section 5?
5. Should content be a page, subpage, tab, section, expansion or inspector—and why?
6. What can the user do here, and what happens after each action?
7. What changes live while the page is open?
8. Which states from the canonical taxonomy apply, and what does each look like?
9. What must survive navigation, reload, collapse/expand and reconnect?
10. How does the same page work in each presentation profile and at different viewport sizes?
11. What is the acceptance test for a correct, polished implementation?
