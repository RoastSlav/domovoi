# Audit log

The audit log records who did what: approvals, actions, setting changes, sign-ins and the hub's own housekeeping. The **Audit log** page in the web UI lets you read it and check it. It only reads: nothing on the page changes, approves or deletes an entry.

## What the page shows

The page reports one status for the log's hash chain: `empty`, `intact` or `broken`. When it is broken, the page names the first entry that fails and says why. See [Audit chain](../reference/data-model.md#audit-chain).

Besides the status the page shows:

- the **head hash**, the hash of the newest entry, with a button to copy it;
- the **anchor**: the install id and the anchor hash the first entry points at;
- the entries, newest first, filterable by kind. Open an entry to read its details. Details are redacted and cut at 4096 bytes; a cut entry says so.

By default the page checks the newest 10,000 entries. **Check the whole log** walks every retained entry on request. On a large log it takes longer, which is why it does not run by itself.

## What the check can and cannot tell you

The chain detects:

- accidental corruption of a row;
- an edit made by a tool that does not recompute the chain;
- a row deleted from the middle of the chain;
- a row inserted without its chain fields.

It does not detect:

- someone who rewrites the whole database and recomputes every hash. The result verifies, because it is a consistent chain;
- removal of the newest rows, unless the head hash was kept somewhere else. The remaining rows still form an intact chain that ends earlier.

So copy the head hash and keep it off the host, after a backup or on a schedule. Comparing it with the head the page shows later is what reveals missing newest rows.

## The command line

[`domovoi audit verify`](../reference/cli.md#domovoi-audit-verify) runs the same check from a terminal and exits 1 when it fails; without `--last` it is the full check. [`domovoi audit head`](../reference/cli.md#domovoi-audit-head) prints the head sequence number and hash for you to store. Both open the database directly, so they need the hub stopped; the page works while it runs.

## Entries the hub writes about the log

- `audit.chain_broken`: the hub found the chain broken when it started. The hub still starts, and the entry is the record that it did. Look at the page to see where the break is.
- `audit.checkpoint`: older entries were removed. The check starts at this entry, so what came before it is not verified.

The HTTP routes behind the page are in the [HTTP API reference](../reference/api.md#audit-log).
