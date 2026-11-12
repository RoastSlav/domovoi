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

Triage and follow-up questions try the fallback whenever the main provider fails for any reason: no connection, rate limited, refused credentials, an answer Domovoi cannot read, or an answer that is not a usable report. When both providers fail, the failure says why each one did.

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

Domovoi has no built-in price list. Without `price`, the cost of a turn is unknown and does not count toward the budget. CLI turns never have a cost.

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
`monthly_budget_usd` is checked in the same place, before every model call.

## Failures

Each request has a total time limit of 120 seconds, and connecting has 10 seconds. A failed turn is reported as one of:

| What happened | Treated as |
| --- | --- |
| HTTP 401 or 403 | the credentials were refused; check the key |
| HTTP 429 | rate limited, waiting as long as `retry-after` says |
| HTTP 408, 5xx or 529, a timeout, or no connection | the provider is unavailable for now |
| A body Domovoi cannot read | an invalid response, quoted in short |

API keys never appear in logs, errors or the web UI. When a model fails, triage tries its provider's fallback models, then the other provider and its fallback models, and posts the incident without a report only when every one of them failed.
