-- Policy versions: one editable draft, immutable numbered versions, their rules and the
-- acknowledgements given at publication, plus the decisions the shadow recorder stores for
-- every Execution step. Additive: nothing existing changes. Names of statuses are validated in
-- Rust when read, so only formats and booleans are checked here.

CREATE TABLE policy_versions (
    id             TEXT PRIMARY KEY,
    status         TEXT NOT NULL,
    number         INTEGER,
    draft_revision INTEGER NOT NULL DEFAULT 1,
    based_on       INTEGER,
    rules_hash     TEXT NOT NULL CHECK (length(rules_hash) = 64 AND rules_hash NOT GLOB '*[^0-9a-f]*'),
    rule_count     INTEGER NOT NULL,
    locked_count   INTEGER NOT NULL,
    shadow_count   INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    created_by     TEXT NOT NULL,
    updated_at     INTEGER NOT NULL,
    updated_by     TEXT NOT NULL,
    published_at   INTEGER,
    published_by   TEXT,
    CHECK ((status = 'published') = (number IS NOT NULL)),
    CHECK ((status = 'published') = (published_at IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX policy_versions_number    ON policy_versions (number) WHERE number IS NOT NULL;
CREATE UNIQUE INDEX policy_versions_one_draft ON policy_versions (status) WHERE status = 'draft';

CREATE TABLE policy_rules (
    version_id TEXT NOT NULL REFERENCES policy_versions(id),
    rule_id    TEXT NOT NULL,
    ordinal    INTEGER NOT NULL,
    enabled    INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    shadow     INTEGER NOT NULL CHECK (shadow  IN (0, 1)),
    locked     INTEGER NOT NULL CHECK (locked  IN (0, 1)),
    body       TEXT NOT NULL,
    PRIMARY KEY (version_id, rule_id),
    UNIQUE (version_id, ordinal)
) STRICT;
CREATE TRIGGER policy_rules_frozen_when_published
BEFORE UPDATE ON policy_rules
WHEN (SELECT status FROM policy_versions WHERE id = OLD.version_id) = 'published'
BEGIN
    SELECT RAISE(ABORT, 'a published policy version is immutable');
END;
CREATE TRIGGER policy_rules_no_delete_when_published
BEFORE DELETE ON policy_rules
WHEN (SELECT status FROM policy_versions WHERE id = OLD.version_id) = 'published'
BEGIN
    SELECT RAISE(ABORT, 'a published policy version is immutable');
END;

CREATE TABLE policy_acknowledgements (
    version_id      TEXT NOT NULL REFERENCES policy_versions(id),
    finding_key     TEXT NOT NULL,
    kind            TEXT NOT NULL,
    first_rule      TEXT NOT NULL,
    second_rule     TEXT NOT NULL,
    resolution      TEXT,
    note            TEXT NOT NULL,
    acknowledged_by TEXT NOT NULL,
    acknowledged_at INTEGER NOT NULL,
    PRIMARY KEY (version_id, finding_key)
) STRICT;

-- No foreign key on execution_id: decision rows outlive pruned executions.
CREATE TABLE policy_shadow_decisions (
    execution_id     TEXT PRIMARY KEY,
    operation_id     TEXT NOT NULL,
    host_id          TEXT NOT NULL,
    contract         TEXT NOT NULL,
    step_created_at  INTEGER NOT NULL,
    evaluated_at     INTEGER NOT NULL,
    status           TEXT NOT NULL,
    reason           TEXT,
    policy_version   INTEGER NOT NULL,
    rules_hash       TEXT NOT NULL,
    enforced_outcome TEXT,
    shadow_outcome   TEXT,
    degraded         INTEGER NOT NULL CHECK (degraded IN (0, 1)),
    candidate        TEXT NOT NULL,
    inputs           TEXT,
    enforced         TEXT,
    shadow           TEXT,
    gaps             TEXT NOT NULL
) STRICT;
CREATE INDEX policy_shadow_evaluated ON policy_shadow_decisions (evaluated_at DESC, execution_id DESC);
CREATE INDEX policy_shadow_host      ON policy_shadow_decisions (host_id, contract, evaluated_at);
CREATE INDEX policy_shadow_version   ON policy_shadow_decisions (policy_version, evaluated_at);

CREATE TABLE policy_shadow_state (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    last_sweep_at     INTEGER,
    last_error        TEXT,
    total_evaluated   INTEGER NOT NULL DEFAULT 0,
    total_unevaluable INTEGER NOT NULL DEFAULT 0,
    total_skipped     INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT INTO policy_shadow_state (id) VALUES (1);
