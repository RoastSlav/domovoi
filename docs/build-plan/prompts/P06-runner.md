# P06 — Runner: skill catalogue and sandboxed execution

| | |
| --- | --- |
| Phase | 2, Agent side |
| Depends on | P03 |
| Crates | `domovoi-runner` |
| Size | L |
| Invariants touched | INV-2 (only spawner, argv only, no shell), INV-5 (output redaction) |

## Goal

Build the only component that starts processes: a skill catalogue loaded from disk, argument validation and argv construction, and a `ProcessExecutor` that runs programs with a cleared environment, optional user switch, a timeout that kills the whole process group, capped output and redaction.

## Read first

- `ARCHITECTURE.md` §12, R3, R4, INV-2, INV-5
- ADR 0004
- P02 (`SkillManifest`, `validate_args`, `ValidatedArgs`), P03 (`ProcessExecutor`, `Redactor`)

## In scope

Add `tokio` (`process`, `io-util`, `time`, `fs`, `rt`, `macros`), `nix` (workspace features), `toml`, `serde`, `thiserror`, `async-trait`, `tracing`, `sha2`, `hex`. Dev: `tempfile`, `proptest`, `domovoi-testkit`.

### `exec/` (the only module allowed to use `tokio::process::Command`)

- `SandboxExecutor { redactor: Arc<dyn Redactor>, base_env: BTreeMap }` implementing `ProcessExecutor`.
- Put `#[allow(clippy::disallowed_methods)]` on the single function that calls `Command::new`, with a comment citing ADR 0004. Nowhere else.
- Behaviour, in order:
  1. Refuse if `program` is not absolute, does not exist, is not a regular executable file, or its file name is a shell (`sh`, `bash`, `dash`, `zsh`, `fish`, `ash`, `ksh`, `busybox`) → `ProcessError::ShellRefused` / `NotFound` / `NotExecutable`.
  2. `env_clear()`, then set `PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`, `LANG=C.UTF-8`, `HOME=/nonexistent`, plus `spec.env` (only keys matching `^[A-Z][A-Z0-9_]*$`).
  3. `cwd` if given, else `/`.
  4. `process_group(0)` so the child leads its own group.
  5. If `run_as` is set: resolve the user via `nix::unistd::User::from_name`; if the agent is not root and the user differs from the current user → `ProcessError::UserSwitch("agent must run as root to switch users")`; otherwise `.uid().gid()` and clear supplementary groups via `nix` before exec is **not** possible without unsafe; document that supplementary groups are inherited and require the systemd unit to drop them (P24).
  6. stdin: write `spec.stdin` if present then close; otherwise `Stdio::null()`.
  7. Read stdout and stderr concurrently into buffers capped at `max_output_bytes` each; when a cap is hit, set `truncated`, stop buffering and kill the process group with `SIGKILL`.
  8. On timeout: `SIGTERM` the group, wait 2 s, then `SIGKILL`; set `timed_out`.
  9. Decode output as lossy UTF-8, then run both streams through the `Redactor`.
  10. Return exit code or terminating signal and duration.

### `catalog/`

- `SkillCatalog::load(dir: &Path) -> (SkillCatalog, Vec<CatalogIssue>)`: for each `dir/<name>/skill.toml`, parse into `SkillManifest`, check folder name equals `name`, call `validate()`, resolve `entry` with `canonicalize` and require it stays inside the skill folder (no symlink escape), is a regular file, executable, and not group- or world-writable; the skill folder must not be world-writable. Invalid skills are excluded and reported; valid ones are kept.
- `catalog.get(name) -> Option<&LoadedSkill>`, `catalog.summaries() -> Vec<SkillSummary>`, `catalog.reload()`.
- `LoadedSkill { manifest, manifest_hash, entry_path, dir }`.

### `skill_run.rs`

- `run_skill(&self, skill: &LoadedSkill, args: &ValidatedArgs, executor: &dyn ProcessExecutor) -> Result<SkillOutput, RunError>`.
- argv = `[entry_path]` followed by each param value as a separate argument in declared order. Environment adds `DOMOVOI_PARAM_<UPPER_NAME>=<value>` for each param, `DOMOVOI_SKILL=<name>`.
- `argv_hash` = SHA-256 of the argv joined with `\0`, returned for the audit log.
- Timeout and output cap from the manifest; `run_as` from the manifest.

### Not here: command templates

The fixed argv templates for `journalctl`, `smartctl`, `systemctl` and `fail2ban-client` belong to `domovoi-checks` (P07), because checks may not depend on the runner (R1). They produce `ProcessSpec` values that any `ProcessExecutor` (this crate's in production, the fake in tests) runs.

## Out of scope

- Agent request handling and policy (P09). Revert/commit-confirm (v0.3).
- Container sandboxing (namespaces, seccomp): document as future work.

## Tests required

Put tests that contain shell names or injection strings in `crates/domovoi-runner/tests/` (source scans exempt only test paths). Use real binaries that exist on CI runners (`/usr/bin/printf`, `/usr/bin/env`, `/bin/sleep`, `/usr/bin/yes`, `/bin/true`, `/bin/false`); skip gracefully if missing.

1. `arguments_with_shell_metacharacters_arrive_verbatim`: run `/usr/bin/printf '%s\n'` with args `"; rm -rf /"`, `"$(id)"`, `"a b"`; stdout lines equal the inputs exactly.
2. `environment_is_cleared`: `/usr/bin/env` output contains only the allowlisted keys plus `DOMOVOI_*`.
3. `timeout_kills_the_process_group`: a skill whose entry spawns a child `sleep 30` (use a tiny compiled test helper or `/bin/sleep` directly) with a 300 ms timeout returns `timed_out` within 3 s and no process from that group survives (check `/proc`).
4. `output_cap_truncates_and_stops`: `/usr/bin/yes` with a 4 KiB cap returns `truncated` and terminates.
5. `shells_are_refused`: `/bin/sh`, `/bin/bash`, `/usr/bin/env` named `sh` via symlink → `ShellRefused`.
6. `relative_program_is_refused`.
7. `output_is_redacted`: a configured secret printed by the program comes back as `[REDACTED]`.
8. Catalogue: `entry_escaping_skill_dir_is_rejected` (`../x` and a symlink to `/bin/true`), `world_writable_entry_is_rejected`, `folder_name_mismatch_is_rejected`, `invalid_manifest_is_reported_and_skipped`, `valid_skills_load_with_hashes`.
9. `argv_hash_is_stable_and_changes_with_args`.
10. `run_as_other_user_without_root_fails_clearly` (skipped when running as root).
11. `crates/domovoi-runner/tests/gates.rs`: `argv_never_contains_shell_invocation` — proptest over manifests and valid args: argv[0] is the entry path inside the skill dir and argv length is params + 1.

## Architecture gates

- The only `allow(clippy::disallowed_methods)` in the workspace is in `crates/domovoi-runner/src/exec/`.
- `cargo xtask arch-check` passes (shell scan, process scan).

## Acceptance criteria

- `cargo xtask ci` passes on Linux. Tests needing Linux-only behaviour are `#[cfg(target_os = "linux")]`.
- Rustdoc on `SandboxExecutor` lists exactly what isolation it provides and what it does not.

## Deliverables

- `crates/domovoi-runner/src/{exec/*,catalog/*,skill_run,error}.rs`
- `crates/domovoi-runner/tests/*` with fixture skill folders under `tests/fixtures/skills/`

## PR

Title: `feat(runner): skill catalogue and sandboxed process executor`
