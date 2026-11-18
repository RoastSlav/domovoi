# 32 — Layered context, item statuses, evidence dedupe and one provider-chain loop

| | |
|---|---|
| Branch | `layered-context` |
| Milestone | M4 |
| Depends on (must be merged) | 31 (and through it 26 and 27, plus 24, 25 and 28). Every name taken from them below is "introduced by prompt NN: verify in code before use". If 31 is not merged, stop and report. Prompt 17's prompt cap (`TriageSettings::max_prompt_bytes`, `fit_to_cap`) is already merged and is used as it is |
| Can run in parallel with | 34, 35, 36 (order-flexible, never concurrent). 34 and 35 each add one context source and wire it through `TriageDeps.extra_context`, a field this prompt introduces (6.9, 6.10), so **they merge after this prompt**. 36 rewrites the model plumbing of `triage::chat`; whichever of 32 and 36 runs second adapts to the other (3.7). 58 (remove the legacy triage path) may have merged: the parts marked **(legacy path)** apply only while `run::outcome` still exists |
| Produces (shared contracts) | Engine: `domovoi_engine::context::{Aggregator, ContextPlan, ContextPurpose, ContextBudget, LayerCaps, LayeredContext, Section, Omission, CONTEXT_GUIDE}`, the built-in sources (`IncidentSource`, `SignalsSource`, `ProfileSource`, `NotesSource`, `ChangesSource`, `SimilarIncidentsSource`, `SkillsSource`, `ThreadSource`); `domovoi_engine::TriageDeps::extra_context: Vec<Arc<dyn ContextSource>>`, **the one registration point for every later context source**; `domovoi_engine::triage::chain::{Attempt, AttemptError, ChainState, run_chain}` (crate-private), the one provider-chain loop; the tool-loop read ledger that stops identical output being sent twice. **The contract a Knowledge-layer source must follow (6.9), which prompt 34 implements by wrapping `KnowledgeRead`; prompt 35 adds its experience source the same way (6.10).** Prompt file: `crates/domovoi-engine/prompts/context_guide.md` |
| Consumes | prompt 31: the `ContextSource` port and the data types `ContextSourceId`, `ContextLayer`, `ContextItemId`, `ContextItem` (`render`), `ItemStatus`, `Origin`, `Provenance`, `ContextScope`, `ContextSelector`, `ContextQuery`, `ContextBatch`, `ContextGap`; `EvidenceSource`; `FakeContextSource` and `domovoi_testkit::contracts::context`; `triage::meta` (the tool loop, `converse`, `EvidenceRecord`, `ToolSet`, the `request_evidence` dispatch); `triage::run::{providers, attempt, generate}` as 31 left them; `META_SYSTEM_PROMPT`; prompt 27: the `BuildContext` handler and its call `context::build`; prompt 17: `max_prompt_bytes`, `fit_to_cap`, `complete`; existing `Redactor`, `Clock`, `HostCatalog`, `IncidentRepo`, `SignalRepo`, `NoteRepo`, `ActionRepo`, `SkillSummary` |
| Launch gate | none |
| Suggested model | medium |

## 1. Objective

After this change every prompt that carries context to a model is built the same way, and the three model-backed paths share one fallback loop:

