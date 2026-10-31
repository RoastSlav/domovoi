## What and why

<!-- One or two sentences. Link the issue. -->

## How it was verified

<!-- Tests added or changed, manual checks. -->

## Checklist

- [ ] Title follows Conventional Commits (`feat(scope): ...`, `fix(scope): ...`)
- [ ] `cargo xtask ci` passes locally (`--web` if `web/` changed)
- [ ] Tests cover the change; no gate test removed or weakened
- [ ] No new `#[allow(...)]`, no edited migrations, no hand-edited generated files
- [ ] ADR added if a rule, boundary, wire format or invariant changed
- [ ] Docs updated if user-visible behaviour changed
