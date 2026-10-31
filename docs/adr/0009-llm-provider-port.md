# 0009. LLM providers behind one port; CLI provider via injected executor

- Status: Accepted
- Date: 2026-10-31

## Context

Users bring very different models: hosted APIs, local OpenAI-compatible servers, or a coding-agent CLI they are already logged into. Triage logic must not care which.

## Decision

- `LlmProvider` in core exposes one model turn (messages and tool definitions in; text or tool calls and usage out).
- Adapters in `domovoi-llm`: Anthropic Messages API, OpenAI-compatible Chat Completions, and a CLI provider.
- The CLI provider runs a fixed argv template (for example a coding-agent CLI in print mode with tools disabled) through a `ProcessExecutor` injected by the `domovoi` binary. The hub crate itself never spawns processes (ADR 0004).
- The engine owns budgets, retries, fallback order and output validation; adapters only translate wire formats and report usage.

## Consequences

- Provider-specific quirks stay in adapters, tested with `wiremock` and recorded fixtures.
- The CLI provider cannot offer tool calling the same way; the engine runs it in single-shot mode with the context bundle inlined and no tools.

## Alternatives considered

- One OpenAI-compatible client for everything: loses Anthropic-native features and CLI users.
