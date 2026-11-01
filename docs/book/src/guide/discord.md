# Discord

Domovoi posts each incident as a card in one Discord channel and opens a thread under it. From the card you can acknowledge, mute, read the details and approve or deny proposed actions. In the thread you can ask the triage agent follow-up questions.

Only the Discord users you list can do anything. Everyone else who presses a button gets "Not allowed", and their messages in threads are ignored.

## Create the bot

1. Open the [Discord developer portal](https://discord.com/developers/applications) and choose **New Application**. Name it, for example `Domovoi`.
2. Under **Bot**, choose **Reset Token** and copy the token. It is shown once. This is `DISCORD_BOT_TOKEN`.
3. On the same page, turn on **Message Content Intent** under *Privileged Gateway Intents*. Without it, the bot can't read follow-up questions in threads. The bot also uses the *Guilds* and *Guild Messages* intents, which need no switch.
4. Turn off **Public Bot**, so only you can add it to servers.

## Invite it to your server

Under **OAuth2 → URL Generator**, tick the scopes `bot` and `applications.commands`. Then tick these bot permissions:

| Permission | Why |
| --- | --- |
| View Channels | See the incident channel |
| Send Messages | Post cards |
| Embed Links | Show the card as an embed |
| Read Message History | Find which incident a thread belongs to |
| Create Public Threads | Open a thread per incident |
| Send Messages in Threads | Answer follow-up questions |
| Use Application Commands | `/status`, `/incidents`, `/mute`, `/digest` |

Or use this link, with your application ID in place of `APP_ID`:

```text
https://discord.com/oauth2/authorize?client_id=APP_ID&scope=bot+applications.commands&permissions=311385213952
```

Open the link, pick your server, and authorize.

## Find the IDs

In Discord, open **User Settings → Advanced** and turn on **Developer Mode**. Then:

- **Channel ID:** right-click the channel incidents should go to, then **Copy Channel ID**.
- **User IDs:** right-click each person who may act on incidents, then **Copy User ID**.

## Configure

```toml
[discord]
bot_token = { env = "DISCORD_BOT_TOKEN" }
channel_id = 123456789012345678
# Who may press buttons, ask in threads and run commands.
allowed_user_ids = [234567890123456789]
# Optional: a channel webhook for digests, used when the bot is down.
webhook_url = { env = "DISCORD_WEBHOOK_URL" }
```

Set `[web] public_url` too. Card titles link to the incident's page in the web UI, and the bot reads that link to know which incident a thread is about.

To create the webhook, open the channel's **Edit Channel → Integrations → Webhooks → New Webhook**, then **Copy Webhook URL**. Treat the URL like a password: anyone who has it can post in the channel.

## What the buttons do

| Button | Effect |
| --- | --- |
| **Ack** | Acknowledges the incident, so reminders stop |
| **Mute 1h** | Mutes it for an hour |
| **Details** | Posts the triage report in the thread |
| **Approve** / **Deny** | Approves or denies a proposed read or write action. Each Approve button works once and expires with the action. |
| **Approve in web UI** | Opens the incident in the web UI. Destructive actions can only be approved there, by typing the skill's name. |

Every press and command is recorded in the audit log with your Discord user ID. Chat actions run as the hub's admin account, so the web UI must be set up first.

Messages in an incident's thread are follow-up questions to the triage agent, which answers in the thread. It can only read: no message, whatever it says, approves anything. Approvals happen only through the buttons.

## Commands

| Command | What it does |
| --- | --- |
| `/status` | Each host's agent and its open incidents |
| `/incidents` | Open incidents with their short IDs and links |
| `/mute incident:<id> duration:<1h\|4h\|24h>` | Mutes an incident by the short ID on its card |
| `/digest` | Sends today's digest now |

Answers are visible only to you. The commands are registered in the incident channel's server when the bot connects, so they appear right away.