- **Layered context.** What the model is told about the incident, the host and the server is a set of **context items** produced by `ContextSource` implementations (the port prompt 31 defined). An **aggregator** asks each source, normalises what comes back, drops duplicates, applies a **per-layer byte cap** and a **total cap derived from the existing per-call prompt cap** (prompt 17's `max_prompt_bytes`), and renders the result in the fixed layer order `Incident, Host, Evidence, Experience, Knowledge, Capabilities`. The seven parts the triage bundle has today (incident, host profile, recent signals, what changed, similar incidents, host notes, skills) become built-in sources; the follow-up thread block becomes one more. Prompts 34 (knowledge) and 35 (experience) add their sources through one registration point and never touch the aggregator.
- **Item statuses in the prompt: unknown is not empty.** Every item is rendered with its layer, source, status (`known`, `UNKNOWN: <reason>`, `stale since <time>`, `disputed with <id>`) and origin (`observed`, `declared by a person`, `inferred`, `historical evidence, not authorization`). A source that could not answer renders as an explicit unknown line, never as `None.`; the only time `None.` appears is when a source was asked and had nothing. Anything the caps drop is **named** in a closing "Context left out" section, so omission is also never silent. The system prompts gain one short guide that tells the model how to read the tags.
- **Evidence dedupe.** (a) The aggregator removes an item that two answers delivered twice (same stable id and same status), keeping the newest. (b) The tool loop does not send the model the same read output twice: when a `request_evidence` read returns text byte-identical to an earlier read of the same call in the same conversation, the model gets a one-line note instead of the repeated text. Nothing is skipped: the read still happens, still counts against the budgets and is still recorded.
- **One provider-chain loop.** Triage (`run::providers`), follow-up (`followup::answer`) and chat (`chat::answer`) each carry their own copy of "try the primary, then each fallback, rebuild once per refused capability, trim the recorded tool use, collect the failure reasons". It becomes one function, `run_chain`, driven by a small `Attempt` trait; the three paths keep their own prompts, tool sets and answer handling.

It does **not** add the knowledge or experience sources, a meta-tool, a config key, a migration, a metric, an HTTP route or any wire change. It does not change what a source may fetch from the world: the evidence boundary, redaction and every budget stay as they are.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**Why layers (04 §10 and §16, RW-14, RW-25, D-02).** The model is one more reader of what Domovoi knows, and it must be able to tell what kind of knowledge each line is. Facts and summaries sent to it carry their source and status; `disputed` and `stale` are labelled; a retracted fact is never sent. Experience from past operations is evidence about the past: when it is supplied to reasoning each item names its operation, date and source, inside a block labelled "historical evidence, not authorization", and past approvals or successes never satisfy policy. The prompt has a total byte cap per call plus per-layer caps. And "one provider-chain loop shared by triage, follow-up and chat" is a recorded fix: there are three copies today.

**Trust boundaries (INV-5, INV-9, D-02).**
- A **source** is read-only and bounded (prompt 31's eight obligations: read-only, bounded, redacted, scoped, unknown-is-not-empty, deadline, no authority, stable identity). Sources run in the engine, over ports it already holds. A source never holds a unit of work, an operation handle, an authorization or an agent link.
- The **aggregator** trusts no source. It re-checks bounds, layer, redaction and origin on everything it receives, because sources will be written by other prompts. It is the only code that turns items into prompt text for the context part of a prompt.
- Everything in a context item is **data**: a note, a log line, a similar incident, a fact. None of it is an instruction. No item field can express permission; an item from a past operation always renders `historical evidence, not authorization`.
- The model never asks for a layer or a source. Which sources run is decided by the engine from the **purpose** of the call (investigation, follow-up, chat).

**Data flow.** `BuildContext` (investigation), `followup::answer` and `chat::answer` call the aggregator with a `ContextPlan` (purpose, scope, budget). It asks each applicable source for its items, normalises and caps them, and returns a `LayeredContext`; `render()` gives the text that becomes the opening user message. For an investigation in operation mode that text travels in the `CallModel` effect payload (about 32 KiB at most, redacted), so a model call re-issued after a crash sees the same context. Then the chain loop (`run_chain`) tries each provider with that opening; the existing tool loop (prompt 31's `triage::meta`) does the rest.

**Design decisions taken here, not to be revisited.**

| Decision | Why |
|---|---|
| The `ContextLayer` enum is **not** extended; no exhaustive match changes | the six layers cover every source in the plan (knowledge 34, experience 35); fewer touched files, no gate edit |
| The aggregator and the built-in sources live in the engine (`src/context/`); sources hold clones of `TriageDeps` (cheap, all `Arc`) | they need the ports `TriageDeps` already carries; no new crate edge, no new dependency |
| Sources are called **one after the other**, in registration order, each bounded by its own deadline; no new dependency (`futures`) and no concurrency | deterministic output and tests; the work is a few store reads and at most one agent read (the old code was sequential too) |
| A source is asked for `Overview` when it supports it, and also for `Search { text }` when it supports it and the plan has search text; a source that supports neither is skipped without a section | one generic rule lets later sources plug in without aggregator changes; overlapping answers are what dedupe is for |
| The built-in sources are chosen by **purpose**; every source in `TriageDeps.extra_context` is added to all three purposes | a later source needs no list to edit |
| An investigation, whether it runs as an operation or on the legacy path, always uses `ContextScope::Incident { host, incident }` (the operation's first incident); `ContextScope::Operation` is not used by any built-in source | the built-in sources are incident-based; a source for a scope it does not serve answers with a gap (obligation 4) |
| Rendering is **per source**: one `## <title>` section per source, in layer order, each item rendered with `ContextItem::render()`; sections keep the titles the model reads today | existing prompts and tests name "Skills you may propose", "Host notes" and the host-profile title; one format makes the status tags uniform |
| Caps: a fixed per-layer table, a total derived from `max_prompt_bytes`, a fixed shrink order, **nothing dropped silently** | the model must know that a missing item is unknown, not absent; deterministic output is snapshot-testable |
| Items of the `Experience` layer are forced to `Origin::Historical` by the aggregator | D-02 fail-safe: a later source cannot forget the label |
| Evidence dedupe in the tool loop compares **output**, never skips a read | a repeated read may legitimately return something new; a read that is not performed would change budgets, traces and recorded evidence |
| `ReasoningTurn.context_refs` (prompt 27) stay the fixed list of seven part names | the trace fixtures and the UI know those names; the refs describe the investigation's standard parts and are not edited here |
| The chain loop shares the **iteration, capability rebuild, tool-use trimming and failure text**; each path keeps building its own opening messages (including its prefetch for providers without tools) inside its `Attempt` | the three paths prefetch differently on purpose; the duplicated part is the loop around them |
| A chat about **one chosen host** gets the Host and Capabilities layers (profile, read-only skills) and any extra sources; a chat about **all hosts** is unchanged (the caller's overview, no tools) | a host chat benefits from the profile; the all-hosts chat has no scope to layer |

## 3. Repository context

Verify each item first. Marks: **(existing, path:line)** or **(new)**. Lines are at the time of writing and are for the committed code before prompts 31 and 27 (re-find them).

### 3.1 The context bundle today (all in `crates/domovoi-engine/src/triage/`)

- `context::build(shared: &Shared, incident: &Incident, skills: &[SkillSummary]) -> String` **(existing, `context.rs:41-67`)** builds seven parts in this order and joins them as `## <title>\n\n<body>\n\n`, each body trimmed, replaced by `None.` when empty and cut to `PART_BYTES = 4 KiB`, then redacts the whole text once:

```rust
let parts = [
    ("Incident", snapshot(incident)),
    ("Host profile (the normal state of this host)", profile_note(shared, incident).await),
    ("Recent signals, newest first", signals(shared, incident).await),
    ("What changed in the last 24 hours", changes(shared, incident).await),
    ("Similar resolved incidents", similar(shared, incident).await),
    ("Host notes", notes(shared, incident).await),
    ("Skills you may propose", catalogue(skills)),
];
```
- The part functions: `snapshot` (`:75`), `signals` (`:99`, `SIGNALS = 10`; a repository error returns an empty string), `changes` (`:128`: reboot incidents of the last 24 h from `IncidentRepo::list`, then, **only if the host is `Presence::Connected`**, one `DockerEvents { container: None, since_minutes: 1440 }` read through `deps.evidence.collect_evidence` with `CHANGES_DEADLINE = 2 s`; a disconnected host or a failed read silently adds nothing), `similar` (`:170`, `SIMILAR = 3`, same host and same check, `Resolved` only, query = target or check), `profile_note` (`:210`, the note tagged `host-profile`, text cut at the first `## Details` or the older `## What the tools said` heading), `notes` (`:234`, `NOTES = 3` of `NOTE_HITS = 8` hits, the profile note excluded), `catalogue` (`:258`, one line per skill). **Every one of them turns a read failure into an empty string, which the bundle then prints as `None.`: this is the "unknown shown as empty" defect this prompt removes.**
- Callers of `context::build`: `run.rs:117` (triage), `followup.rs:180`. Chat does not call it: `chat.rs:178` builds `host_opening(overview, context::catalogue(&skills))`, which is the caller's overview plus the read-only skill catalogue cut to `CATALOGUE_BYTES = 4 KiB` (`chat.rs:36, 88`).
- The follow-up also adds `about(shared, id)` **(`incident_context.rs:126`)**: the latest triage report (`report_block`), the incident's actions (`actions_block`, `MAX_ACTIONS = 20`) and its newest 20 timeline lines (`timeline_block`), cut to `BLOCK_BYTES = 8 KiB`, appended after the bundle in the opening message (`followup.rs:179-196`).
- The profile note's tag `PROFILE_TAG = "host-profile"` is copied from the hub because the engine cannot see it (`context.rs:31`); the hub refreshes a profile at least every 7 days (`crates/domovoi-hub/src/profile/mod.rs:50` `STALE_AFTER`).

### 3.2 The three provider loops (existing duplicate code)

All three do: for each provider of `shared.chain()` (primary then fallbacks): remember `used_before = used.len()`, keep a `handled: Vec<LlmCapability>`; run one attempt; on `LlmError::CapabilityRejected` for a capability not yet handled push it, `used.truncate(used_before)`, log, and run the attempt again from the top; on any other error record the reason `"<provider name> failed: <error>"` and move to the next provider.

| Path | Code | Attempt body | Failure text |
|---|---|---|---|
| Triage | `run::providers` **(`run.rs:193-243`)**, `run::attempt` (`:304-345`), `Failure { Provider(LlmError), Invalid(String) }` (`:246-255`), `provider_name` (`:186`) | `opening` (bundle, plus prefetched reads when the provider has no tools), `agent::converse`, `parse_report`, one repair turn through `agent::ask` | each reason redacted, joined with `"; "` |
| Follow-up | the loop in `followup::answer` **(`followup.rs:204-271`)** | messages: opening, remembered turns, prefetched `chat::host_output` when no tools, the question; `agent::converse` | `provider_failure(redactor, &reasons)` (`:69`): joined, redacted, cut to 500 chars, `TriageError::Provider` |
| Chat | the loop in `chat::answer` **(`chat.rs:188-274`)** | the same message shape; with a host and tools `agent::converse`, otherwise `agent::ask` with no tools | `provider_failure` |

`RunBudget` (`agent.rs:96`) and the failure `reasons` are shared across the providers of one run. The log lines differ slightly ("rebuilding the run without it" in triage, "asking again without it" in the other two). Prompt 31 changed `attempt`/`generate` to carry `staged`, `dropped` and `evidence` records beside `used`: read its merged code (3.7) and keep `evidence` truncated in step with `used` on a capability rebuild.

### 3.3 The prompt cap (prompt 17, merged)

`TriageSettings::max_prompt_bytes` (default 131 072; the config key `triage.per_run.prompt_bytes` rejects anything below 32 768) **(`triage/mod.rs:91, 105`, `crates/domovoi-config/src/validate.rs:68`)**. `complete()` calls `fit_to_cap(req, max_prompt_bytes)` before every provider call **(`complete.rs:25`)**: it replaces old tool results with a placeholder, then cuts the newest tool results down to a 1 KiB floor, **never touches the system prompt or the first user message**, and returns `LlmError::Unavailable` when that is still not enough **(`prompt.rs:50-121`)**. So the bundle in the first message must itself be small enough: this prompt derives its total cap from `max_prompt_bytes` (6.2).

### 3.4 Prompts

`SYSTEM_PROMPT = include_str!("../../prompts/triage_system.md")` **(`triage/mod.rs:55`)**: sections `What you can do`, `Untrusted data`, `Your answer`, `Verdicts`, `Style`. The test `the_triage_prompt_runs_a_read_skill_instead_of_proposing_one` **(`tests/services/report.rs`)** requires **exactly one** bullet line that contains `manual_steps`: the new text must not add another. `FOLLOW_UP_PROMPT` **(`followup.rs:28`)** and `CHAT_PROMPT` **(`chat.rs:15`)** are Rust string constants. Prompt 31 adds `META_SYSTEM_PROMPT` (`prompts/triage_meta_system.md`) and rewrites the follow-up and chat prompts: apply this prompt's text to what is merged.

### 3.5 Existing tests that read context text or its calls (you will re-run them; some change, 12.6)

- `tests/services/profile.rs`: the model sees `Host profile (the normal state of this host)`, the profile summary words, not the raw output behind it, and the text between `## Host notes` and `## Skills you may propose` does not contain the profile note title.
- `tests/services/reliability.rs`: `similar_incidents_come_from_the_same_host_only` (`:780`); `prompt_over_the_cap_is_trimmed_from_old_tool_results` (`:682`) and the ignored `measure_prompt_sizes` script **identical 8 KiB read results** several times (the same `host_overview` call, the link answers `x` * 8 KiB each time) and assert the placeholder and the 4 KiB newest result: with output dedupe (6.7) a repeated identical result becomes a note, so these two scripts must make each answer differ.
- `tests/services/conversation.rs:279-288`: the follow-up opening contains the report cause and `restart_container container=nginx status: proposed`.
- `tests/services/capability.rs:127-192`: a rebuilt prompt for a provider that lost `tools` carries `## Tool output gathered for you` and `You cannot call tools`.
- `tests/services/chat.rs`: counts the text parts of a request (`the overview, the two earlier turns, the question`; `the overview and the question`): **the chat opening must stay one message**; the host-chat tests expect exactly three prefetch reads and one `Journal` tool call: **the chat context must not read from the agent**.
- `tests/services/turns.rs:985-1060`: `store_delay` tests (`InMemoryStore::with_delay`, tokio paused time): the context reads now stop at their source deadline instead of waiting for the store; the tests assert only the outcome and request counts.
- `Setup { evidence, provider, usage, admission, store_delay, .. }` in `tests/services/harness.rs:31-68` builds `TriageDeps` in two places (`:282`, `:340`).

### 3.6 `TriageDeps` constructors

`TriageDeps` has 17 fields **(`triage/mod.rs:132-171`)** and five constructors: `crates/domovoi-hub/src/runtime/triage_build.rs:88`, `crates/domovoi-hub/tests/hub/api_harness.rs:342`, `crates/domovoi-engine/tests/drills/world.rs:319`, `tests/services/harness.rs:282` and `:340`. Prompt 31 added `resolver`; this prompt adds `extra_context` to each.

### 3.7 What earlier prompts provide (introduced by prompts 26, 27, 31, 36: verify every name in code)

- **31:** the port `ContextSource { id, layer, supports, fetch }` and the types listed under Consumes, exactly as 31 section 6.3 defines them (the status text, the `render` format, the eight obligations, `FakeContextSource`, the contract suite `domovoi_testkit::contracts::context`); `EvidenceSource` in `crates/domovoi-engine/src/context/evidence.rs` (id `evidence`, layer `Evidence`, answers `Read` only; item id `evidence:<tool>:<8 hex of the call's SHA-256>`); the module `triage/meta/` (the loop, `dispatch.rs`, `evidence.rs`); `LoopScope`, `LoopOutput`, `EvidenceRecord`, `Generated`; `agent::converse` on `ToolSet::Read`; `TriageDeps.resolver`; the rewritten follow-up and chat prompts.
- **27:** `BuildContext` calls `context::build(shared, incident, &skills)`, stores the returned redacted text in the `CallModel` payload (about 32 KiB at most) and finishes the turn; `ReasoningTurn.context_refs` is the fixed list `["incident","host_profile","signals","changes","similar","notes","skills"]`; the parity suite `tests/services/parity.rs`. **Prompt 31** appends the section `## This investigation so far` to that text when the handler resumes after a question (31 section 6.7d): that is up to about 1.4 KiB added after `build` returns.
- **36 (if merged):** `chat::answer_with` returns the text and the tools used; `RunBudget` carries an `AnswerSink`; the Operator builds the chat overview in the hub. `chat::answer`'s callers and signatures are unchanged by this prompt.
- **34, 35:** not required to be merged. If either has merged, step 1 records it; this prompt still adds no knowledge or experience source (6.9, 6.10).

## 4. Exact scope

### 4.1 `domovoi-core`

No change. `ContextLayer`, `ContextSource` and the item types stay exactly as prompt 31 left them.

### 4.2 `domovoi-engine` (each file at most 500 lines; split by responsibility)

| File | Change |
|---|---|
| `src/context/plan.rs` **(new)** | `ContextPurpose`, `ContextPlan`, `ContextBudget`, `LayerCaps` and the byte constants (6.2) |
| `src/context/aggregate.rs` **(new)** | `Aggregator::gather`: selectors, per-source deadline, collecting items and gaps (6.3) |
| `src/context/normalize.rs` **(new)** | the checks and repairs applied to every item a source returned (6.3 step 3) and the dedupe rule |
| `src/context/shrink.rs` **(new)** | per-layer caps, the total cap, the shrink order, `Omission` (6.4) |
| `src/context/layout.rs`, `src/context/render.rs` **(new)** | `Section`, `LayeredContext`, section titles, the fixed rendering (6.5) |
| `src/context/guide.rs` **(new)** | `pub const CONTEXT_GUIDE: &str = include_str!("../../prompts/context_guide.md");` |
| `src/context/sources/mod.rs`, `incident.rs`, `signals.rs`, `profile.rs`, `notes.rs`, `changes.rs`, `similar.rs`, `skills.rs`, `thread.rs` **(new)** | the eight built-in sources (6.6); their bodies are the code of `triage/context.rs` and `triage/incident_context.rs`, moved and made to report gaps |
| `src/context/mod.rs` | 31's file: add the modules and re-exports; `EvidenceSource` is untouched except that its item id function becomes `pub(crate)` if it is not already (the ledger of 6.7 reads the id of the item a fetch returned, so it needs no function; make it `pub(crate)` only if you find you do) |
| `src/triage/context.rs` | shrinks to the wiring: `build` (investigation, the same signature as today so prompt 27's handler and the legacy path need no edit), `build_follow_up`, `build_chat`, and `sources(shared, purpose, skills, connected) -> Vec<Arc<dyn ContextSource>>` |
| `src/triage/incident_context.rs` | **deleted**: its three block builders move into `context/sources/thread.rs` |
| `src/triage/chain.rs` **(new)** | `Attempt`, `AttemptError`, `ChainState`, `run_chain`, `thread_messages` (6.8) |
| `src/triage/run.rs` | `providers` and `attempt` use `run_chain`; `Failure` is replaced by `AttemptError` |
| `src/triage/followup.rs`, `src/triage/chat.rs` | each loop becomes an `Attempt` run by `run_chain`; follow-up and chat openings come from the aggregator (6.6, 6.8); `host_opening`, `CATALOGUE_BYTES` and the catalogue use in `chat.rs` go; `FOLLOW_UP_PROMPT` and `CHAT_PROMPT` gain the guide |
| `src/triage/meta/evidence.rs`, `src/triage/meta/ledger.rs` **(new)** | the read ledger and its use in the `request_evidence` dispatch (6.7) (31's file names: re-find them) |
| `src/triage/mod.rs` | `mod chain;`; `TriageDeps.extra_context` (6.9); module doc |
| `prompts/context_guide.md` **(new)**, `prompts/triage_system.md`, `prompts/triage_meta_system.md` | the guide text, added verbatim to both system prompts (6.10 and 6.11) |
| `src/lib.rs` | re-export `Aggregator`, `ContextPlan`, `ContextPurpose`, `LayeredContext`, `CONTEXT_GUIDE` |
| `tests/` | section 12; `tests/services/main.rs` gains `mod context;` and `mod chain;` |

### 4.3 `domovoi-hub`, `domovoi-testkit`, docs

- hub: `src/runtime/triage_build.rs` and `tests/hub/api_harness.rs` give `extra_context: Vec::new()`. No route, DTO, config key or OpenAPI change. No module is added to the hub (so nothing for `ZONES`/`UNZONED_MODULES`) and no hub command is added (no `Caller`, no use-case service).
- testkit: nothing new is required. Use `FakeContextSource` and `contracts::context` from prompt 31. If you need a failing store in a test, write a small local stub in the test file (`SignalRepo` has three methods).
- docs: section 15.

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| The Knowledge source (facts, relations, entities), `KnowledgeRead` reads | 34. This prompt does **not** depend on 34. 6.9 is the contract 34 must follow when it adds its source |
| The experience source (resolution summaries, `search_experience`, similar operations) | 35. 6.10 says only how it plugs in. The built-in `SimilarIncidentsSource` (past *incidents*, as today) stays here |
| A new meta-tool, a change to `MetaTool`, `ToolSet` or the plan validator | 31 (35 adds `search_experience`) |
| A new `ContextLayer` variant, a change to `ContextSource`, the item types or `ContextItem::render` | 31 owns them; this prompt only uses them |
| Reading from the agent while building a **chat** context, or any new live read in a context | the chat tests pin three prefetch reads; only `ChangesSource` reads the agent (as today) |
| Seeding the read ledger from the bundle, or deduping reads **across** the turns of an operation | a later turn (after a question, or the next cycle) may legitimately need a fresh read; out of scope |
| A config key for the caps, a new metric, a book table of caps | the caps are constants; logging uses `tracing` only |
| Editing `ReasoningTurn.context_refs` or any operation trace schema | 26 and 27 own them (6.2 explains why they stay) |
| Streaming, the Operator's conversation aggregate, the hub-built chat overview | 36 |
| Changing `fit_to_cap`, `complete`, `RunBudget`, admission, the repeat cache or any budget | prompt 17 and earlier. This prompt only derives the bundle cap from `max_prompt_bytes` |
| A migration, a protocol or wire change, an HTTP route, a DTO | none; no `cargo xtask openapi` |
| Any change to `HumanApproval`, `Authorization` or the approval paths | INV-1; a gate scan covers the new modules (12.5) |

## 6. Required interfaces and contracts

Marks: **fixed** = names, semantics and texts must match; **convention** = follow repository style. Every public item gets a doc comment saying what it is for. Never copy a prompt number, "the build plan" or an RW/D id into code, doc comments, commit messages, the book or the PR description: say what the behaviour is in plain words.

### 6.1 Module layout **convention**

`crates/domovoi-engine/src/context/` (31 created `mod.rs` and `evidence.rs`) gains `plan.rs`, `aggregate.rs`, `normalize.rs`, `shrink.rs`, `layout.rs`, `render.rs`, `guide.rs` and `sources/`. The aggregator never names `TriageDeps`: it takes `Arc<dyn ContextSource>`, an `Arc<dyn Redactor>` and an `Arc<dyn Clock>`. Only `sources/` and `triage/context.rs` know `TriageDeps`.

### 6.2 Plan, budget and caps **fixed**

```rust
// crates/domovoi-engine/src/context/plan.rs
/// Which call the context is for. It chooses the built-in sources and the size limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPurpose { Investigate, FollowUp, Chat }

/// What one aggregation is asked to build.
#[derive(Debug, Clone)]
pub struct ContextPlan {
    pub purpose: ContextPurpose,
    /// `ContextScope::Incident { host, incident }` for `Investigate` and `FollowUp` (an operation uses its
    /// first incident); `ContextScope::Host(host)` for `Chat`.
    pub scope: ContextScope,
    /// Text for `ContextSelector::Search`: `Some` only for an incident scope; already redacted, 1 to 200 chars.
    pub search_text: Option<String>,
    pub budget: ContextBudget,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    pub total_bytes: usize,            // cap on the rendered text, 6.4
    pub layers: LayerCaps,
    pub item_bytes: u32,               // ContextQuery::max_item_bytes: ITEM_BYTES = 4096
    pub overview_limit: u16,           // ContextQuery::limit for Overview: 10
    pub search_limit: u16,             // ContextQuery::limit for Search: 5
    pub source_deadline: jiff::SignedDuration,   // 5 s per source and selector
}
impl ContextBudget { pub fn for_purpose(purpose: ContextPurpose, max_prompt_bytes: usize) -> Self; }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerCaps { pub incident: usize, pub host: usize, pub evidence: usize,
                       pub experience: usize, pub knowledge: usize, pub capabilities: usize }
impl LayerCaps { pub const DEFAULT: LayerCaps; pub fn of(&self, layer: ContextLayer) -> usize; }
```

Constants (**fixed**; name them in `plan.rs`):

| Constant | Value |
|---|---|
| `LayerCaps::DEFAULT` | incident 16 KiB, host 16 KiB, evidence 8 KiB, experience 4 KiB, knowledge 8 KiB, capabilities 6 KiB |
| `ITEM_BYTES` | 4096 (the old `PART_BYTES`) |
| `MIN_ITEM_BODY` | 256 bytes: a body is never cut below it by a cap |
| `TOTAL_MIN` | 8 KiB |
| `TOTAL_MAX` by purpose | `Investigate` 32 KiB, `FollowUp` 40 KiB, `Chat` 24 KiB |
| `APPENDED_RESERVE` | 2 KiB, taken off `Investigate` only: room for the text the `BuildContext` handler appends after `build` returns (the resume section) |

`ContextBudget::for_purpose`: `total_bytes = (max_prompt_bytes / 4).clamp(TOTAL_MIN, TOTAL_MAX[purpose]) - reserve[purpose]` (reserve is `APPENDED_RESERVE` for `Investigate`, 0 otherwise). With the default 131 072-byte prompt cap that is 30 KiB, 40 KiB and 24 KiB; with the smallest allowed cap (32 768) it is 6 KiB, 8 KiB and 8 KiB. The quarter leaves the rest of the prompt cap for the system prompt, remembered turns and tool results, and the bundle is never trimmed by `fit_to_cap` (the first message is protected), so it must be small by construction. The layer caps stop one layer crowding out the others; the total is the limit that binds.

`ReasoningTurn.context_refs` (prompt 27) stay the fixed list of seven names. Do not edit them.

### 6.3 The aggregator **fixed**

```rust
// crates/domovoi-engine/src/context/aggregate.rs
pub struct Aggregator { /* sources: Vec<Arc<dyn ContextSource>> in registration order, redactor, clock */ }
impl Aggregator {
    pub fn new(sources: Vec<Arc<dyn ContextSource>>, redactor: Arc<dyn Redactor>, clock: Arc<dyn Clock>) -> Self;
    /// Never fails and never panics: a source that fails becomes a gap in its section.
    pub async fn gather(&self, plan: &ContextPlan) -> LayeredContext;
}
```
`gather`, in this order:

1. **Applicable selectors per source.** `Overview` if `source.supports(&ContextSelector::Overview)`. `Search { text: plan.search_text }` if the plan has search text and the source supports `Search`. A source that supports neither gets **no section** (this is how `EvidenceSource`, which answers exact reads only, stays out of a bundle). Overview is asked first.
2. **Fetch.** One `ContextQuery { scope: plan.scope.clone(), selector, limit, max_item_bytes: plan.budget.item_bytes, deadline }` per selector, one after the other, sources in registration order. `deadline = clock.now() + plan.budget.source_deadline`. The call is also wrapped in `tokio::time::timeout(deadline - clock.now(), ...)`; if no time is left or the timeout fires, the answer is `ContextGap { source, reason: "timed out" }` and no item (a source must honour the deadline itself; this is the backstop). A source's answer is never an error.
3. **Normalise every item** (`normalize.rs`, one pure function per rule so each is unit-testable). In this order:
   a. `item.layer` is set to `source.layer()` and `item.source` to `source.id()` (a mismatch is logged at `warn` with the source id only);
   b. in the `Experience` layer `provenance.origin` is forced to `Origin::Historical`;
   c. redact `title`, `body`, `provenance.reference`, `provenance.actor` and the `reason` of an `Unknown` status and of every gap with the `Redactor` (idempotent; **redact first, cut after**, so a cut cannot split a secret);
   d. `title`: trimmed, an empty one replaced by the source id, cut to 120 characters on a character boundary; `reference` cut to 120 characters; every `reason` cut to 200 characters;
   e. `body` longer than `plan.budget.item_bytes` is cut on a character boundary to that length and `truncated` is set;
   f. more items than the query's `limit` from one fetch: keep the first `limit`; the rest are counted in an `Omission` for that source (and logged at `info` with the count).
4. **Dedupe.** Two items are the same when they have the same `id` **and** the same status kind (`known`, `unknown`, `stale`, `disputed`). Keep the one with the newest `provenance.observed_at` (`None` is older than any time); on a tie keep the first in answer order; the kept item stays at its own position. A `Known` item and an `Unknown` item with the same id are **both** kept: the model must see that the newest read failed. A removed duplicate loses no information and is not an omission (log at `debug`).
5. **Sections.** One `Section { source, layer, title, items, gaps }` per applicable source, in registration order, then a stable sort by layer (so the order inside a layer is registration order). `title` comes from `layout::title_of` (6.5). A source with no item and no gap keeps its section: it renders `None.` (a known empty).
6. **Per-layer caps and the total cap** (6.4).
7. Return `LayeredContext { sections, omissions }`.

### 6.4 Caps and the shrink order **fixed**

Sizes are measured on the rendered text: `section_len` is the byte length of a section as 6.5 renders it (heading and trailing blank line included); `layer_len` is the sum over the layer's sections; `total_len` is the length of `render()` (the omission section included).

`SHRINK_ORDER = [Experience, Knowledge, Evidence, Host, Capabilities, Incident]` (the layer to give up first comes first). **Droppable:** an item whose status is not `Unknown`, except the first item of the `Incident` layer (the incident itself). `drop_last(layer)` removes the last droppable item of the layer (the last section's last item first) and records it; when the removed item has `Disputed { with }` and the item it is disputed with is in the same layer, or another item of the layer is `Disputed` with it, **that partner goes too** (a disputed pair is never split by a cap; a pair across layers is not guaranteed).

- **Step A, per layer, in layer order.** While `layer_len(layer) > caps.of(layer)`: `drop_last(layer)`; if nothing is droppable, stop. If it is still over, cut the longest remaining body in the layer, never below `MIN_ITEM_BODY`, setting `truncated`; repeat until it fits or no body can be cut. A cut item is **not** an omission.
- **Step B, the total.** While `total_len > plan.budget.total_bytes`: take the first layer in `SHRINK_ORDER` where `drop_last` succeeds; stop the loop when none does.
- **Step C, only if still over.** Drop `Unknown` items, last first, in `SHRINK_ORDER` (each is recorded in the omissions). Then cut the incident item's body down to `MIN_ITEM_BODY`. Then stop: gaps are never removed and the result may exceed the cap by the size of the gaps (bounded: each is under 300 bytes).
- A section left with no item **and no gap** by dropping is removed; its items are in the omissions. A section with a gap stays.
- `Omission { layer, source, items }` merges per `(layer, source)`. Log each non-empty omission at `info` with layer, source and count (no text).

### 6.5 Layout and rendering **fixed text** (snapshot-tested)

```rust
// crates/domovoi-engine/src/context/layout.rs and render.rs
pub struct Section { pub source: ContextSourceId, pub layer: ContextLayer, pub title: String,
                     pub items: Vec<ContextItem>, pub gaps: Vec<ContextGap> }
pub struct Omission { pub layer: ContextLayer, pub source: ContextSourceId, pub items: u32 }
pub struct LayeredContext { pub sections: Vec<Section>, pub omissions: Vec<Omission> }
impl LayeredContext { pub fn render(&self) -> String; }
```
`render` produces, for each section in order, `## <title>`, a blank line, then the body, then a blank line:

- items: each `ContextItem::render()` (prompt 31's format, unchanged), separated by one blank line; then, for each gap, the two lines `[<layer> | <source> | UNKNOWN: <reason>] <title>` and `nothing could be read from this source.`;
- no item and no gap: the single line `None.`.

After the last section, when `omissions` is not empty:

```
## Context left out to fit the size limit

- <layer>: <n> item(s) from <source>
What is listed here was not shown to you. It is unknown to you, not absent: do not assume there is nothing there.
```
(`<layer>` is the snake_case layer name; `<n> item` is singular for 1, `<n> items` otherwise; one bullet per omission, in layer order.) The text ends without a trailing newline. A plan with no sections renders as the empty string. The final text is passed through the `Redactor` once more.

`layout::title_of(source: &ContextSourceId, purpose: ContextPurpose, layer: ContextLayer) -> String`:

| Source id | Title |
|---|---|
| `incident` | `Incident` |
| `signals` | `Recent signals, newest first` |
| `profile` | `Host profile (the normal state of this host)` |
| `notes` | `Host notes` |
| `changes` | `What changed in the last 24 hours` |
| `similar_incidents` | `Similar resolved incidents` |
| `skills` | `Skills you may propose` (purposes `Investigate`, `FollowUp`); `Read-only skills you may use on this host` (`Chat`) |
| `incident_thread` | `This incident so far: report, actions and timeline` |
| any other id | by layer: `Incident` -> `Incident: <id>`, `Host` -> `Host: <id>`, `Evidence` -> `Evidence: <id>`, `Experience` -> `Historical evidence, not authorization: <id>`, `Knowledge` -> `Knowledge: <id>`, `Capabilities` -> `Capabilities: <id>` |

(The first seven titles are exactly the headings of today's bundle, so the prompts and tests that name them keep working. The `Experience` fallback carries the D-02 label in the heading itself.)

### 6.6 The built-in sources **fixed** (ids, layers, items, statuses); code layout **convention**

Every source is `#[derive(Clone)] pub struct XSource { deps: TriageDeps }` (except where stated), implements `ContextSource`, never writes, never returns an error and obeys prompt 31's eight obligations. Common rules:

- `supports` answers `true` for `ContextSelector::Overview` only, except where stated. They all ignore `limit` beyond `min(limit, their own cap)`.
- **Scope.** A source needing an incident, asked with `ContextScope::Host` or `Operation`, answers a gap `"this source needs an incident"` and no item.
- **Gaps and unknown items replace the old silent empties.** Where the old code swallowed a repository error and printed `None.`, the source answers a **gap** with the reason below. Redaction, bounds and ids follow the aggregator rules; the id of an item is `ContextItemId::new(&id, key)`.
- The deadline of `fetch` is `query.deadline`; a store call that has no time left is a gap `"timed out"`.
- The text of each body is what the old part function produced (move the code; do not reformat lines), so the model reads the same facts.

| Source id (struct) | Layer | Asked with | Items and bodies | Status and provenance | Gap or unknown (exact reason text) |
|---|---|---|---|---|---|
| `incident` (`IncidentSource`) | Incident | `Incident` scope | one item, key = the incident id, title `Incident`, body = the old `snapshot` lines | `Known`; origin `Observed`, reference `incident:<id>`, observed at the incident's last firing time | `IncidentRepo::get` error: gap `the incident could not be read`; `None`: gap `the incident no longer exists` |
| `signals` (`SignalsSource`) | Incident | `Incident` | up to 10: **one item** listing them (old `signals` lines), key = the incident id, title `Recent signals, newest first`; no signals: no item | `Known`; `Observed`, reference `signals of incident:<id>`, observed at the newest signal | `list_for_incident` error: gap `the recent signals could not be read` |
| `profile` (`ProfileSource`) | Host | `Incident` or `Host` | the `host-profile` note's summary (`note_summary`, cut at the raw-output heading), key = the note id, title `Host profile (the normal state of this host)`; no such note: no item | `Known`, or `Stale { observed_at: note.updated_at }` when `now >= updated_at + 8 days` (the hub refreshes at least every 7 days, one day of slack); `Observed` (the hub generated it from the host's own tool output), reference `note:<id>`, observed at `updated_at` | `NoteRepo::list` error: gap `the host profile could not be read` |
| `notes` (`NotesSource`) | Host | `Incident` | up to 3 items (of the 8 hits searched), one per note, key = the note id, title = the note title, body = the snippet; the profile-tagged note is excluded; no hits: no item | `Known`; origin `Declared`, reference `note:<id>`, observed at `updated_at` | `NoteRepo::search` error: gap `the host notes could not be searched` |
| `changes` (`ChangesSource`) | Evidence | `Incident` | (1) key `reboots`, title `Reboots`: the old line `- The host rebooted around <time>.` per reboot incident of the last 24 h (`IncidentRepo::list`, host filter, 50), none: no item; (2) key `docker_events`, title `Container events (24 h)`: when `catalog.presence(host)` is `Connected`, the result of **one** `EvidenceSource::fetch` for `DockerEvents { container: None, since_minutes: 1440 }` with deadline `min(query.deadline, now + 2 s)` re-stamped to this source (`source = changes`, id `changes:docker_events`), body `Docker events:\n<text>` when the text is not empty, empty text: no item | reboots: `Known`, `Observed`, reference `incident:<newest reboot id>`; events: the status `EvidenceSource` returned (`Known` or `Unknown`), `Observed`, reference `docker_events 24h` | reboot listing error: gap `recent incidents could not be read, so reboots are unknown`; **host not connected: an item with key `docker_events`, empty body, status `Unknown { reason: "the host's agent is not connected, so its container events of the last 24 hours were not read" }`** (the old code skipped silently); an `EvidenceSource` gap is passed on with `source = changes` |
| `similar_incidents` (`SimilarIncidentsSource`) | Experience | `Incident` | up to 3 resolved incidents of the **same host and check** (old `similar` filters, defence in depth kept), one item each, key = that incident's id, title `<check>: <title>`, body = the old line (`- <opened_at> <check>: <title> (<verdict>)`) | `Known`; origin `Historical` (forced anyway), reference `incident:<id>`, observed at its opening time | `IncidentRepo::search` error: gap `similar incidents could not be searched` |
| `skills` (`SkillsSource { skills, connected }`, not `TriageDeps`) | Capabilities | any scope with a host | one item, key `catalogue`, title `Skills`, body = the old `catalogue(&skills)` text; empty list: no item | `Known`; origin `Declared`, reference `skill manifests of host:<id>` | `connected == false`: an item with key `catalogue`, empty body, status `Unknown { reason: "the host's agent is not connected, so its skills are not known" }` |
| `incident_thread` (`ThreadSource`) | Incident | `Incident`, **follow-up only** | up to three items, each only when it has content: key `report` (title `Latest triage report`; the old `report_block` without its first heading line), key `actions` (title `Actions of this incident`; the old `actions_block` without its heading line, 20 actions), key `timeline` (title `Timeline of this incident, oldest first`; the old `timeline_block` without its heading line, newest 20 non-follow-up entries) | `Known`; `report`: origin `Inferred` (model-written), reference `triage report of incident:<id>`; `actions` and `timeline`: `Observed`, references `actions of incident:<id>`, `timeline of incident:<id>` | `IncidentRepo::timeline` error: gap `the incident timeline could not be read` (no `report`, no `timeline` item); `ActionRepo::list` error: gap `the incident's actions could not be read` |

**Which sources a purpose registers** (`triage/context.rs::sources`, in this order, **then every source of `TriageDeps.extra_context`** for all three purposes):

| Purpose | Built-in sources |
|---|---|
| `Investigate` | `incident`, `signals`, `profile`, `notes`, `changes`, `similar_incidents`, `skills` |
| `FollowUp` | `incident`, `signals`, `incident_thread`, `profile`, `notes`, `changes`, `similar_incidents`, `skills` |
| `Chat` (a host chosen) | `profile`, `skills` (the read-only skills only) |

`sources` takes `connected = matches!(deps.catalog.presence(host), Presence::Connected { .. })` and the `skills` slice its caller already has. The search text is `incident.check()` with `.` and `_` replaced by spaces, a space and the target when there is one (exactly the old notes query), trimmed, redacted, cut to 200 characters; empty means `None`.

### 6.7 Evidence dedupe in the tool loop **fixed**

```rust
// crates/domovoi-engine/src/triage/meta/ledger.rs
pub(crate) const MIN_LEDGER_BYTES: usize = 256;
/// What the model has already been shown in one conversation, by read.
pub(crate) struct ReadLedger { /* BTreeMap<ContextItemId, Seen { call_number: u32, taken_at: Timestamp, fingerprint: [u8; 32] }> */ }
impl ReadLedger {
    pub(crate) fn new() -> Self;
    /// Records a successful read and returns the note to show instead of `text` when an earlier read of the same
    /// call returned byte-identical text. `None` for the first read, for changed text and for short text.
    pub(crate) fn check(&mut self, id: &ContextItemId, text: &str, call_number: u32, now: Timestamp) -> Option<String>;
}
```
Rules: `text.len() < MIN_LEDGER_BYTES` gives `None` and records nothing. The fingerprint is the SHA-256 of `text` (`sha2` is already an engine dependency). An entry with the same id and fingerprint gives the note and is **not** refreshed (the note always points at the first read); a different fingerprint replaces the entry and gives `None`. The note text is **fixed**: `Same output as your earlier read (tool call <n>, taken <RFC 3339>); nothing changed, so it is not repeated.`, `<n>` being the 1-based number of the model's tool call in this attempt (the loop's `calls_used` when the first read was made) and the time the timestamp's `Display` form.

Where it is used: in the `RequestEvidence` arm of the meta-tool dispatch (prompt 31's `triage/meta/`), only after the read happened and `EvidenceSource::fetch` returned an item with status `Known`: the item's `id` and `body` go to `ledger.check`; on `Some(note)` the **model-facing result text** is the note (`is_error` false). Everything else is unchanged: the read happened, it counts against `max_tool_calls` and (in operation mode) the evidence budget, the `ToolUse` and the `EvidenceRecord` (built from the item, with the real text) are recorded as before, so the trace shows the read. `Unknown`, failed and refused reads are never recorded in the ledger and never replaced by a note (a retry may succeed). The ledger lives in the loop state beside `staged`: created fresh for every provider the chain tries and for every capability-rejection rebuild; never persisted. Follow-up and chat share the dispatch, so they get it too. `list_capabilities`, `describe_capability`, `propose_plan`, `ask_user` and `finish` are never deduped.

### 6.8 The one provider-chain loop **fixed** (behaviour), layout **convention**

```rust
// crates/domovoi-engine/src/triage/chain.rs
/// Why one provider's attempt failed.
pub(crate) enum AttemptError { Provider(LlmError), Invalid(String) }   // Display: the error text / the string
impl From<LlmError> for AttemptError { /* Provider */ }
/// What every attempt of one run shares: the budget and deadline, the recorded tool use, the evidence records.
pub(crate) struct ChainState { pub(crate) run: agent::RunBudget, pub(crate) used: Vec<ToolUse>, pub(crate) evidence: Vec<EvidenceRecord> }
impl ChainState { pub(crate) fn new(shared: &Shared) -> Self }          // RunBudget::for_run(shared), empty vectors
#[async_trait]
pub(crate) trait Attempt: Send + Sync {
    type Output: Send;
    /// One provider's whole try: rebuild the messages from the top and run them. Called again after a capability rebuild.
    async fn attempt(&self, provider: &Arc<dyn LlmProvider>, state: &mut ChainState) -> Result<Self::Output, AttemptError>;
}
/// Tries the primary, then each fallback, and returns the first success, or every provider's reason in order.
pub(crate) async fn run_chain<A: Attempt>(shared: &Shared, state: &mut ChainState, attempt: &A) -> Result<A::Output, Vec<String>>;
/// The messages of a conversation turn: the opening, the remembered turns, the gathered read output when the
/// provider has no tools, and the new question. Used by follow-up and chat.
pub(crate) fn thread_messages(opening: &str, history: &[LlmMessage], gathered: Option<&str>, question: &str) -> Vec<LlmMessage>;
```
`run_chain` is the body of today's `run::providers`, generalised:

```text
reasons = []
for (i, provider) in shared.chain().iter().enumerate():
    used_before = state.used.len(); evidence_before = state.evidence.len(); handled = []
    why = loop:
        match attempt.attempt(provider, state).await:
            Ok(out) -> return Ok(out)
            Err(Provider(CapabilityRejected { capability, .. })) if capability not in handled ->
                handled.push(capability); state.used.truncate(used_before); state.evidence.truncate(evidence_before)
                tracing::info!(provider = provider.id(), %capability, "the provider refused a capability; rebuilding the run without it")
            Err(e) -> break redact(format!("{} failed: {e}", provider_name(provider)))      # Provider or Invalid, same text form
    if i + 1 < chain.len(): tracing::info!(provider = provider.id(), reason = why, "the model falls back to the next provider")
    reasons.push(why)
Err(reasons)
```
The reasons are already redacted; callers join and wrap them: triage keeps `reasons.join("; ")` for `TriageOutcome::Failed`; follow-up and chat keep `provider_failure(redactor, &reasons)` (which redacts the joined text again, idempotently, and cuts it to 500 characters). The `Attempt` implementations:

| Implementation | Owns | `attempt` does | `Output` |
|---|---|---|---|
| `InvestigateAttempt<'a>` in `run.rs` | the incident, `skills`, the bundle, 31's `LoopScope` | exactly today's `run::attempt` as prompt 31 left it: `opening` (with the prefetch for a provider without tools), the investigation loop, the report parse and the one repair | what 31's `generate` needs (the report plus staged plan and dropped lines) |
| `FollowUpAttempt<'a>` in `followup.rs` | the opening text, the remembered turns, the question, the scope, the skills | `thread_messages(...)`, with `chat::host_output(...)` as `gathered` when `!provider.supports_tools()`; `agent::converse` on `ToolSet::Read` | the answer text |
| `ChatAttempt<'a>` in `chat.rs` | `host: Option<HostId>`, the opening, the remembered turns, the question, the skills | `thread_messages(...)` with `gathered` for a chosen host and a provider without tools; with a host and tools `agent::converse`, otherwise `agent::ask` with no tools (as today) | the answer text |

Each path creates its `ChainState` once (so the `RunBudget` is shared across its providers exactly as now), calls `run_chain`, and reads `state.used` / `state.evidence` afterwards as it reads `used` today (triage records the tool use; follow-up records it under `follow-up`; chat discards it). `provider_name` stays in `run.rs` (`pub(crate)`) and the chain imports it. No behaviour of any path changes: the request counts, budgets, fallback order, rebuild-once-per-capability rule, the failure texts and the stored tool-use entries are the same as before.

### 6.9 Contract for a Knowledge-layer source (prompt 34 implements it; nothing is built here) **fixed**

Prompt 34 adds exactly one source, `KnowledgeSource`, in `domovoi_engine::knowledge` (or `context::knowledge` if 34's layout prefers it), wrapping the `KnowledgeRead` port, and registers it by pushing it onto `TriageDeps.extra_context` where the hub builds `TriageDeps` (`crates/domovoi-hub/src/runtime/triage_build.rs`); it edits **no** aggregator, cap, layout or rendering code and adds no layer. Id `knowledge`, layer `Knowledge`; it supports `Overview` (the live facts and relations of the scope's host, at most 20, user-sourced and current first, then by predicate, newest first) and `Search { text }` (`KnowledgeRead::search_facts(text, Some(host), limit)`), never `Read`; it needs no other selector support. It maps each fact or relation to one `ContextItem` with id key `fact:<id>` or `relation:<id>`, a title naming the predicate or relation kind and the subject, a redacted body holding the value's canonical text (`max_item_bytes` bounded, `truncated` set when cut), and a status taken from `effective_status(now)`: `Current` is `Known`, `Stale` is `Stale { observed_at: last_verified_at or observed_at }`, `Disputed` is `Disputed { with: <the id of one other live fact of the same subject and predicate> }`; `Superseded` and `Retracted` facts are **never returned** and a retracted fact has no item at all; its provenance origin is by source (`User` and `Imported` are `Declared`, `Discovered` and `Integration` are `Observed`, `Inferred` is `Inferred`), reference `fact:<id>`, `observed_at` and the actor's display. A store error is a gap with a short fixed reason; no live fact is an empty batch (a known empty). It is read-only (it holds `KnowledgeRead` and a `Redactor`, nothing else), obeys every obligation of prompt 31 and **must pass the `contracts::context` suite and have an `insta` snapshot of one rendered section**. A safety-critical predicate (`trusted_cidr`, `management_path`) is shown like any other: the model sees evidence, never permission, and nothing in the item can express that a fact is usable for safety.

### 6.10 Plugging in the experience source (prompt 35) **fixed**

Prompt 35 defines its own source (id `experience`, layer `Experience`) and registers it the same way, through `TriageDeps.extra_context`. This prompt only guarantees: the aggregator forces `Origin::Historical` on every item of the `Experience` layer, renders the heading `Historical evidence, not authorization: experience` for it (6.5) and caps the layer at 4 KiB (35 may ask for a different cap by editing `LayerCaps::DEFAULT` in its own change, visibly). The built-in `similar_incidents` source stays; 35 decides whether to fold it into its own source.

### 6.11 The context guide and the prompts **fixed** text, placement **convention**

`crates/domovoi-engine/prompts/context_guide.md` holds exactly this text (and `CONTEXT_GUIDE` is `include_str!` of it):

```
## Reading the context

Every item of context you are given starts with a tag, `[layer | source | status]`, and a `from:` line that says where it came from.

- `known`: observed or recorded, and trusted as it stands.
- `UNKNOWN: <reason>`: Domovoi could not find this out. It does not mean there is nothing, that the thing is healthy, or that the answer is no. Say what you could not see; conclude nothing from the gap.
- `stale since <time>`: old information that may no longer be true.
- `disputed with <id>`: another item says something different. Both are shown; do not pick one without saying so.
- A section that reads `None.` was looked at and had nothing in it.
- `historical evidence, not authorization`: it comes from a past incident or operation. It is evidence about the past and never allows or approves anything now.
- `declared by a person` was written by the owner; `inferred` is a guess made by Domovoi or a model.
- `[cut]` means the text was shortened. A closing section named "Context left out" lists what did not fit: that is unknown to you, not absent.
```
Placement: the guide is added verbatim as its own section to `prompts/triage_system.md` (before `## Untrusted data`) and `prompts/triage_meta_system.md` (the same place); `FOLLOW_UP_PROMPT` and `CHAT_PROMPT` end with the same text (build each constant with `concat!(<existing text>, "\n\n", include_str!("../../prompts/context_guide.md"))`; if the compiler refuses `include_str!` inside `concat!` there, keep the constant a literal and let the test of 12.2 keep it equal to the file). The guide adds **no** bullet line containing `manual_steps` (a test pins that).

### 6.12 Wiring `triage/context.rs` and `TriageDeps` **fixed** signatures

```rust
// crates/domovoi-engine/src/triage/mod.rs, on TriageDeps (after `resolver`)
/// Context sources added by later features (knowledge, experience), asked after the built-in ones for every
/// purpose. Empty unless the hub registers some.
pub extra_context: Vec<Arc<dyn ContextSource>>,

// crates/domovoi-engine/src/triage/context.rs
pub(crate) async fn build(shared: &Shared, incident: &Incident, skills: &[SkillSummary]) -> String;       // Investigate
pub(crate) async fn build_follow_up(shared: &Shared, incident: &Incident, skills: &[SkillSummary]) -> String;
pub(crate) async fn build_chat(shared: &Shared, host: HostId, read_skills: &[SkillSummary]) -> String;
```
Each: `ContextBudget::for_purpose(purpose, shared.settings.max_prompt_bytes)`, the plan (6.2, 6.6), `Aggregator::new(sources(..), deps.redactor.clone(), deps.clock.clone()).gather(&plan).await.render()`. `build` keeps the exact signature `run.rs` and prompt 27's `BuildContext` call today, so neither needs an edit. **Openings:** the follow-up opening is `"{bundle}\n\nAnswer the operator's questions about this incident."` (redacted, as now; the old `incident_block` is gone because `incident_thread` carries it); the chat opening for a chosen host is the caller's overview (still cut to 8 KiB) followed by a blank line and `build_chat`'s text; for all hosts it is the overview alone. Both stay **one** message.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Open every file and symbol of section 3 and record moved lines and renamed types.
   - Confirm prompt 31 is merged: the port `ContextSource` and the item types (31 section 6.3), `EvidenceSource`, `FakeContextSource`, `domovoi_testkit::contracts::context`, `triage/meta/` (the loop, the dispatch, `LoopScope`, `EvidenceRecord`), `Generated`, `ToolSet`, `META_SYSTEM_PROMPT`, `TriageDeps.resolver`. **If any is missing, stop and report.** Use the merged names and list every difference.
   - Open prompt 27's `BuildContext` handler (`crates/domovoi-engine/src/operations/investigate/build_context.rs`) and confirm it calls `context::build(shared, incident, &skills)` and appends 31's resume section after it.
   - Check whether prompts 58, 36, 34 and 35 have merged and write down what that changes: with 58 merged `run::outcome` and the queue are gone (skip **(legacy path)** edits); with 36 merged `chat::answer_with` and the `AnswerSink` on `RunBudget` exist (keep them: adapt 6.8 to the function that runs the loop); 34 or 35 merged changes nothing here (6.9, 6.10).
   - Keep four lists: `grep -rn "TriageDeps {" crates` (expect five), `grep -rn "context::build\|incident_context\|context::catalogue" crates`, every test that scripts the same read call more than once (`grep -n "host_overview\|docker_ps" crates/domovoi-engine/tests/services/*.rs crates/domovoi-engine/tests/drills/*.rs`) and every test that asserts on `## ` headings or `None.` of a bundle.
   - Run `cargo xtask arch-check` and `cargo nextest run -p domovoi-engine` once for a green baseline. Record `ls crates/domovoi-store/migrations/` (no migration is added).
2. Create branch `layered-context` from the latest `overhaul-investigation`.
3. **The chain loop (behaviour-preserving).** Add `triage/chain.rs` (6.8), then move triage onto it (`run::providers` and `run::attempt` become `InvestigateAttempt` and a call to `run_chain`; `Failure` becomes `AttemptError`), run `cargo nextest run -p domovoi-engine` (nothing may change), then follow-up, run again, then chat, run again. Delete the three copies. Commit `refactor(engine): share one provider-chain loop between triage, follow-up and chat`.
4. **The aggregator.** `context/plan.rs`, `aggregate.rs`, `normalize.rs`, `shrink.rs`, `layout.rs`, `render.rs` and `guide.rs`; unit tests with `FakeContextSource` (12.1) and the golden snapshots. `cargo nextest run -p domovoi-engine --lib context` green. Commit `feat(engine): aggregate context items into capped layers`.
5. **The built-in sources.** Move the code of `triage/context.rs` and `triage/incident_context.rs` into `context/sources/` (6.6), turn every swallowed error into the gap or unknown item of the table, add the source tests and the contract suite runs (12.2). Commit `feat(engine): serve the triage context from sources that say what they could not read`.
6. **Wiring.** `TriageDeps.extra_context` and its five constructors; `triage/context.rs` (`build`, `build_follow_up`, `build_chat`, `sources`); the follow-up and chat openings (6.12); delete `incident_context.rs`, `host_opening` and `CATALOGUE_BYTES`; `prompts/context_guide.md`, the four prompts (6.11). Adapt the existing tests exactly as 12.6 lists. The engine, drill and hub test binaries green. Commit `feat(engine): build every model prompt from layered context`.
7. **The read ledger.** `triage/meta/ledger.rs` and its use in the `request_evidence` dispatch (6.7); adapt the two scripts 12.6 names. Commit `feat(engine): do not repeat identical read output to the model`.
8. **Remaining tests and gates** of section 12. Commit `test(engine): context layering, caps and chain gate tests`.
9. **Docs** (section 15) and `cargo xtask docs-links`.
10. **Checks and the completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-engine -p domovoi-hub -p domovoi-testkit --all-targets -- -D warnings`; `cargo nextest run -p domovoi-engine -p domovoi-hub -p domovoi-testkit`; `cargo nextest run -E 'rdeps(domovoi-engine)'` (a public field was added to `TriageDeps`); `cargo xtask arch-check`; `cargo xtask docs-links`. No `cargo xtask openapi`, no `cargo xtask docs-config` (no API or config change): say so.

## 8. Database and persistence

No migration and no schema change: `ls crates/domovoi-store/migrations/` is identical before and after. Nothing in this prompt is stored: the bundle text is built per call, travels (as before) in the `CallModel` effect payload of an investigation, and is not written anywhere else; the read ledger lives in memory for one attempt. **N-1 upgrade test: not applicable** (no schema change); the existing upgrade tests must still pass.

## 9. Protocol and compatibility

No wire change; `PROTOCOL_VERSION` stays 1.2. No HTTP route, DTO or OpenAPI change. The model-facing text changes: the context part of a prompt now carries status tags, section order follows the layers (the incident and its signals come first, then the host, what changed, similar incidents, the skills), and the four system prompts gain the guide of 6.11. Providers see no new field and no new tool. The `CallModel` effect payload still carries one redacted text, and its bound holds: an investigation's context is at most `(max_prompt_bytes / 4).clamp(8 KiB, 32 KiB) - 2 KiB`, plus the at most 2 KiB the resume section appends, so at most 32 KiB. A `CallModel` effect written by the previous version carries a bundle in the old format; the handler treats it as opaque text, so it is processed unchanged.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| A source cannot answer (store error, timeout, unsupported scope) | it returns a `ContextGap`; its section shows the `UNKNOWN: <reason>` line and `nothing could be read from this source.`; the run continues; the guide tells the model what that means. **Never `None.`** |
| A source asked and found nothing | no item and no gap: its section shows `None.` |
| A source does not answer by its deadline (5 s) | `ContextGap { reason: "timed out" }` from the aggregator's backstop; no item from that source and selector |
| A source returns an item whose layer or source id is not its own | the aggregator sets both to the source's and logs at `warn` (source id only) |
| An `Experience`-layer item whose origin is not `Historical` | forced to `Historical`; renders `historical evidence, not authorization` |
| A source returns more items than `limit`, or a body over `item_bytes` | the first `limit` kept, the rest counted in the omissions; the body cut on a character boundary with `truncated` set (renders `[cut]`) |
| A title, body, reference, actor or reason contains a registered secret | redacted before any cut; the whole rendered text is redacted once more |
| The same item arrives twice (same id and same status kind), for example from `Overview` and `Search` | one is kept (newest `observed_at`, first on a tie); no omission |
| A `Known` and an `Unknown` item share an id | both are rendered |
| The text is over a layer cap or the total cap | 6.4 steps A to C; every dropped item is counted in the closing section `Context left out to fit the size limit`; a disputed pair in one layer goes together; `Unknown` items and gaps are the last to go |
| The incident was deleted, or the incident repository fails | `incident` gap; the run still proceeds (a triage of a vanished incident is ended by prompt 27's handler, not here) |
| The host is not connected | `changes` shows the container-events item as `Unknown` and `skills` shows `Unknown`; the investigation still runs and its tool calls answer `Unavailable` (31) |
| The profile note is older than 8 days | the item is rendered `stale since <time>` |
| An attempt fails with a provider error or an invalid answer | the next provider is tried; the reason is `"<provider> failed: <text>"`, redacted |
| A provider refuses a capability not yet handled for it | `used` and `evidence` are truncated to their length before that attempt and the attempt runs again from the top, once per capability per provider |
| Every provider fails | `run_chain` returns every reason in order; triage joins them with `"; "` into `TriageOutcome::Failed`, follow-up and chat return `TriageError::Provider` through `provider_failure` (cut to 500 characters) |
| The model repeats a `request_evidence` call and the output is byte-identical and at least 256 bytes | the read happens and is counted and recorded; the model-facing text is the fixed note of 6.7 |
| The repeated read's output differs, is shorter than 256 bytes, or the earlier read was `Unknown`, `Failed` or `Refused` | the full text is returned; the ledger entry is replaced (differs) or absent |
| A provider without tool calling | prefetch exactly as before (not ledgered, nothing deduped); the opening is built by the same aggregator |
| `extra_context` is empty | only the built-in sources run |
| A source violates the contract (it panics, or writes) | not handled specially: a panic propagates as it would from any engine code, and reviewers must reject such a source; the contract suite and the gate scan are what catch it |

## 11. Security requirements

- **INV-1: nothing here authorizes anything.** Sources are read-only and hold ports for reading only. The new modules `src/context/` and `src/triage/chain.rs` never name `OperationHandle`, `OperationCommand`, `Authorization`, `HumanApproval`, `PolicyPermit`, `AgentLink`, `run_skill`, `run_tool`, `UnitOfWork`, `Transaction`, `propose_execution` or `.propose(` (the gate of 12.5 scans them; extend 31's gate, do not weaken it). `ChangesSource` reads the host only through `EvidenceSource`, which goes through the one evidence boundary. `HumanApproval::new` is not called anywhere new.
- **INV-9 and D-02: context is data, never permission.** A note, a signal, a similar incident, a fact or a skill description is shown as evidence under its tag. `ContextItem` has no field that can express permission (31's key-set test stays green), the aggregator forces `Origin::Historical` on the `Experience` layer, and `Historical` always renders `historical evidence, not authorization`. The guide tells the model to treat `UNKNOWN`, `stale` and `disputed` as limits on what it may conclude.
- **Unknown is never rendered as empty (fail-safe labelling).** Every read failure the old code swallowed is now a gap or an `Unknown` item; `None.` is only for a source that was asked and had nothing. Omission by a cap is named in the closing section, never silent.
- **INV-5: secrets.** Every `title`, `body`, `reference`, `actor` and `reason` is redacted by the aggregator **before** it is cut, and the rendered text is redacted once more; gap reasons too. `plan.search_text` is built by the engine from the incident's own check and target (never model text), redacted, and never logged unredacted. Logging carries source ids, layers and counts only, never item text. A `ListRedactor` secret appears in no request (12.3).
- **Scope and host isolation.** Items belong to the plan's scope. `similar_incidents` keeps its defence-in-depth host filter, `notes` and `profile` read only the incident's host, the Chat purpose uses `ContextScope::Host` and registers no incident source, and a source asked for a scope it does not serve answers a gap.
- **Bounds hold by construction.** The bundle is at most a quarter of `max_prompt_bytes` (clamped), so `fit_to_cap` never has to trim the first message; every item is bounded by `item_bytes`, every layer by its cap, every fetch by a 5 s deadline and `limit`. The aggregator trusts no source on bounds, redaction, layer or origin.
- **The ledger shows nothing new.** Its note carries only a call number and a time; the fingerprint stays in memory; nothing is persisted.
- **No new audit kind, no journal write, no effect, no migration.** Context gathering is reading.
- **Closed tool list unchanged.** This prompt adds no tool, so `model_meta_tools_are_closed` (31) is untouched.

## 12. Testing requirements

Names describe behaviour. Fakes that exist: `InMemoryStore` (with `with_delay`), `FakeClock`, `FakeAgentLink` (`set_presence`, `on_tool`, `set_skills`, `requests()`), `ScriptedLlm` (`push_ok`, `push_text`, `without_tools`, `requests()`; use its error scripting as `tests/services/capability.rs` and `reliability.rs` do), `ListRedactor`/`PatternRedactor`, `restart_skill()`, `calls(..)`, `seen(..)` (the `Debug` text of a request). From prompt 31: `FakeContextSource` (scripted batch per selector kind, records every `ContextQuery`, can answer a gap or sleep past the deadline on the fake clock) and `domovoi_testkit::contracts::context`. **Tests never sleep**: they advance `FakeClock`; deadline tests use `#[tokio::test(start_paused = true)]` as the `store_delay` tests do. Golden prompts use `insta` (read every snapshot before accepting); a snapshot of text that contains ids or times first replaces them with fixed placeholders (an `insta` filter on UUIDs and RFC 3339 times) so it is stable.

### 12.1 Aggregator, caps and rendering (unit tests in `crates/domovoi-engine/src/context/`, over `FakeContextSource`)

| Test | Setup, action, expected |
|---|---|
| `layers_render_in_layer_order_whatever_the_registration_order` | sources registered as Capabilities, Knowledge, Incident, Host: sections come out Incident, Host, Knowledge, Capabilities; sources of one layer keep registration order |
| `layered_context_render_snapshot` | insta: one section per status and origin (known observed, `UNKNOWN: <reason>`, `stale since`, `disputed with`, a truncated body with `[cut]`, a `Historical` item, a gap-only section, a `None.` section) and a closing omission section |
| `a_gap_renders_unknown_never_none` | source answers a gap and no item: its section has `UNKNOWN: <reason>` and `nothing could be read from this source.` and no `None.` |
| `a_source_with_nothing_renders_none` | no item, no gap: the section body is exactly `None.` |
| `a_source_that_supports_neither_selector_gets_no_section` | `supports` false for Overview and Search: no section, no query recorded |
| `overview_is_asked_first_and_search_only_with_search_text` | the recorded queries: Overview then Search with the plan's text; with no search text only Overview; limits 10 and 5; `max_item_bytes` 4096; deadline = now + 5 s |
| `overlapping_overview_and_search_answers_are_deduped` | both return an item with the same id and status: one is rendered |
| `dedupe_keeps_the_newest_and_the_first_on_a_tie` | same id and status kind, different `observed_at`: the newer; equal times: the earlier in answer order, at its position |
| `a_known_and_an_unknown_item_with_the_same_id_are_both_rendered` | both appear |
| `a_source_past_its_deadline_becomes_a_timed_out_gap` | the fake sleeps past the deadline: a gap with reason `timed out`; the next source is still asked |
| `an_item_is_restamped_to_its_source_and_layer` | the fake returns items naming another layer and source: they render under the source's own |
| `experience_items_are_forced_historical` | an `Experience` item with `Origin::Observed` renders `historical evidence, not authorization`; the section heading is `Historical evidence, not authorization: <id>` |
| `secrets_are_redacted_before_the_cut` | a `ListRedactor` secret straddles the `item_bytes` boundary of a body: the secret appears nowhere, in no title, reference, actor or reason either |
| `a_body_is_cut_on_a_character_boundary` | a body of multi-byte characters over `item_bytes`: valid UTF-8, `truncated` set, renders `[cut]` |
| `more_items_than_the_limit_are_omitted_and_counted` | 12 items for limit 10: 10 kept, the omission section says `2 items from <source>` |
| `a_layer_cap_drops_from_the_end_and_names_it` | a layer over its cap: the last items go, the closing section names layer, source and count |
| `an_item_alone_over_its_layer_cap_is_cut_not_dropped` | one big item: cut to fit, never below 256 bytes, still rendered |
| `the_total_cap_gives_up_experience_then_knowledge_first` | every layer full and the total too small: `Experience` items go first, then `Knowledge`, then `Evidence`, in `SHRINK_ORDER`; `Incident` last |
| `the_incident_item_is_never_dropped` | a total so small that everything else goes: the incident item is cut to its floor, not removed |
| `unknown_items_and_gaps_survive_until_the_last_step` | `Unknown` items outlive `Known` ones in the same layer; a gap section is never removed; step C drops `Unknown` items and lists them |
| `a_disputed_pair_is_never_split_by_a_cap` | two `Disputed` items pointing at each other in one layer: both stay or both go |
| `omission_section_text_is_fixed` | insta snapshot of the closing section with one and several items |
| `the_budget_follows_the_prompt_cap` | `for_purpose`: 131 072 gives 30 KiB, 40 KiB, 24 KiB (investigate, follow-up, chat); 32 768 gives 6, 8, 8 KiB; 4 MiB clamps to 30, 40, 24 KiB |
| `titles_cover_the_known_sources_and_label_the_fallbacks` | the table of 6.5; the `skills` title differs for chat; an `Experience` fallback carries the label |
| `rendering_is_deterministic` | the same plan and sources twice: byte-identical text |
| `an_empty_plan_renders_the_empty_string` | no sections, no omissions: `""` |

### 12.2 Built-in sources (unit tests beside each source, and `crates/domovoi-engine/tests/services/context.rs`, registered in `main.rs`, using `world(Setup)`)

| Test | Setup, action, expected |
|---|---|
| `each_built_in_source_passes_the_context_contract` | run `contracts::context` over every source of 6.6 with a scripted failure (a failing store stub, an absent host): bounds, redaction, unknown-not-empty, deadline and stable ids hold |
| `the_incident_source_gives_the_old_snapshot_lines` | one item whose body equals today's `snapshot` text; id `incident:<uuid>` is the same on two calls |
| `a_signals_read_failure_is_an_unknown_section_not_none` | `Setup.signals` set to a stub whose `list_for_incident` fails: the first request contains `UNKNOWN: the recent signals could not be read` under `## Recent signals, newest first` and not `None.` there |
| `no_signals_renders_none` | an incident with no signal: that section reads `None.` |
| `the_profile_item_turns_stale_after_eight_days` | a `host-profile` note: `known` at T0 + 7 days, `stale since <T0>` after advancing the clock past 8 days; the raw output behind it never appears (the three existing `profile.rs` scenarios stay green) |
| `a_missing_profile_note_renders_none` | no note: `None.` |
| `notes_are_declared_by_a_person_and_exclude_the_profile` | three runbook notes and a profile note: three items with origin text `declared by a person`; none is the profile |
| `a_disconnected_host_makes_container_events_and_skills_unknown` | presence removed: `changes` shows the item `UNKNOWN: the host's agent is not connected, so its container events of the last 24 hours were not read`, `skills` shows the unknown skills reason, and the run still ends in a report |
| `container_events_are_read_once_through_the_evidence_boundary` | connected host: the link sees exactly one `DockerEvents` read for the context, and the section shows `Docker events:` with the answer; a failing read shows `UNKNOWN` not `None.` |
| `reboots_are_listed_as_known` | a resolved `system.reboot` incident within 24 h: a `Reboots` item with the old line |
| `similar_incidents_are_historical_and_host_scoped` | the existing `similar_incidents_come_from_the_same_host_only` passes; the item renders `historical evidence, not authorization` |
| `skills_are_unknown_when_disconnected_and_none_when_connected_without_skills` | the two cases of 6.6 |
| `the_thread_source_carries_report_actions_and_timeline` | the follow-up context holds the report cause, `restart_container container=nginx status: proposed` and the timeline lines, each under its title, with no duplicated old headings; an action read failure shows `UNKNOWN: the incident's actions could not be read` |
| `the_search_text_is_the_old_notes_query` | `docker.container` and target `nginx` give `docker container nginx`; no target gives only the check words; it is redacted and at most 200 characters |

### 12.3 Prompts end to end (`tests/services/context.rs`; harness `world(Setup)`; add `Setup.extra_context` and `Setup.signals`)

| Test | Setup, action, expected |
|---|---|
| `the_investigation_prompt_is_layered_in_order` | the headings in the first request appear in the order Incident, Recent signals, Host profile, Host notes, What changed, Similar resolved incidents, Skills you may propose |
| `golden_investigation_prompt`, `golden_follow_up_prompt`, `golden_host_chat_prompt` | insta snapshots of the first message of a triage run, of a follow-up (report, an action, a note, a similar incident present) and of a chat about one host |
| `an_extra_source_is_asked_and_labelled` | `Setup.extra_context` holds a `FakeContextSource` in the `Knowledge` layer with a `Disputed` item pair and one in the `Experience` layer claiming `Observed`: both appear after the built-ins, the disputed pair is tagged, the experience one reads `historical evidence, not authorization`; a `Search` query reaches a source that supports it |
| `the_context_stays_inside_the_derived_cap` | `max_prompt_bytes = 32 KiB`, many large notes and signals: the investigation context is at most 6 KiB plus the closing section, the closing section names what went, and the request is not refused by `fit_to_cap` |
| `a_secret_in_a_note_a_signal_or_a_gap_never_reaches_the_request` | `SECRET` in a note body, a signal summary and a failing source's reason: absent from every request |
| `the_chat_context_for_a_chosen_host_reads_nothing_from_the_agent` | host chat (tools and no-tools providers): the only link requests are the model's own calls and the three existing prefetch reads |
| `chat_about_all_hosts_is_unchanged` | the opening is the overview alone; no tools |
| `the_openings_stay_one_message` | the follow-up and both chat shapes keep the text-part counts the existing tests assert |
| `every_system_prompt_carries_the_context_guide` | `SYSTEM_PROMPT`, `META_SYSTEM_PROMPT`, `FOLLOW_UP_PROMPT` and `CHAT_PROMPT` each contain `CONTEXT_GUIDE` verbatim; the existing `manual_steps` bullet test still passes |
| `an_investigation_context_fits_the_call_model_payload` | operation mode (31's harness `Mode::Operations`): the `CallModel` payload text is at most 32 KiB, also after a resume section is appended |

### 12.4 The chain loop (`tests/services/chain.rs`, registered in `main.rs`; unit tests in `chain.rs`)

| Test | Setup, action, expected |
|---|---|
| `the_chain_returns_the_first_success_and_asks_no_further_provider` | primary fails, secondary answers: the third is never asked (for each of triage, follow-up and chat) |
| `a_capability_rejection_rebuilds_once_per_capability` | the provider refuses `tools` once: the attempt runs again from the top with no tools, `used` and `evidence` are back to their length before the attempt; a second refusal of the same capability is an ordinary failure |
| `every_failure_reason_is_returned_in_order_and_redacted` | three providers fail with a secret in their text: the reasons are in chain order, each `<provider> failed: ...`, none holds the secret |
| `the_failure_texts_of_the_three_paths_are_unchanged` | triage: `TriageOutcome::Failed` with the reasons joined by `; `; follow-up and chat: `TriageError::Provider`, cut to 500 characters |
| `thread_messages_keeps_the_conversation_shape` | opening, remembered turns, the gathered output only when given, the question last |
| `the_existing_capability_and_fallback_tests_pass_unchanged` | `tests/services/capability.rs`, `reliability.rs` and `turns.rs` fallback scenarios, unedited |

### 12.5 Evidence dedupe and gates

| Test | Setup, action, expected |
|---|---|
| `a_first_read_is_never_a_note`, `short_output_is_never_deduped`, `changed_output_replaces_the_entry`, `unknown_reads_are_never_recorded` | unit tests of `ReadLedger::check` (256-byte threshold, SHA-256 equality, fixed note text with the first call number and time) |
| `a_repeated_identical_read_reaches_the_link_but_the_model_gets_a_note` | the same 8 KiB read twice: the link sees two requests; the second tool result is exactly the note of 6.7; the first was the full text |
| `a_repeated_read_still_counts_against_the_tool_call_budget` | with `max_tool_calls = 2`, two identical reads then a third: the third gets `NO_CALLS_LEFT` |
| `a_changed_read_is_returned_in_full` | the second answer differs: full text, entry replaced |
| `the_evidence_record_keeps_the_real_text` | operation mode: the second `EvidenceRequest` step's output is the real text, not the note |
| `follow_up_and_chat_dedupe_too` | the same scenario through `follow_up` and a host `chat` |
| `a_rebuilt_attempt_starts_with_an_empty_ledger` | a capability rejection between two identical reads: the repeat after the rebuild is returned in full |

Gates in `crates/domovoi-engine/tests/gates.rs` (extend; never weaken): **`context_modules_cannot_reach_the_writer_or_an_authorization`**: no line of `src/context/` or `src/triage/chain.rs` contains `OperationHandle`, `OperationCommand`, `Authorization`, `HumanApproval`, `PolicyPermit`, `AgentLink`, `run_skill`, `run_tool`, `UnitOfWork`, `Transaction`, `propose_execution` or `.propose(` (31's scan of `src/context/` already exists: add `chain.rs` and the new names to it instead of a second copy if it lets you); **`the_provider_chain_is_iterated_in_one_place`**: `shared.chain()` (and `.chain()` on `Shared`) is called only in `src/triage/chain.rs`, using `production_sources()`; **`context_text_is_built_only_by_the_aggregator`**: no file under `src/triage/` other than `context.rs` writes the headings `## Host notes`, `## Recent signals` or `## Skills you may propose`.

### 12.6 The complete list of edits to existing tests (nothing else may change; list each in the report)

1. `tests/services/harness.rs`: `Setup` gains `signals: Option<Arc<dyn SignalRepo>>` and `extra_context: Vec<Arc<dyn ContextSource>>`; both `TriageDeps` constructors (`:282`, `:340`) pass them (`signals` defaulting to the store, `extra_context` to empty).
2. The other three `TriageDeps` constructors (`drills/world.rs:319`, `hub/tests/hub/api_harness.rs:342`, `hub/src/runtime/triage_build.rs:88`) gain `extra_context: Vec::new()`.
3. `src/triage/chat.rs`: the unit test `a_huge_skill_catalogue_is_cut_before_the_opening_reaches_the_model` goes with `host_opening` and `CATALOGUE_BYTES`; its behaviour is covered by `a_huge_skill_catalogue_is_cut_by_the_item_cap_in_the_chat_context` (12.3: a 10 KiB catalogue in a chat context is cut at 4 KiB, shows `[cut]` and leaves the question intact).
4. `tests/services/reliability.rs`: `prompt_over_the_cap_is_trimmed_from_old_tool_results` and the ignored `measure_prompt_sizes`, and every other test the step 1 list found that scripts the same read with the same output of 256 bytes or more: make the link's answer differ on each call (a counter in the `on_tool` closure). **No assertion changes.**
5. Tests that assert the old section order or a `None.` for a failed read: change only the expectation that the new behaviour changes (order by layer; `UNKNOWN` instead of `None.`), and name each in the report. `tests/services/profile.rs`, `capability.rs`, `chat.rs`, `turns.rs` and `report.rs` are expected to pass unedited.
6. The follow-up prompt and chat prompt constants gain the guide: tests that compare the whole constant (if any) are updated to the new text. Prompt 31's prompt tests keep their assertions.
7. If prompt 36 has merged, its tests that assert the chat opening or the offered tools keep passing; if one counts text parts and fails, fix the count, never the behaviour, and report it.

## 13. Acceptance criteria

1. The context of an investigation, a follow-up and a chat about one host is produced by `Aggregator::gather` over `ContextSource` implementations and rendered by `LayeredContext::render`; sections come out in the layer order Incident, Host, Evidence, Experience, Knowledge, Capabilities, each item rendered with prompt 31's `ContextItem::render`, with the section titles of 6.5 (the headings of today's bundle are unchanged).
2. The seven parts of the old bundle and the follow-up thread block are the sources of 6.6 with the ids, layers, items, statuses and gap texts given there; `triage/incident_context.rs` and the old part functions are gone; `build` keeps its signature so prompt 27's handler and the legacy path need no edit.
3. A read failure that the old code printed as `None.` is now a gap or an `Unknown` item and renders `UNKNOWN: <reason>`; `None.` appears only for a source that was asked and found nothing; a disconnected host makes the container events and the skills `Unknown`.
4. The aggregator re-checks every item (layer, source id, `Historical` origin for the `Experience` layer, redaction before cutting, bounds, `limit`), applies the 5-second source deadline, removes duplicates by `(id, status kind)` keeping the newest, and keeps a `Known` and an `Unknown` item with the same id.
5. Per-layer caps and the total cap of 6.2 and 6.4 hold: the context is at most `(max_prompt_bytes / 4).clamp(8 KiB, purpose max)` (minus 2 KiB for an investigation); the shrink order is as specified; the incident item, `Unknown` items and gaps are the last to go; a disputed pair in one layer is not split; **every dropped item is named** in the closing section `Context left out to fit the size limit`.
6. The four system prompts carry `CONTEXT_GUIDE` verbatim; the by-hand-checklist bullet test still passes.
7. `TriageDeps.extra_context` exists and is the only registration point: a `FakeContextSource` pushed there is asked for every purpose and rendered in its layer, and a `Search` query reaches a source that supports it.
8. The contract a Knowledge-layer source must follow is written in 6.9 and the experience plug-in in 6.10; this prompt adds neither source.
9. The provider loop exists once (`run_chain`); triage, follow-up and chat run on it; `shared.chain()` is called nowhere else; request counts, budgets, fallback order, rebuild-once-per-capability, failure texts and recorded tool use are unchanged (the existing capability, reliability and turns tests pass unedited except as 12.6 lists).
10. A repeated `request_evidence` read whose output is byte-identical and at least 256 bytes reaches the link, counts against the budgets and is recorded with its real text, but the model reads the fixed note of 6.7; changed, short, `Unknown`, failed and refused reads are returned as before; the ledger is per attempt.
11. A chat about all hosts is unchanged (overview only, no tools); a chat about one host reads nothing from the agent while building its context; every opening is one message.
12. The golden prompt snapshots, the aggregator, source, chain and ledger tests of section 12 exist and pass; the gates of 12.5 pass.
13. No migration, no wire change, no new config key, no new dependency, no new metric, no HTTP or OpenAPI change; no hub module or command is added.
14. `cargo fmt --all`, `cargo clippy -p domovoi-engine -p domovoi-hub -p domovoi-testkit --all-targets -- -D warnings`, `cargo nextest run -p domovoi-engine -p domovoi-hub -p domovoi-testkit`, `cargo nextest run -E 'rdeps(domovoi-engine)'` and `cargo xtask arch-check` pass; every changed existing test is listed with its reason.

## 14. Migration and compatibility

- **Existing installs.** Nothing to migrate. After the upgrade a triage, a follow-up and a host chat build their context through the aggregator. Reports, cached reports, the repeat cache (its digest comes from the incident and its latest signal, not from the context), timeline entries and tool-use entries keep their shape.
- **Behaviour the owner will notice.** The text the model reads changes: sections come in layer order (the incident and its signals first), every item carries a status tag, a read that failed says `UNKNOWN` instead of `None.`, a disconnected host shows unknown container events and unknown skills, a prompt that does not fit lists what was left out, and a chat about one host also receives the host profile. Say so in the PR description and the book (section 15). Answers, cards and approvals are unaffected.
- **Both paths.** With `[orchestration] enabled = false` the legacy path calls the same `context::build` and the same chain loop; in operation mode prompt 27's `BuildContext` and `CallModel` use the same function, so the parity suite compares equal behaviour. The `CallModel` payload bound (32 KiB) holds (section 9).
- **Operations in flight.** A `CallModel` effect created before the upgrade carries an old-format bundle as opaque text and is processed unchanged.
- **Providers, agents, wire, store, API, retention, config.** No change.
- **Later prompts.** 34 and 35 add their sources by pushing onto `TriageDeps.extra_context` after this prompt merges (6.9, 6.10). If either has already merged without a context source (their briefs say so), nothing breaks: the knowledge or experience layer is simply empty until it adds one.
- **Rollback.** The previous binary reads everything: nothing new is stored.

## 15. Documentation requirements

- `ARCHITECTURE.md`: §5.5 (the `TriageRequest` bullet: replace "context bundle (recent signals, what changed, similar past incidents, relevant host notes)" with the layered context: items from `ContextSource`s in six layers, each with a status and a provenance; `ContextSource` itself is documented by prompt 31's §6 row: add a clause saying later sources register through `TriageDeps.extra_context`); §11 (a **Context** bullet: the aggregator, the layers, per-layer caps and the total cap derived from the prompt cap, unknown shown as unknown, omissions named; the **Fallback chain** bullet: one loop shared by triage, follow-up and chat; the **Redaction** bullet: also on every context item before it is cut). No new crate edge, no ADR (no arch rule changes). If `cargo xtask arch-check` reports a rule the new modules break, stop and report instead of editing a rule.
- `AGENTS.md`: no change (no command, rule or workflow changes). Say so in the PR.
- Book: `docs/book/src/concepts/incidents.md`, a short section after **What triggers triage**, "What the model is told": the incident and its signals, the host profile and notes, what changed on the host, similar past incidents (labelled as history, never as permission), the skills it may propose; each item says whether it is known, stale, unknown or disputed; "unknown" means Domovoi could not find it out, not that nothing is there; what does not fit is listed as left out. `docs/book/src/guide/llm-providers.md` **Prompt size and call deadlines**: add that the opening context is limited to a quarter of `prompt_bytes` (never below 8 KiB; at most 32 KiB for an investigation, 40 KiB for a follow-up, 24 KiB for a chat) so it is never trimmed away, and that older tool output is trimmed first as before. `docs/book/src/concepts/how-it-works.md` (the Chat paragraph): a host chat now also gets the host profile in its context. Run `cargo xtask docs-links`.
- No config reference regeneration (no config key), no OpenAPI regeneration (no API change); say so in the PR.
- Do not edit `docs/rework/`. Do not mention prompts, the build plan or coding agents in any committed file; in code and book say "context source" and "layer", not prompt numbers or decision ids.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
