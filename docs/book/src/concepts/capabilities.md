# Capabilities

A **capability** is one thing Domovoi can do to a host: read its containers, read its journal, run a skill, look up an incident. Each one is described by a **contract** that the hub itself owns: an id such as `container.logs` or `skill.prune_logs`, a title, the effect class it carries, the kind of target it acts on, and where it executes.

Agents do not define capabilities. An agent reports what it implements — a list of contract ids and versions, plus a summary of every skill it loaded — and the hub decides which contracts exist and how dangerous each one is. Nothing an agent sends can add a capability or change its effect class.

## Two switches, both yours

- **Enabled** in a skill's row switches that skill off for one host. The skill stays on the host and in the list, and Domovoi stops running it and stops offering it.
- A **floor** can raise a skill's effect class for one host, never lower it. A skill you mark `mutate` cannot be made safe again by a setting; the floor only makes it stricter.

## The eight states

Every capability row answers one question: *would the hub offer this here, and if not, why not?*

- **Available** — the host implements it, you enable it, and nothing it needs is missing.
- **Not installed** — the host's own list, read completely, does not contain it.
- **Disabled here** — you switched it off for this host; the agent still has it.
- **Refused by the host** — the host's own unattended limit does not cover it.
- **Unavailable** — the host has it, but something it needs is missing or broken right now, such as an isolated skill runner or a container runtime.
- **Host unknown** — this host has never connected to this hub run.
- **Host offline** — the host had connected and its agent is gone.
- **Incompatible** — the host's agent speaks another protocol version than the hub.

## What "available" does not mean

Available is not approved. A row says the hub would offer the capability on that host; whether a particular request may run is decided when the request is made, by the policy layer and, for anything that changes a host, by you. The Capabilities page only reports: it never enables, disables or approves anything.

## Host limits

Every host decides for itself what may run without a person: the `[unattended]` section of its `agent.toml` is its unattended ceiling. The agent sends a summary of it when it connects, and the **Host limits** tab of the Capability Center shows that summary for the selected host, read-only. Hub policy may only narrow what the host allows, never widen it.

- **Reported by the host** lists each capability that may run unattended, the targets it covers, and its limits per hour, at once and for how long.
- **Not reported** means the host has sent no limits, because it is offline or its agent does not send them yet. This is information, not a fault. Until a host reports, Domovoi treats unattended actions on it as not allowed.
- **Unreadable** means the host sent something the hub could not read. It is treated as not allowed.

To change a host's limits, edit the `[unattended]` section of `/etc/domovoi/agent.toml` on that host and restart the agent. The hub has no way to do it.

## The host's own list

The hub reads a host's implementation list page by page and checks it against the fingerprint the agent announced. If any part of that read fails — a page does not arrive, the agent leaves, the fingerprint does not match — the list is shown as unverified, and a capability missing from it is never reported as "not installed": an unread list proves nothing about what a host lacks.
