# LLM providers

Triage needs a language model. Domovoi talks to it through one of three adapters:

- `anthropic`: the Anthropic Messages API.
- `openai_compatible`: any server with an OpenAI-style Chat Completions endpoint, such as OpenAI, OpenRouter, Ollama, LM Studio or vLLM.
- `cli`: a coding-agent CLI already installed and logged in on the hub machine, run in print mode.

Configure one provider as `[llm.primary]` and, optionally, a fallback as `[llm.secondary]` in `hub.toml`. Keep API keys in an environment variable or a file, never inline.

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

## Failures

Each request has a total time limit of 120 seconds, and connecting has 10 seconds. A failed turn is reported as one of:

| What happened | Treated as |
| --- | --- |
| HTTP 401 or 403 | the credentials were refused; check the key |
| HTTP 429 | rate limited, waiting as long as `retry-after` says |
| HTTP 408, 5xx or 529, a timeout, or no connection | the provider is unavailable for now |
| A body Domovoi cannot read | an invalid response, quoted in short |

API keys never appear in logs, errors or the web UI. When a provider fails, triage falls back to `[llm.secondary]` and then to posting the incident without a report.
