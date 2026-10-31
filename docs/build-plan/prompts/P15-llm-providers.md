# P15 — LLM providers

| | |
| --- | --- |
| Phase | 4, Intelligence |
| Depends on | P03 |
| Crates | `domovoi-llm` |
| Size | M |
| Invariants touched | INV-5 (API keys never logged), INV-2 (CLI provider uses the injected executor) |

## Goal

Implement the `LlmProvider` port for the Anthropic Messages API, OpenAI-compatible Chat Completions (covering Ollama, LM Studio, OpenRouter and others), and a CLI provider that runs a locally installed coding-agent CLI in print mode through an injected `ProcessExecutor`. Adapters translate formats and report usage; they contain no triage logic.

## Read first

- `ARCHITECTURE.md` §11
- ADR 0009, ADR 0004
- P03 `LlmProvider`, `ProcessExecutor`

## In scope

Add `reqwest` (workspace features), `rustls`, `serde`, `serde_json`, `thiserror`, `async-trait`, `tracing`, `secrecy`, `jiff`, `url`. Dev: `wiremock`, `tokio`, `domovoi-testkit`, `insta`.

Common rules:

- One shared `reqwest::Client` per provider with timeouts (connect 10 s, total = request budget from caller, default 120 s), `User-Agent: domovoi/<version>`, rustls only. Do **not** install a crypto provider in the library; the binary does that (P23). Tests install `rustls::crypto::ring::default_provider()` once via a `OnceLock` helper.
- API keys are `SecretString`; exposed only when building the header. Never in `Debug`, errors or tracing fields.
- Map HTTP failures to `LlmError`: 401/403 → `Auth`; 429 → `RateLimited { retry_after }` from the `retry-after` header (seconds or HTTP date); 408/5xx/529/connection errors → `Unavailable`; malformed body → `InvalidResponse` with a short, redacted excerpt.
- Cost: if a price table is configured, compute `cost_micros` from token usage; otherwise `None`. Never hard-code prices.

### Anthropic (`anthropic.rs`)

- `POST {base_url}/v1/messages` with `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`.
- Map `LlmRequest`: `system` → top-level `system`; messages with text, `tool_use` and `tool_result` content blocks; tools → `{ name, description, input_schema }`; `max_tokens`.
- Map responses: `text` and `tool_use` blocks → `Part`s; `stop_reason` (`end_turn`, `tool_use`, `max_tokens`); `usage.input_tokens`/`output_tokens`.

### OpenAI-compatible (`openai.rs`)

- `POST {base_url}/chat/completions` with `Authorization: Bearer` when a key is configured (local servers may need none).
- Tools as `{"type":"function","function":{name, description, parameters}}`; assistant `tool_calls` with JSON-string `arguments` parsed into `Value` (invalid JSON → `InvalidResponse`); tool results as `role: "tool"` messages with `tool_call_id`.
- `finish_reason` mapping (`stop`, `tool_calls`, `length`); `usage.prompt_tokens`/`completion_tokens` when present.

### CLI provider (`cli.rs`)

- Constructed with `Arc<dyn ProcessExecutor>` and a `CliPreset { program: String (absolute path), args: Vec<String> (template), prompt_via: Stdin | Arg, output: Json { field } | Text }`.
- Built-in presets `claude` and `codex` are **starting points**: the installed CLI's `--help` decides the right flags. Document in rustdoc and the book that users must verify flags for their CLI version and can override `args` in config. Defaults must request non-interactive print mode, a single turn, no tool use and machine-readable output where the CLI supports it.
- `supports_tools() == false`. The engine sends one prompt (system + context + question) and expects the final JSON report as text.
- Non-zero exit or timeout → `Unavailable` with the first 300 redacted characters of stderr. Usage tokens unknown → `None` costs.

### Factory (`factory.rs`)

- `fn build(config: &LlmProviderConfig, executor: Option<Arc<dyn ProcessExecutor>>) -> Result<Arc<dyn LlmProvider>, LlmBuildError>` (CLI kind without an executor is an error).

## Out of scope

- Retries, fallback order, budgets, prompt content and output parsing: all engine (P16).
- Streaming.

## Tests required

wiremock for both HTTP adapters, `FakeProcessExecutor` for the CLI:

1. `anthropic_text_response_maps_to_parts_and_usage`.
2. `anthropic_tool_use_round_trip_request_shape` (insta snapshot of the outgoing JSON for a request containing a prior tool call and tool result).
3. `openai_tool_calls_parse_arguments_and_reject_invalid_json`.
4. `openai_works_without_api_key`.
5. `rate_limit_maps_retry_after_seconds_and_http_date`.
6. `auth_and_server_errors_map_to_variants`.
7. `api_key_never_appears_in_debug_or_errors` (format the provider, the error and captured tracing output).
8. `cli_preset_builds_argv_and_passes_prompt_via_stdin`; `cli_json_output_extracts_result_field`; `cli_failure_reports_redacted_stderr`.
9. `cost_is_computed_only_with_configured_prices`.

## Architecture gates

- `domovoi-llm` depends only on `domovoi-core` internally (R1). No `std::process` (R3); the CLI provider only uses the injected executor.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/guide/llm-providers.md`: config examples for Anthropic, OpenAI, Ollama and the CLI presets, with the warning to verify CLI flags.

## Deliverables

- `crates/domovoi-llm/src/{anthropic,openai,cli,factory,http,error}.rs`
- Tests and snapshots; `docs/book/src/guide/llm-providers.md`

## PR

Title: `feat(llm): Anthropic, OpenAI-compatible and CLI providers`
