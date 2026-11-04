# 11 · Feature: useful suggestions for hardware incidents ("AC unplugged")

Two independent prompts. Run 11a, then 11b.

## Background (verified, applies to both)

- "AC unplugged" is the `power.supply` check, target `ac`, critical and immediate
  (`crates/domovoi-checks/src/system/power.rs`). Related checks: `temperature.cpu`,
  `kernel.errors`, `smart.health`.
- Triage may only propose skills installed on the host
  (`crates/domovoi-engine/prompts/triage_system.md`: "If no listed skill fits, propose
  nothing"), and the starter skills (`examples/skills/`) have nothing for power or
  hardware. Most hardware fixes are physical, which no skill can do, so today the report
  has one `next_step` sentence and the Actions section says "No actions proposed".
- Triage can run any **read** skill itself through the `run_read_skill` tool; a skill's
  `applies_to` only limits *proposals*.

---

## 11a · Two read-only starter skills: `power_status` and `hardware_health`

### Read these files

- `examples/skills/system_overview/` and `examples/skills/journal_errors/` (copy their
  layout: `skill.toml`, executable `run`, `README.md`)
- `docs/book/src/guide/skills.md` (the starter skills table)

### Steps

1. Create `examples/skills/power_status/`:
   - `skill.toml`: `name = "power_status"`, description "Mains, battery and UPS state of
     this host.", `risk = "read"`, `entry = "run"`, `timeout = "15s"`,
     `applies_to = ["power.supply"]`, `max_output_bytes = 16384`, no params.
   - `run` (`#!/bin/sh`, `set -u`, mode 755, reads only): for each
     `/sys/class/power_supply/*`, print its name and the files `type online status
     capacity health energy_full energy_full_design charge_control_end_threshold` that
     exist; then, only if the command exists, `upower -d` and `upsc -l` followed by
     `upsc <each ups>` (NUT). Never fail when a tool is missing: print "not installed".
   - `README.md`: two or three lines like the other starters.
2. Create `examples/skills/hardware_health/` the same way:
   - description "Temperatures, recent kernel hardware errors and disk health summary.",
     `timeout = "30s"`,
     `applies_to = ["temperature.cpu", "kernel.errors", "power.supply", "smart.health"]`,
     `max_output_bytes = 65536`, no params.
   - `run`: `sensors` if installed, else each `/sys/class/thermal/thermal_zone*/{type,temp}`;
     `journalctl -k -p err --since -2h --no-pager --lines=100` if installed; EDAC
     counters from `/sys/devices/system/edac/mc/mc*/{ce_count,ue_count}` if present;
     `smartctl -H` for each `/dev/sd?` and `/dev/nvme?n1` if `smartctl` is installed.
     Label each block with `== <title>` like `system_overview/run`.
3. Update the table and the count ("seven" → "nine") in `docs/book/src/guide/skills.md`.
4. Run `cargo run -p domovoi -- check skills examples/skills` and fix every problem it
   reports (file modes, name matching the folder).

### Done when

- `cargo test -p domovoi --test cli` passes (it loads every starter skill).
- `domovoi check skills examples/skills` lists nine valid skills.

---

## 11b · A "manual steps" checklist in the triage report

### Goal

The report can carry up to five short steps a person does by hand (check the power
cable, reseat the disk, look at the UPS display). The incident page shows them as a
checklist under the next step, so hardware incidents always come with concrete advice.

### Read these files

- `crates/domovoi-core/src/triage/report.rs` (`TriageReport`, `validate`, the limits)
- `crates/domovoi-engine/prompts/triage_system.md`
- `crates/domovoi-engine/src/triage/report.rs` (`parse_report`, `redact_report`)
- `crates/domovoi-hub/src/api/dto.rs` (`ReportDto`)
- `web/src/features/incidents/IncidentDetailPage.tsx`

### Steps

1. Core: add to `TriageReport`
   ```rust
   /// Steps a person does by hand, such as checking a cable; at most [`MAX_MANUAL_STEPS`].
   #[serde(default)]
   pub manual_steps: Vec<String>,
   ```
   with `pub const MAX_MANUAL_STEPS: usize = 5;` and a per-step limit of 200 characters,
   both checked in `validate()` (field name `manual_steps`). Fix every place that builds
   a `TriageReport` literal (search the workspace for `TriageReport {`); add
   `manual_steps: Vec::new()`.
2. Engine: in `redact_report`, redact each manual step like the other text fields.
3. Prompt `triage_system.md`: add `manual_steps` to the JSON schema
   (`"type": "array", "maxItems": 5, "items": { "type": "string", "maxLength": 200 }`,
   description "Steps a person must do by hand, in order, such as checking a cable or a
   fan. Plain words, no commands for Domovoi.") and one rule under "What you can do":
   "When the fix needs a person at the machine (power, cables, fans, disks, UPS), list
   the steps in `manual_steps`, and propose a read skill such as `power_status` or
   `hardware_health` when it is installed."
4. API: add `manual_steps: Vec<String>` to `ReportDto` and map it. Regenerate OpenAPI.
5. Web: in `IncidentDetailPage.tsx`, when `report?.manual_steps.length`, render a
   section "Check by hand" with an ordered list.
6. Update snapshots: `INSTA_UPDATE=always cargo test -p domovoi-core` (the triage report
   JSON snapshot) and the hub OpenAPI snapshot.

### Tests

- Core: `validate()` rejects six steps and a 201-character step.
- Engine: `parse_report` accepts a report with `manual_steps` and one without
  (`#[serde(default)]`).
- Web: the "Check by hand" list renders.

### Done when

- Reports may include manual steps; old stored reports still load.
- All tests and snapshots pass.
