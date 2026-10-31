# Review prompt

Run this in a **fresh** coding-agent session after a build-plan prompt is implemented, before merging. The reviewer must not have seen the implementation session. Paste everything below the line, replacing `PNN` and the branch name.

---

You are reviewing the pull request for build-plan prompt **PNN** on branch `pNN-...` of the Domovoi repository. You did not write this code. Your job is to find problems, not to praise. Do not modify any files.

Read, in this order:

1. `AGENTS.md` and `ARCHITECTURE.md` (all of it).
2. `docs/build-plan/prompts/PNN-*.md`.
3. The diff: `git diff origin/main...HEAD`.
4. Any file the diff touches, in full, where the diff alone is not enough context.

Then run:

```sh
cargo xtask ci
cargo xtask arch-check --base origin/main
```

Check each of the following and report every issue you find.

**Scope**
- Does the diff do everything in **In scope** and nothing in **Out of scope**?
- Does it touch crates or files not listed in the prompt? If so, is that justified in the PR description?

**Architecture**
- Do crate dependencies, module placement and data flow match `ARCHITECTURE.md` §4–§14?
- Is logic in the right layer: decisions as pure functions in core or engine, I/O in adapters?
- Are new types placed in the crate that owns the concept? Any duplicated concepts?
- Did any `#[allow(...)]`, new dependency, or change to `xtask/src/arch/rules.rs` slip in? Each needs a reason; rule changes need an ADR.

**Invariants** (ARCHITECTURE.md §15)
- For every invariant the prompt lists, is there a test that would fail if the invariant broke? Read the test; confirm it asserts the right thing.
- Look specifically for: any path where model output, chat text or webhook content reaches a process argv, an approval, or a config change; secrets reaching logs, prompts or responses; `HumanApproval` constructed outside `crates/domovoi-hub/src/auth/`.

**Tests**
- Is every item under **Tests required** covered by a named test that tests the behaviour, not just the happy path?
- Do tests use the fake clock instead of sleeping? Are snapshot changes intentional and readable?
- Are there obvious edge cases missing (empty input, limits, time zones, concurrency, reconnects)?

**Code quality**
- Errors: typed, informative, no `unwrap`/`expect` outside tests, no swallowed errors.
- Files and functions small and named for what they do. Public items documented meaningfully.
- No dead code, debug leftovers, commented-out blocks or TODOs without an issue reference.

**Operability and security**
- Inputs from outside (HTTP, WebSocket, webhooks, chat, files, model output) validated and size-limited?
- Timeouts on every network call and external process? Cancellation handled?
- Logs useful and free of secrets?

Report in this format:

```text
VERDICT: APPROVE | CHANGES REQUIRED

BLOCKING
1. <file:line> <problem> -> <concrete fix>

NON-BLOCKING
1. <file:line> <suggestion>

ACCEPTANCE CRITERIA
- <criterion>: met | not met | not verifiable (why)
```

A finding is blocking if it breaks a rule, an invariant, an acceptance criterion or a listed test, or would need to be undone by a later prompt.
