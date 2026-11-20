-- Resolution summaries: append-only field revisions (the truth) and a derived projection with a
-- full-text index. Additive only: nothing existing is altered, and an older binary ignores
-- these tables.

CREATE TABLE resolution_summary_fields (
    operation_id TEXT    NOT NULL REFERENCES operations(id),
    field        TEXT    NOT NULL,                 -- a summary field name, validated in Rust
    revision     INTEGER NOT NULL,                 -- 1-based, dense per (operation_id, field)
    value        TEXT    NOT NULL,                 -- canonical JSON, redacted before it gets here
    source       TEXT    NOT NULL,                 -- 'code' | 'model' | 'user', validated in Rust
    actor        TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (operation_id, field, revision),
    CHECK (revision >= 1),
    CHECK (length(value) <= 16384)
) STRICT;

CREATE TRIGGER resolution_summary_fields_append_only_update
BEFORE UPDATE ON resolution_summary_fields
BEGIN
    SELECT RAISE(ABORT, 'summary revisions are append-only');
END;

CREATE TRIGGER resolution_summary_fields_append_only_delete
BEFORE DELETE ON resolution_summary_fields
BEGIN
    SELECT RAISE(ABORT, 'summary revisions are append-only');
END;

-- One row per operation that has a summary: the current value of each field, rewritten by the
-- store in the same transaction as every revision. Nothing else writes it.
CREATE TABLE operation_resolutions (
    operation_id TEXT    PRIMARY KEY REFERENCES operations(id),
    goal         TEXT    NOT NULL,                 -- copied from operations.goal (already redacted)
    outcome      TEXT    NOT NULL,
    verified     INTEGER NOT NULL DEFAULT 0,       -- 0 or 1
    problem      TEXT    NOT NULL DEFAULT '',      -- newest revision's text, '' when none
    cause        TEXT    NOT NULL DEFAULT '',
    fix          TEXT    NOT NULL DEFAULT '',
    subjects     TEXT    NOT NULL DEFAULT '',      -- words of the pinned subjects
    contracts    TEXT    NOT NULL DEFAULT '',      -- words of the executed contracts
    tags         TEXT    NOT NULL DEFAULT '',      -- space-separated outcome words
    ended_at     INTEGER NOT NULL,                 -- copied from operations.ended_at
    updated_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX operation_resolutions_ended ON operation_resolutions (ended_at DESC, operation_id DESC);

-- The `hosts` field, one row per host; scopes host searches.
CREATE TABLE operation_resolution_hosts (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    host_id      TEXT NOT NULL,
    PRIMARY KEY (operation_id, host_id)
) STRICT;

CREATE INDEX operation_resolution_hosts_host ON operation_resolution_hosts (host_id, operation_id);

-- The `subjects` field, one row per pinned subject; finds similar operations.
CREATE TABLE operation_resolution_subjects (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    host_id      TEXT NOT NULL,
    check_key    TEXT NOT NULL,
    target       TEXT NOT NULL DEFAULT '',         -- '' when the subject has no target
    PRIMARY KEY (operation_id, host_id, check_key, target)
) STRICT;

CREATE INDEX operation_resolution_subjects_lookup
    ON operation_resolution_subjects (host_id, check_key, target, operation_id);

CREATE VIRTUAL TABLE operation_resolutions_fts USING fts5 (
    goal, problem, cause, fix, subjects, contracts, tags,
    content = 'operation_resolutions', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);

CREATE TRIGGER operation_resolutions_fts_insert AFTER INSERT ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES (new.rowid, new.goal, new.problem, new.cause, new.fix, new.subjects, new.contracts, new.tags);
END;

CREATE TRIGGER operation_resolutions_fts_delete AFTER DELETE ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (operation_resolutions_fts, rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES ('delete', old.rowid, old.goal, old.problem, old.cause, old.fix, old.subjects, old.contracts, old.tags);
END;

CREATE TRIGGER operation_resolutions_fts_update AFTER UPDATE ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (operation_resolutions_fts, rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES ('delete', old.rowid, old.goal, old.problem, old.cause, old.fix, old.subjects, old.contracts, old.tags);
    INSERT INTO operation_resolutions_fts (rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES (new.rowid, new.goal, new.problem, new.cause, new.fix, new.subjects, new.contracts, new.tags);
END;
