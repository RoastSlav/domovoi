# LLM providers

Triage needs a language model. Domovoi talks to it through one of three adapters:

- `anthropic`: the Anthropic Messages API.
- `openai_compatible`: any server with an OpenAI-style Chat Completions endpoint, such as OpenAI, OpenRouter, Ollama, LM Studio or vLLM.
- `cli`: a coding-agent CLI already installed and logged in on the hub machine, run in print mode.

Configure one provider as `[llm.primary]` and, optionally, a fallback as `[llm.secondary]` in `hub.toml`. Keep API keys in an environment variable or a file, never inline.

In the web setup and **Settings → Models and triage**, choose the provider first. API providers show the model, base URL and API key fields; CLI providers show only the CLI options. **Fetch models** asks the provider for model IDs using the values and key currently in the form without saving them. You can also enter a model ID manually, which is useful when a compatible server has no model-list endpoint.

## Fallback models

An API provider can list up to five of its other models in `fallback_models`. When its
main model fails, the same request is retried on those models, in order, with the same
base URL and key:

```toml
[llm.primary]
kind = "openai_compatible"
base_url = "https://api.openai.com/v1"
model = "gpt-5"
fallback_models = ["gpt-5-mini", "gpt-4.1-mini"]
```

Fallback models work only with the `anthropic` and `openai_compatible` kinds; a `cli`
provider always runs the same program. In the web UI they are the **Fallback models**
rows under **Settings → Models and triage** and **Settings → Fallback model**, entered
comma-separated.

Triage tries every model of a provider before moving to the other provider: the primary
model, its fallback models, then the secondary model and its fallback models. The first
provider that answers wins.

## Fallback provider

The **Settings → Fallback model** section, or `[llm.secondary]` in `hub.toml`, sets up a second, optional provider:

```toml
[llm.secondary]
kind = "openai_compatible"
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"
```

Triage, follow-up questions and the general chat try the fallback whenever the main provider fails for any reason: no connection, rate limited, refused credentials, an answer Domovoi cannot read, or an answer that is not usable. When every provider fails, the failure says why each one did.

## Anthropic

```toml
[llm.primary]
kind = "anthropic"
model = "claude-sonnet-5-5"            # any model your key can use
api_key = { env = "ANTHROPIC_API_KEY" }
# base_url = "https://api.anthropic.com"   # optional, for a proxy or gateway
```

## OpenAI and OpenAI-compatible servers

`base_url` is the API root that `/chat/completions` is appended to. It usually ends in `/v1`.

```toml
[llm.primary]
kind = "openai_compatible"
base_url = "https://api.openai.com/v1"
model = "gpt-5"
api_key = { env = "OPENAI_API_KEY" }
```

### Ollama

Local servers usually need no key; leave `api_key` out and no `Authorization` header is sent.

```toml
[llm.primary]
kind = "openai_compatible"
base_url = "http://localhost:11434/v1"
model = "qwen3:14b"
```

Choose a model that supports tool calling. Triage works with tools; without them, answers are much less specific.

### Capability fallback

Some OpenAI-compatible servers accept only a subset of the Chat Completions request. Domovoi handles that without extra configuration:

- If a server refuses the optional `stream_options` field (streamed usage totals), Domovoi asks for a whole answer instead. The answer still arrives, with the usage totals the monthly budget needs.
- If it refuses `tools`, Domovoi stops offering them and rebuilds the run: it puts the host's read output in front of the model and tells it to answer from that output. Incident triage, follow-up questions and the general chat all do this.
- A refusal is handled once per capability, so a server cannot cause a retry loop. If the rebuilt answer cannot be produced, the provider fails and the chain moves on as usual.

Only a refusal of an optional field the request actually sent is treated this way. An invalid model, a context overflow or a malformed tool schema is an ordinary provider failure.

### Usage, estimates and reasoning

Provider-reported input and output tokens are used as they are. When a provider reports only part of its usage, Domovoi keeps the reported half and estimates the other from the request and answer content, including tool calls. When it reports nothing, both halves are estimated. An explicit reported zero stays a real zero.

An estimate is not a tokenizer count from the provider. It is an approximation based on the amount of text sent and received, so treat it as a guide to usage and cost, not as an invoice-accurate figure. Reported values are never adjusted to match an estimate.

Reasoning-heavy models may count hidden reasoning against the output allowance. Domovoi includes reasoning the provider exposes in its estimate. An answer whose tokens cannot be counted, or whose reasoning the provider redacted, is charged at its authorised output allowance, so a missing total never makes a billable request look free. Redacted reasoning content is never shown or stored.

### Output limits and the run budget

