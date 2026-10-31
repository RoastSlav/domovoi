# 0010. Discord is the first interactive chat gateway

- Status: Accepted
- Date: 2026-10-31

## Context

v0.1 needs one interactive chat platform with threads per incident, buttons and follow-up questions. The reference deployment already alerts into Discord.

## Decision

- v0.1 ships a Discord gateway (bot with threads, buttons and a few slash commands) and a one-way Discord webhook notifier.
- Telegram and Matrix follow as additional `ChatGateway` adapters; the core `Card` and `Interaction` models must not contain Discord-specific fields.

## Consequences

- Card rendering is snapshot-tested per gateway; the engine never formats platform text itself.
- The Discord bot token is a required secret when the gateway is enabled.

## Alternatives considered

- Telegram first: simpler bot API, but weaker threading and not where the first users are.
