# FAQ

## Does Domovoi change anything on my servers by itself?

No. The model can only read. It can *propose* a skill you installed, with arguments that must pass the skill's manifest, and nothing runs until a person approves it. Destructive skills can only be approved in the web UI, by typing the skill's name. See the [security model](security.md).

## Do I need an LLM?

No, but it is the point. Without a model, Domovoi still runs every check, groups signals into incidents, posts cards and digests, and lets you run skills. Cards then have no verdict or explanation.

## Which models work?

Any model behind the Anthropic API or an OpenAI-compatible API (OpenAI, OpenRouter, Ollama, LM Studio, vLLM), or a Claude Code or Codex login. Triage needs tool calling for its best results. Small local models work, but follow the instructions less reliably. See [LLM providers](guide/llm-providers.md).

## What does it cost to run?

Domovoi itself is free and open source. Model costs depend on how often things break: triage runs only when an incident opens or comes back, with a capped number of tool calls. Set a monthly budget in **Settings → Models and triage**, and triage pauses when it is spent.

## What leaves my network?

What triage reads goes to your model provider: incident evidence, log excerpts and tool output, with secrets redacted. Cards go to Discord. Nothing else is sent anywhere. With a local model and no chat, nothing leaves at all.

## How many hosts can one hub handle?

It is built for homelabs and small setups: one to a few dozen hosts. Each agent sends a heartbeat every 10 seconds and signals only when something changes.

## Can I run it without Docker or systemd?

Yes. The binary is static and runs anywhere on Linux. Docker checks turn off when there is no Docker, and unit checks when there is no systemd.

## Does it replace Grafana, Prometheus, Uptime Kuma or Netdata?

No. Keep them for graphs and history. Send their alerts to a [webhook](guide/webhooks.md) and Domovoi triages them with everything else. It also exposes its own `/metrics`.

## Can I manage everything as files?

Yes. Every setting can come from `hub.toml` and environment variables, and the web UI then shows it read-only. See [config as code](install/config-as-code.md) and the [configuration reference](reference/configuration.md).

## Is there Telegram, Matrix or email?

Not yet. Discord is the chat in v0.1. Other chats and notifiers plug in as new adapters later.

## How do I know triage is any good?

Every release runs a set of drills: realistic incidents with decoys and prompt injections, scored on whether the verdict, the blame and the proposed action are right, and on whether every safety gate held. You can run them against your own model; see the `drills-live` workflow in the repository.