`triage.per_run.output_tokens` is the allowance for one model turn (default 2000, capped at 16000). If a model stops at that allowance before finishing, Domovoi does not accept the partial answer: it does not show it, does not run any tool call in it and does not add it to the conversation. It retries the turn with four times the allowance, up to the 16000 cap.

`triage.per_run.total_tokens` limits the output the whole run may ask for, including every normal turn, the growth retries and the report repair. It is optional; when unset, Domovoi calculates a default from the other settings (82,000 with defaults). Every request is charged against it before it is sent. Provider fallbacks and retries share one wall-clock deadline, `triage.per_run.seconds` (90 seconds by default).

## Coding-agent CLIs

The `cli` kind runs a program you already use, such as Claude Code or the Codex CLI, with its own login. Domovoi runs the program directly with an argument list and no shell. It sends one prompt with the incident context and reads the final answer. The CLI is given no tools.

```toml
[llm.secondary]
kind = "cli"
cli = { preset = "claude", command = "/usr/local/bin/claude" }
```

`command` must be an absolute path. The built-in presets start from these arguments:

| Preset | Arguments | Prompt | Answer |
| --- | --- | --- | --- |
| `claude` | `-p --output-format json --tools "" --strict-mcp-config --no-session-persistence` | standard input | the `result` field of the JSON output |
| `codex` | `exec --sandbox read-only --skip-git-repo-check -` | standard input | all of standard output |

> **Warning: check the flags against your CLI version.** CLIs change their flags between releases, and the presets are only starting points. Run `claude --help` or `codex exec --help` on the hub machine. Confirm the arguments ask for non-interactive print mode, a single turn, no tool use and, where the CLI supports it, machine-readable output. If anything differs, set `args`. A non-empty `args` replaces the preset's arguments completely:
>
> ```toml
> cli = { preset = "claude", command = "/usr/local/bin/claude", args = ["-p", "--output-format", "json", "--tools", ""] }
> ```

The CLI runs with an empty environment apart from what Domovoi sets for it. A CLI that fails, or runs past its time limit, counts as unavailable. Its error message is checked for credentials and cut to 300 characters before it is shown.

## Prices and cost

To track spending against `monthly_budget_usd`, give each paid provider its price in US dollars per million tokens:

```toml
price = { input_per_mtok_usd = 3.0, output_per_mtok_usd = 15.0 }
```

Domovoi has no built-in price list. Without `price`, the cost of a turn is unknown. Unknown cost is not the same as free: if a model genuinely costs nothing, give it an explicit zero price.

When `monthly_budget_usd` is set, every provider in the chain — the primary, its fallbacks and its fallback models — must have a price before any request is made, because a turn that cannot be priced cannot be counted. A provider with no price is refused with a message naming it, and nothing is sent to it. A local model that costs nothing is configured with both parts set to `0`.

Without a monthly budget, turns from an unpriced provider still run. Their tokens are counted and estimated, but their cost cannot be included in a monthly total.

A CLI provider is not inherently free. With a `price` it is billed from its estimated usage; when its JSON output reports complete totals, those are priced directly, and a partial report keeps the reported half and estimates the other.

## Runs per hour

Triage answers at most `per_hour_runs` times an hour (6 by default), and keeps
`reserved_for_incidents` of them free for triage itself (2 by default, or one fewer than
the hour when that is smaller). Follow-up questions
(web and Discord) and the general chat count against the same hourly limit, but they may
only use it while more than `reserved_for_incidents` runs are left, so a few questions can
never push a new incident's triage out of the hour. When the hour is used up, triage waits
for the oldest run to leave it; a question or a chat says so at once, and the API answers
429 with a `Retry-After` header. One question counts as one run whatever the number of tool
calls its answer makes, and no model call starts without passing this check. The monthly
`monthly_budget_usd` is checked in the same place, before every model call, together with
the requirement that every provider has a price.

## Failures

A provider request is cut off after 120 seconds without any data; the timer restarts whenever data arrives, including streamed bytes. Connecting has a 10-second limit. Separately, one triage, follow-up or chat run has a total wall-clock deadline (`triage.per_run.seconds`, 90 seconds by default) that no retry or provider fallback can extend. A failed turn is reported as one of:

| What happened | Treated as |
| --- | --- |
| HTTP 401 or 403 | the credentials were refused; check the key |
| HTTP 429 | rate limited, waiting as long as `retry-after` says |
| HTTP 408, 5xx or 529, a timeout, or no connection | the provider is unavailable for now |
| A body Domovoi cannot read | an invalid response, quoted in short |
| The server refuses an optional field | retried without that field; only a failure if the rebuilt request also fails |

API keys never appear in logs, errors or the web UI. When a model fails, triage tries its provider's fallback models, then the other provider and its fallback models, and posts the incident without a report only when every one of them failed. Follow-up questions and chat report the same per-provider reasons.
