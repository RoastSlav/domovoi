# Quick start

This puts the hub and an agent on one Linux machine (x86_64 or arm64) and gets you to a first triaged card in about ten minutes. Later you can add agents on other machines, or move the hub elsewhere so it notices when a whole host goes down.

You need root on the machine, a Discord server where you can add a bot, and an API key for a model (Anthropic, OpenAI or any OpenAI-compatible server), or a Claude Code or Codex login.

## 1. Install the hub and an agent

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh -o install.sh
sudo sh install.sh --role both
```

`--role both` checks the machine, verifies the download's SHA-256, installs `/usr/local/bin/domovoi` and the systemd units, starts the hub and waits until it is ready, then enrolls and starts an agent for this machine and waits until it is connected. Add `--dry-run` first to see what it would change. The installer writes no config file for the hub; on the agent it writes only `/etc/domovoi/helper.toml`. The log is `/var/log/domovoi-install.log`. See [Binary and systemd](install/binary.md) for the options and for what it creates, and [Unattended install](install/unattended.md) to run it from a script.

## 2. Create your account

The installer ends with an address to open and a single-use setup link. Open the link and create the admin account with a password of at least 12 characters. From another machine, tunnel the port (`ssh -L 7420:127.0.0.1:7420 <host>`) or install with `--bind lan` so the link uses this machine's address.

To skip the link, put the password in a file (mode 0600) and add `--admin-password-file FILE` to the install command; the account is `admin`.

## 3. Follow the guided first run

The first run walks through the rest. Each step can be skipped and done later on **Settings**:

1. **Hub:** a name, a time zone and the public URL people open. The address agents dial is **Agent server URL** under **Settings**, **Hub**. Until you set it, the enroll command uses `wss://<hub name>:7421`; you can set it at install time with `--agent-url`.
2. **Discord:** create the bot, invite it and paste its token, the channel id and your user id. The [Discord guide](guide/discord.md) has the clicks.
3. **Model:** pick a provider and paste its key. **Test** sends one short request. See [LLM providers](guide/llm-providers.md).
4. **Digest and quiet hours:** the time of the daily summary, and the hours when only critical alerts get through.
5. **First host:** choose **Add a host** here, or continue with the next section.

The bar above the steps shows where you are and which steps you finished or skipped. **Test** on a step checks the connection before you go on. If the page says the hub did not answer, the hub is unreachable: check that it is running and choose **Retry**. The last page lists what you set up and what you skipped, each with a link to its section of **Settings**.

## 4. Check the agent

`--role both` already enrolled the agent. On **Hosts** the machine shows as connected, and its built-in checks start: disks, memory, temperatures, containers, failed units, SMART, logins and more. See [Built-in checks](reference/checks.md).

To watch another machine, choose **Add host** on **Hosts** and follow [Install an agent](install/binary.md#install-an-agent).

## 5. Starter skills

Skills are the only things Domovoi can ever run on a host, and only after you approve. The agent installer copies the eleven starter skills into `/etc/domovoi/skills`, and on every later upgrade it adds any starter skill that is missing. Skills you edit are never overwritten. To check them:

```sh
sudo domovoi check skills /etc/domovoi/skills
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
