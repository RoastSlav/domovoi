# 39 · Refactor: split the files that are at the 500-line limit

No behaviour change. Do it before the prompts that add code to these files (33, 34, 26).

## Background (verified on `main`, non-test lines)

| File | Lines | Split along |
| --- | --- | --- |
| `crates/domovoi-engine/src/manager/decide.rs` | 500 | one module per command family: signal folding, human commands (ack, mute, unmute, dismiss), timers (TTL, mute expiry), triage attach |
| `crates/domovoi-hub/src/api/dto.rs` | 490 | `dto/incident.rs`, `dto/action.rs`, `dto/host.rs`, re-exported from `dto/mod.rs` |
| `crates/domovoi-hub/src/api/settings_write.rs` | 486 | settings save and schema vs host checks and skills (`api/host_settings.rs`) |
| `crates/domovoi-hub/src/runtime/mod.rs` | 483 | move adapter and service wiring out of `HubRuntime::start` into `runtime/wire.rs` |
| `crates/domovoi-hub/src/api/incidents.rs` | 474 | list and detail vs commands (ack, mute, dismiss, ask, triage) |
| `crates/domovoi-hub/src/settings/service.rs` | 472 | save and apply vs host overrides |
| `crates/domovoi-config/src/hub/mod.rs` | 467 | one module per section group |
| `crates/domovoi-hub/src/selftest/mod.rs` | 460 | steps vs report |
| `crates/domovoi-engine/src/actions/executor.rs` | 454 | leave if 25b already shrank it; else verification into `actions/verify.rs` |

`crates/domovoi-hub/src/runtime/tests.rs` (768) and `examples/dev_hub.rs` (507) are test
or example code and exempt; leave them.

## Steps

1. For each file, move whole functions into new sibling modules; keep public paths the
   same with `pub use` so no caller changes. Each new module starts with a `//!` line
   saying what it holds.
2. Target at most about 350 lines per resulting file so the next feature fits.
3. One commit per file (`refactor(engine): split decide.rs by command`, ...).
4. Run `cargo xtask arch-check` after adding modules.

## Tests

- No test changes except imports. `cargo nextest run -p domovoi-engine -p domovoi-hub -p domovoi-config`
  passes.

## Done when

- No non-test `.rs` file in the workspace is over 420 lines.
