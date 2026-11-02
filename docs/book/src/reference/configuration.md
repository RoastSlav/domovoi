# Configuration

Domovoi has separate TOML files for the hub and each agent. The source checkout's
`examples/hub.toml` shows hub settings for the database, web and agent listeners, LLM
providers, triage, incident policy, notifications, Discord, webhooks, probes, backups
and secret hygiene. Its `examples/agent.toml` shows agent identity and paths, skill risk
policy, Docker access, check settings, container baselines and the local webhook relay.

The examples are the most complete reference today. Settings omitted from a file use
the defaults documented beside their fields in `domovoi-config`. The configuration
loader rejects unknown fields, validates cross-field rules and reports issues with TOML
paths. A runtime loads and validates its file at startup.

The binary does not yet provide `domovoi check config`; there is no standalone CLI
validator. Secrets must be indirect references, such as `{ env = "DISCORD_BOT_TOKEN" }`
or `{ file = "/etc/domovoi/discord-token" }`, and are held as secret strings after
loading. Do not put secret values directly in TOML.

Scalar values can be overridden with environment variables named
`DOMOVOI_<SECTION>__<KEY>`, using double underscores between nested names. For example,
`DOMOVOI_WEB__BIND=127.0.0.1:7420` overrides `[web].bind`.
