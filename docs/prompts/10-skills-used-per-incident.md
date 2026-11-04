# 10 · Feature: show which skills and tools were used on each incident

## Goal

The incident page has a **Skills and tools used** section listing every read tool and
read skill the model called (during triage and follow-ups, with how many times and how
many failed) and every skill proposed or run as an action.

## What is already known (verified)

- The model's tool calls are answered in
  `crates/domovoi-engine/src/triage/agent.rs`, `converse()`: for each
  `Part::ToolCall { id, name, input }` it calls `tools::call(shared, incident, skills, &name, input)`
  and gets `ToolResult { content, is_error }`. Nothing records which tools ran.
- Tool names are the `ReadTool` names (`docker_ps`, `docker_logs`, `journal`,
  `host_overview`, `run_read_skill`, ...). For `run_read_skill` the skill name is
  `input["skill"]`.
- `converse()` is called from `triage/run.rs` (`attempt`) and `triage/followup.rs`
  (`answer`).
- Timeline writes go through the incident manager. Free functions in
  `crates/domovoi-engine/src/manager/annotate.rs` build a `Decision`; see
  `follow_up()` and the `Command::RecordFollowUp` arm in `decide()`.
  **`decide.rs` is 492 lines: add at most 6 lines there.**
- Actions per incident are already in `IncidentDetail.actions` (skill, status).
- The detail API is `crates/domovoi-hub/src/api/incidents.rs`, `detail()`; DTOs in
  `api/dto.rs`; the page is `web/src/features/incidents/IncidentDetailPage.tsx`.

## Read these files

- `crates/domovoi-engine/src/triage/{agent,run,followup}.rs`
- `crates/domovoi-engine/src/manager/{command,handle,annotate,decide,actor}.rs`
- `crates/domovoi-hub/src/api/{incidents,dto}.rs`
- `web/src/features/incidents/IncidentDetailPage.tsx`

## Steps

1. **Engine type.** In `crates/domovoi-engine/src/triage/agent.rs` add
   ```rust
   /// One tool call the model made.
   #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
   pub struct ToolUse {
       /// The tool name, such as `journal` or `run_read_skill`.
       pub tool: String,
       /// For `run_read_skill`, the skill.
       pub skill: Option<String>,
       /// Whether the call failed.
       pub failed: bool,
   }
   ```
   and re-export it from `triage/mod.rs`.
2. `converse()` gets a new last parameter `used: &mut Vec<ToolUse>`; push one entry per
   tool call it answers (including the ones refused with `NO_CALLS_LEFT`, `failed: true`).
   Update both callers; each collects into a local `Vec` per run.
3. **Engine command.** Add
   `Command::RecordToolUse { incident: IncidentId, during: String /* "triage" or "follow-up" */, tools: Vec<ToolUse> }`
   and `ManagerHandle::record_tool_use(..)`. Add a free function in `annotate.rs`
   (like `follow_up()`) that writes one timeline entry, kind `"triage.tools"`, summary
   like `"triage used journal, docker_logs ×2"`, detail
   `json!({ "during": during, "tools": tools })`; publishes nothing. One arm in
   `decide()`; add the command to the "loads nothing" arm in `actor.rs`.
   `triage.tools` must not count as a triage outcome: check that
   `latest_triage()` in `api/incidents.rs` and `reload()` in `triage/queue.rs` still only
   look at `triage.report|cached|skipped|failed` (exclude `triage.tools` explicitly in
   `latest_triage`).
4. In `run.rs` (after the provider chain finishes, success or failure) and in
   `followup.rs` (after the answer), send `record_tool_use` when the list is not empty.
   Log a warning on error; never fail the run because of it.
5. **API.** Add to `IncidentDetail`:
   `pub tools_used: Vec<ToolUsageDto>` with
   `ToolUsageDto { name: String /* skill name for run_read_skill, else tool */, kind: String /* "tool" | "skill" */, calls: u32, failures: u32, last_used: String }`,
   built in `detail()` by folding every `triage.tools` timeline entry's detail. Regenerate
   OpenAPI.
6. **Web.** In `IncidentDetailPage.tsx`, after "Actions", add a section
   "Skills and tools used": a compact table (name, kind, calls, failures, last used),
   then "Skills proposed" from `actions` (skill, status). Show "Nothing used yet." when
   both are empty. Add a one-line hint: "Read tools and read skills only look; they never
   change the host."

## Tests

- Engine: a triage test where the scripted model calls `journal` then answers (see
  `crates/domovoi-engine/tests/services/turns.rs` for scripted tool calls) asserts a
  `triage.tools` timeline entry listing `journal`.
- Hub: `detail()` folds two entries into counts (unit test on the folding helper).
- Web: the section renders tools and proposed skills.

## Done when

- Every triage and follow-up run that called tools leaves a `triage.tools` entry, and the
  incident page lists tools and skills with counts.
- The latest triage state shown on the page is unchanged by the new entries.
- All tests pass; `decide.rs` ≤ 500 lines.
