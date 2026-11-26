# 0041. The audit log is a hash chain

- Status: Accepted
- Date: 2026-11-26

## Context

The audit log answers who approved what and who changed which setting. An append-only trigger stops edits through the application and ordinary SQL, but not a person with the database file. The owner needs a way to notice tampering, and a way to prove the log's head at a time without trusting the file.

## Decision

- `audit_log` is append-only: triggers reject `UPDATE`, `DELETE` and the replacement of a row.
- Every row carries `prev_hash` and `hash`. The store hashes each event (canonical JSON, SHA-256) inside the transaction that inserts it, and `insert_audit` is the only code that inserts into `audit_log`. A failed commit rolls the chain position back with it, so every state change and its audit row commit together or not at all.
- One `audit_anchor` row fixes the start of the chain. It is append-only too.
- `domovoi audit verify` and `GET /api/v1/audit/verification` check the chain, and `domovoi audit head` prints the newest hash for the owner to keep elsewhere.
- An audit failure fails the transition it explains: the store never holds a status without the entry that records it.
- Audit rows never name a secret, a typed confirmation, an answer's text or a plan hash's inputs.

## Consequences

- The chain detects edited rows, deletions inside the chain and rows inserted by tools that do not compute the chain.
- It cannot detect a full rewrite of the database that recomputes every hash, or the deletion of the newest rows, unless the head hash was recorded elsewhere. Keeping the head off the host closes both.
- Retention never prunes the audit log.

## Alternatives considered

- A trigger alone: stops application-level edits but leaves no evidence of file-level tampering.
- Signing each row with a key held by the hub: the key sits next to the database, so a person with the file holds it too.
- Shipping rows to an external log service: adds a required service; the head hash gives the same assurance for the owner who wants it.
