# 0004. Only `domovoi-runner` spawns processes; argv only, never a shell

- Status: Accepted
- Date: 2026-10-31

## Context

Domovoi reads attacker-influenced text (log lines, container names, alert payloads) and feeds some of it to an LLM. Any path from that text to a shell command line is a remote code execution bug.

## Decision

- Only `domovoi-runner` may construct `std::process::Command` or `tokio::process::Command`. `clippy.toml` disallows the constructors; the runner module that needs them carries a scoped `#[allow]`, and `xtask arch-check` fails if that allow appears anywhere else.
- Commands are argv vectors. Parameters are validated against the manifest's `ParamSpec` and passed as separate argv elements and `DOMOVOI_PARAM_*` variables.
- No code invokes `sh -c`, `bash -c` or `/bin/sh`; `xtask arch-check` scans for these patterns.
- Other crates that need a process (command-backed checks, the LLM CLI provider) receive a `ProcessExecutor` port, implemented by the runner and injected by a composition root.

## Consequences

- One place to audit for execution: timeouts, process groups, user switching, environment scrubbing, output caps and redaction.
- Skill authors write scripts that read arguments; they cannot rely on shell expansion in manifests.

## Alternatives considered

- Allowing `Command` in any crate with review: does not scale with agent-written code.
- Shell with escaping: escaping bugs are a classic source of injection.
