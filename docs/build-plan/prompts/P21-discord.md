# P21 — Discord gateway and notifier

| | |
| --- | --- |
| Phase | 5, Surfaces |
| Depends on | P12, P17, P18 |
| Crates | `domovoi-chat`, `domovoi-hub` (interaction authorizer in `auth/`) |
| Size | L |
| Invariants touched | INV-1, INV-6 (chat approvals), INV-5 |

## Goal

Make Discord the interactive front end: one thread per incident with a compact card, buttons for ack, mute, details and approvals, follow-up questions in the thread, a few slash commands, and a one-way webhook notifier for digests. The gateway only translates; the hub decides who is allowed to do what.

## Read first

- `ARCHITECTURE.md` §13, §15
- ADR 0010, ADR 0007
- P03 `ChatGateway`, `InteractionSink`; P12 cards and render; P17 approval service

## In scope

Add to `domovoi-chat`: `serenity` (workspace features), `poise`, `reqwest`, `serde`, `serde_json`, `thiserror`, `async-trait`, `tracing`, `secrecy`, `base64`. Verify `cargo deny` still passes and that the crypto provider setup is compatible with the rest of the workspace (document in the PR which rustls provider serenity pulls in).

### Gateway (`discord/`)

- `DiscordGateway::new(token: SecretString, channel_id, settings)` implementing `ChatGateway`.
- Intents: `GUILDS`, `GUILD_MESSAGES`, `MESSAGE_CONTENT` (privileged; document enabling it in the Discord developer portal). No DMs.
- **Card → message** (`embed.rs`): embed colour by badge (ACTION NEEDED red, MONITOR amber, EXPECTED grey, FIXED green; severity colours when there is no verdict); title `"{badge} · {host}: {headline}"` (≤ 256 chars); description = bullets (≤ 3) and the next step in bold; footer with the incident short ID; resolved cards get a ✅ prefix and muted colour.
- **Buttons** (`components.rs`): custom IDs `dv1:{kind}:{incident}[:{action}:{token}]` kept ≤ 100 characters (use 8-byte base64url short IDs mapped server-side if UUIDs do not fit). `Ack`, `Mute 1h`, `Details`; `Approve`/`Deny` only for read/write actions; destructive actions show a link button to the web UI instead.
- **Threads:** `open_thread` creates a public thread from the card message named after the headline (≤ 100 chars). `reply` posts plain markdown, splitting messages over 2,000 characters.
- **Interactions:** component presses and thread messages become `Interaction`s delivered to the `InteractionSink`. The gateway responds to the component interaction within 3 s: deferred update, then an ephemeral message with the sink's `InteractionReply` (rejections are ephemeral: "Not allowed").
- **Slash commands** (poise): `/status` (hosts and open incident counts), `/incidents` (open list with links), `/mute incident:<id> duration:<1h|4h|24h>`, `/digest` (sends the digest now). All go through the sink as `InteractionKind::Command`.
- Reconnects and rate limits are handled by serenity; log disconnects at warn.

### Notifier (`discord/webhook.rs`)

- `DiscordWebhookNotifier` implementing `Notifier`: `POST` the webhook URL with `{ "content": ..., "embeds": [...] }`, splitting long digests into multiple messages; retries on 429 respecting `retry_after`. The URL is a secret.

### Hub authorizer (`crates/domovoi-hub/src/auth/interactions.rs`)

- Implements `InteractionSink`. Rejects any interaction whose `platform_user` is not in `[discord] allowed_user_ids`.
- Buttons map to engine commands: Ack → manager `Ack`; Mute 1h → `Mute`; Details → reply in the thread with the report details; Approve → load the action: if destructive → reject "Approve this one in the web UI" (INV-6); else build `HumanApproval::new(admin_user, ApprovalChannel::Chat { platform: "discord", platform_user }, now)` and call `ApprovalService::approve` with the button's token (single use); Deny similarly.
- Thread messages from allowed users → `TriageHandle::follow_up`, answer posted with `reply`.
- Commands map to the same services the API uses.
- Every accepted interaction is audited with the platform user ID.

## Out of scope

- Telegram and other platforms (v0.2). Wiring the gateway into the hub runtime and dispatcher (P22).

## Tests required

No live Discord calls in CI. Unit-test the pure mapping and use `RecordingGateway`-style fakes for the hub side:

1. Insta snapshots of the embed JSON for each badge, resolved, and with each button set.
2. `custom_id_round_trip_and_length_limit`.
3. `long_replies_are_split_at_2000_chars_on_line_boundaries`.
4. `webhook_notifier_splits_digest_and_retries_on_429` (wiremock).
5. Authorizer: `non_allowlisted_user_is_rejected`, `ack_and_mute_reach_incident_manager`, `approve_with_valid_token_approves_once`, `reused_token_is_rejected`, `thread_message_becomes_follow_up_and_reply`.
6. Gates (`crates/domovoi-hub/tests/gates.rs`): `destructive_action_cannot_be_approved_from_discord` (INV-6), `discord_message_text_can_never_approve` (a thread message saying "approve" changes nothing) (INV-1).
7. An `#[ignore]` live smoke test that posts a card to a test channel when `DOMOVOI_DISCORD_TEST_TOKEN` and `DOMOVOI_DISCORD_TEST_CHANNEL` are set.

## Architecture gates

- `domovoi-chat` depends only on `domovoi-core` internally; it never decides authorization.
- `HumanApproval::new` only in `crates/domovoi-hub/src/auth/`.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/guide/discord.md`: creating the bot, intents, permissions (send messages, create public threads, embed links, use application commands), inviting it, finding channel and user IDs, configuration.

## Deliverables

- `crates/domovoi-chat/src/discord/*`, `crates/domovoi-hub/src/auth/interactions.rs`
- Tests, snapshots, `docs/book/src/guide/discord.md`

## PR

Title: `feat(chat): Discord threads, buttons, follow-ups and webhook notifier`
