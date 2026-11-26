# 0039. Start from a single baseline migration

- Status: Accepted
- Date: 2026-11-26
- Changes `xtask/src/arch/git.rs` (the append-only migration rule, R13).

## Context

The schema grew in twenty-two numbered migrations while the product was built. Some rebuild tables, some add columns that only held data from an earlier design, and some create tables the product no longer uses. No release has been made, so no database exists that needs that history. Keeping the history would mean every new install replays twenty-two files, and every reader of the schema has to reconstruct the final shape from them.

Rule R13 forbids editing or deleting a merged migration. That rule protects released databases, and there are none yet.

## Decision

The twenty-two migrations are replaced once by a single `0001_init.sql` that creates the schema as it is. It has no table rebuild steps, no columns that only held earlier data and no tables the product no longer reads. The audit chain starts at the first row of a new database, so the audit anchor has no columns for earlier rows.

From the first release, migrations are immutable. A schema change is a new numbered file next to the baseline.

`xtask arch-check --base <ref>` keeps rejecting any edit, rename or deletion of a migration. It makes one exception: a change that also adds this ADR (`docs/adr/0039-single-baseline-migration.md`) may rewrite the migrations directory. Once this file exists on the base ref it cannot be added again, so the exception applies to the change that squashes the history and to no later one.

## Consequences

- A new database is created by one file, and its schema is the schema the code expects.
- A database created by an earlier development build cannot be opened by this build: it holds migration versions the baseline does not know, and the newer-schema guard refuses it. Development databases are recreated.
- The guards that serve the next release stay: the refusal to open a database written by a newer build, the quick check, and the backup taken before a migration runs.
- A test fails if a later change edits the baseline without adding this ADR.

## Alternatives considered

- Keep the history and add a new migration that drops what is unused. It leaves twenty-three files to read and keeps the rebuild steps.
- Let the rule allow edits to the baseline until a release tag exists. It depends on a tag that does not exist yet and leaves the exception open after the release.
