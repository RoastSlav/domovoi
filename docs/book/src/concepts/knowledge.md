# Knowledge

Knowledge is what you tell Domovoi about your servers: the role of a host, who owns it, what a container depends on, how a host is reached and when it may be restarted. Each statement is a **fact**. Checks see what a machine reports; facts record what is true about it, who said so and how old that is.

## What a fact is

A fact has:

- a **subject**: `host:<uuid>`, `container:<host>/<name>`, `unit:<host>/<name>` or `entity:<uuid>`;
- a **predicate**, from a closed list: `role`, `criticality`, `owner_note`, `depends_on`, `management_path`, `maintenance_window`, `backup_location`, `never_restart`, `trusted_cidr` and `host_platform`;
- a **value**;
- a **source**: `user`, `discovered`, `integration`, `inferred` or `imported`;
- a **status**: `current`, `stale`, `disputed`, `superseded` or `retracted`;
- its times: when it was observed, when it stops being valid, and when it was last verified;
- an **evidence** reference, and the fact it supersedes, if any.

Facts have no numeric confidence. The source and the status say how far to trust a fact.

## Freshness

Each predicate has a staleness period. Past it, a fact reads as **stale**:

| Predicate | Stale after |
| --- | --- |
| `role`, `criticality`, `depends_on` | 180 days |
| `owner_note`, `never_restart` | 365 days |
| `management_path`, `trusted_cidr` | 30 days |
| `maintenance_window` | 7 days |
| `backup_location` | 90 days |
| `host_platform` | 14 days |

## Who may assert what

- Only the signed-in owner can write a `user` fact.
- Checks, skills and integrations write `discovered`, `integration` and `imported` facts.
- Domovoi's own conclusions are `inferred`. An inferred fact becomes a `user` fact only when the owner accepts it.

## Disputes

`role`, `criticality`, `never_restart` and `host_platform` take one value at a time. When two current facts give different values for one of them, from different sources, both become **disputed**. They stay disputed until the owner asserts one value, or the sources agree.

## Safety-critical predicates

`management_path` and `trusted_cidr` count only while they are current and owner-sourced. Observation never revives a stale one:

- a stale `trusted_cidr` is brought back only when the owner re-verifies it;
- a stale `management_path` is brought back only when the owner confirms it (see [Management paths](#management-paths)).

Writing, accepting or verifying either predicate needs the admin password. Retracting does not.

## Retraction

Retracting a fact removes it from search and from everything sent to a model, at once. Its value is cleared after `retracted_facts_days` (30 days by default). A tombstone stays, with the id, subject, predicate, source, status, actor and timestamps, so you can see that the fact existed and who retracted it.

Backups keep the content of a retracted fact until they rotate out. A restored backup brings it back until the next purge clears it again. See [Retention](../guide/retention.md#what-deletion-guarantees).

Relations follow the same rule: retracting one removes it at once, and its content is cleared on the same schedule.

## Journal and audit

Journal and audit entries never contain fact values. They name the fact by its id and record what changed.

## The Knowledge pages

Open **Knowledge** in the sidebar to see and correct what Domovoi knows. The page follows the host
you chose at the top: pick one host, or all of them.

- **Facts** lists each fact with its source, status and when it was last seen or checked. You can
  filter by status, source and kind, or search by words. Retracted and replaced facts are left out
  until you choose their status. Select a fact to see where it came from, its times, any competing
  values and its history.
- **Proposals** are the facts Domovoi inferred. They are not yours until you accept one. Accepting
  saves a new fact in your name and keeps the original as history. You can retract a proposal
  instead.
- **Relations** and **Entities** list how things connect and the things you named yourself, such as
  a VPN. You can add, retract, rename and delete them. An entity cannot be deleted while a live fact
  or relation uses it.

On a fact you can accept it (make it yours), verify it (mark it as checked today) or retract it.
When two sources disagree, both facts show as disputed and **Use this value** on the right one
settles it. You can also add a fact of your own with **Add a fact**.

Retracting asks you to confirm and never needs the password. Accepting, verifying or adding a
management path or a trusted network asks for your password first, because Domovoi may use those
for safety decisions. A wrong password changes nothing, and too many wrong tries make you wait.

## Management paths

A management path records how a host is reached for management. Because a wrong path can lock you out, it counts only when it is current and confirmed by the owner.

A confirmation needs a verified, fresh helper attestation that shows the path covered. That attestation is not built yet. Until it ships, every confirmation fails with:

> the helper's coverage cannot be verified yet

Monitoring and work you approve by hand are not affected by this.

### The Management paths page

**Management paths** (under Knowledge) shows one host at a time: each path with its state, source,
freshness and who confirmed it. The page says plainly that the helper's check is not available yet,
so **Confirm** is disabled with that reason and nothing is sent. You can still add a path, accept
one Domovoi found and retract a wrong one. Adding and accepting ask for your password.

A line at the top says whether unattended containment is blocked on the host and why: coverage
cannot be verified, no paths, paths Domovoi found that you have not accepted, paths not confirmed,
or stale paths. It only reports what the hub knows. It never says an action is allowed, and it
reminds you that monitoring and actions you approve yourself are not affected. This version never
shows a path as covered.

## Host profiles and facts

A host's profile is a generated note (see [Hub and agents](hub-and-agent.md)). Its machine description (host name, operating system, kernel, architecture, CPU count and memory) is also recorded as a `discovered` `host_platform` fact whose evidence is `note:<id>`, and the note is labelled `superseded-by-facts`. Containers, ports and skills stay in the note, because they are inventory rather than knowledge.
