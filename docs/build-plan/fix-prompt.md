# Fix prompt

Use this in the implementation session (or a new one on the same branch) after a review returns **CHANGES REQUIRED**. Paste everything below the line and append the reviewer's report.

---

A reviewer found problems in the pull request for build-plan prompt **PNN** on this branch. Their report is below.

1. Read `AGENTS.md`, `ARCHITECTURE.md` and `docs/build-plan/prompts/PNN-*.md` again.
2. For each **BLOCKING** item: fix it, and add or strengthen a test that would have caught it. If you disagree with a finding, do not change the code; explain why in your reply with references to the architecture or prompt.
3. Apply **NON-BLOCKING** items only if they are small and inside the prompt's scope. List the ones you skipped.
4. Do not refactor unrelated code. Do not widen scope.
5. Run `cargo xtask ci` (plus `--web` if you touched `web/`) until it passes.
6. Reply with a table: finding number, what you changed, which test covers it.

Reviewer report:

<paste here>
