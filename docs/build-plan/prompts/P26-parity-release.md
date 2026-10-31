# P26 — Parity verification and the v0.1.0 release

| | |
| --- | --- |
| Phase | 6, Ship |
| Depends on | P25 |
| Crates | all (fixes only) |
| Size | M |
| Invariants touched | all (final verification) |

## Goal

Prove v0.1 meets its bar: it replaces a homegrown cron-and-script monitor on two machines without losing anything. Verify parity, performance budgets and security invariants, fix the gaps in small tested commits, and cut v0.1.0.

## Read first

- `docs/build-plan/README.md` (the v0.1 bar), `ARCHITECTURE.md` §15, §18
- The parity table below

## In scope

### Parity checklist

Turn each row into an automated test where possible (fixture-driven scenario through agent → hub → recording gateway) and otherwise a manual check recorded in `docs/build-plan/v0.1-verification.md` with date, machine and result.

| Reference behaviour | Domovoi must |
| --- | --- |
| Checks every 5 min: containers, units, HTTP and MQTT/TCP probes, disk, RAM and swap, temperature, reboots, kernel errors, SSH/sudo, new ports, power; hourly SMART and pending reboot | run the same checks on the same cadences |
| Expected containers with per-container criticality | learn baselines from managed containers only; criticality configurable |
| A problem must show twice before alerting; power and hourly checks immediate | confirmation runs plus immediate class |
| One message per issue, 24 h reminders for warnings, a Resolved message | one card + thread, reminders, resolution reply |
| Quiet hours 00:00–09:00 with held alerts released at 09:00 | quiet hours with one release message |
| Daily digest after 09:00 | one digest for all hosts, quiet hosts collapsed, trends |
| Accepted-as-normal conditions | ack rules and delta counters |
| Read-only AI triage with command allowlist, redaction, 6 runs/hour, 4-minute timeout, fallback when the LLM is unavailable | read-only tool loop, redaction, caps, fallback with degraded notice |
| Each machine notices the other one dying | off-box hub: host down vs agent down vs hub network down |
| `--dry-run`, `--test`, `--selftest`, `--digest` | `check run`, `notify test`, `selftest`, `digest now` |
| Graphs stay in Grafana/Prometheus | `/metrics` exposed; no graphing in Domovoi |

### Budgets

Measure on a real host and record in the verification file: hub idle RSS (≤ 80 MB with UI), agent idle RSS (≤ 20 MB), CPU for one check cycle (≤ 2 s per 5 min), triage latency p50 and p95 for the configured provider.

### Security pass

- Run the review prompt over the whole repository focused only on §15 invariants; fix findings.
- `cargo deny check`, OpenSSF Scorecard results reviewed; document accepted findings.
- Verify a release candidate end to end: download from a draft release, verify the checksum and `gh attestation verify`, run the installer on a clean VM, enroll an agent from a second VM, trigger a real incident, approve a write skill from Discord, see verification and the audit log entry.

### Release

- Merge the release-plz PR for `0.1.0` once everything above is green. Confirm the release workflow attached both archives, checksums, `install.sh`, provenance attestations, and pushed both images with `0.1.0` and `latest` tags.
- Publish release notes: what it does, how to install, safety model, known limitations, what is next (v0.2 list from the build plan).

## Out of scope

- New features. Anything not needed for parity, budgets or invariants goes into an issue for v0.2.

## Tests required

- Automated parity scenarios under `crates/domovoi/tests/parity/` (one test per automatable row).
- All existing gates and drills green.

## Acceptance criteria

- `docs/build-plan/v0.1-verification.md` complete with every row marked pass.
- v0.1.0 published with verified artifacts.

## Deliverables

- Parity tests, fixes, `docs/build-plan/v0.1-verification.md`, release notes

## PR

Title: `chore: v0.1 parity verification` (fixes in separate `fix(...)` PRs as needed), then the release-plz PR.
