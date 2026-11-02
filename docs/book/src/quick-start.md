# Quick start

This puts the hub and an agent on one Linux machine (x86_64 or arm64) and gets you to a first triaged card in about ten minutes. Later you can add agents on other machines, or move the hub elsewhere so it notices when a whole host goes down.

You need root on the machine, a Discord server where you can add a bot, and an API key for a model (Anthropic, OpenAI or any OpenAI-compatible server), or a Claude Code or Codex login.

## 1. Install both roles

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh -o install.sh
sudo sh install.sh --role hub
sudo sh install.sh --role agent
```

The installer checks the download's SHA-256, installs `/usr/local/bin/domovoi` and two systemd units, and writes no config file. See [Binary and systemd](install/binary.md) for what it creates and how to verify it.

## 2. Start the hub and create your account

```sh
sudo systemctl enable --now domovoi-hub
sudo journalctl -u domovoi-hub | grep -A1 setup
```

Open the link. From another machine, replace `127.0.0.1` with this machine's address, or tunnel the port: `ssh -L 7420:127.0.0.1:7420 <host>`. Create the admin account with a password of at least 12 characters.

## 3. Follow the guided first run

The first run walks through the rest. Each step can be skipped and done later on **Settings**:

1. **Hub:** a name, and the addresses people and agents use. For one machine, the agent address is `wss://127.0.0.1:7421`.
2. **Discord:** create the bot, invite it and paste its token, the channel id and your user id. The [Discord guide](guide/discord.md) has the clicks.
3. **Model:** pick a provider and paste its key. **Test** sends one short request. See [LLM providers](guide/llm-providers.md).
4. **Digest:** the time of the daily summary.
5. **First host:** continue to the next step here.

## 4. Enroll the agent

On **Hosts**, choose **Add host**, name it after the machine, and copy the command it shows. Run it with `sudo`:

```sh
sudo domovoi agent enroll --hub wss://127.0.0.1:7421 --token … --fingerprint … --name …
sudo systemctl enable --now domovoi-agent
```

The host shows as connected within seconds, and its built-in checks start: disks, memory, temperatures, containers, failed units, SMART, logins and more. See [Built-in checks](reference/checks.md).

## 5. Install the starter skills

Skills are the only things Domovoi can ever run on a host, and only after you approve. The release ships seven starter skills: read-only overviews, container and unit restarts, and a journal vacuum.

```sh
sudo cp -R /usr/local/share/domovoi/examples/skills /etc/domovoi/skills
sudo chown -R root:root /etc/domovoi/skills
sudo chmod -R go-w /etc/domovoi/skills
sudo domovoi check skills /etc/domovoi/skills
sudo systemctl restart domovoi-agent
```

[Writing skills](guide/skills.md) explains each one and how to add your own.

## 6. See a card

Send a synthetic critical through the whole pipeline with your real model and Discord:

```sh
sudo -u domovoi domovoi selftest
```

A card appears in your channel with a verdict, a short explanation and a thread, then resolves. For a real incident, stop a container you do not need (`docker stop <name>`) and wait two minutes: the card names the container, the triage agent explains what it found, and **Approve** on a `restart_container` proposal starts it again.

## Next

- [How it works](concepts/how-it-works.md)
- [Agents and enrollment](guide/agents.md), to watch more machines
- [Hardening](guide/hardening.md), before you expose the web UI
