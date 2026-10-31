## What and why

<!-- One or two sentences. Link the issue or build-plan prompt (e.g. P07). -->

## How it was verified

<!-- Tests added or changed, manual checks. For build-plan prompts, list each acceptance criterion and how it is met. -->

## Checklist

- [ ] Title follows Conventional Commits (`feat(scope): ...`, `fix(scope): ...`)
- [ ] `cargo xtask ci` passes locally (`--web` if `web/` changed)
- [ ] Tests cover the change; no gate test removed or weakened
- [ ] No new `#[allow(...)]`, no edited migrations, no hand-edited generated files
- [ ] ADR added if a rule, boundary, wire format or invariant changed
- [ ] Docs updated if user-visible behaviour changed
- [ ] For build-plan prompts: review prompt run in a fresh session; blocking findings resolved
