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

## Management paths

A management path records how a host is reached for management. Because a wrong path can lock you out, it counts only when it is current and confirmed by the owner.

A confirmation needs a verified, fresh helper attestation that shows the path covered. That attestation is not built yet. Until it ships, every confirmation fails with:

> the helper's coverage cannot be verified yet

Monitoring and work you approve by hand are not affected by this.

## Upgrading from host profile notes

Host profile notes are replaced by facts. When the knowledge store first runs, each host's profile note is kept and labelled `superseded`. Its machine description is copied into a `discovered` `host_platform` fact, whose evidence is `note:<id>` pointing at the old note.
</content>
