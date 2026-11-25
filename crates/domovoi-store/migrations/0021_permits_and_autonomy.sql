-- Policy permits, the global pause switch, circuit-breaker resets and unknown-outcome
-- resolutions. Enums are validated in Rust, never by CHECK, so a new value needs no migration.

-- The permit that authorized one Execution step (at most one per execution).
CREATE TABLE execution_permits (
    execution_id   TEXT PRIMARY KEY REFERENCES executions (execution_id),
    decision_id    TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    ceiling_hash   TEXT NOT NULL,
    plan_hash      TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    issued_at      INTEGER NOT NULL,
    expires_at     INTEGER NOT NULL,
    decision       TEXT NOT NULL
) STRICT;

-- One row per rule a permit names; the circuit breaker reads this.
CREATE TABLE execution_permit_rules (
    execution_id TEXT NOT NULL REFERENCES execution_permits (execution_id),
    rule_id      TEXT NOT NULL,
    PRIMARY KEY (execution_id, rule_id)
) STRICT;
CREATE INDEX execution_permit_rules_rule ON execution_permit_rules (rule_id);
CREATE INDEX execution_permits_issued    ON execution_permits (issued_at);

-- The global pause switch: exactly one row.
CREATE TABLE autonomy_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    paused     INTEGER NOT NULL CHECK (paused IN (0, 1)),
    changed_by TEXT,
    changed_at INTEGER,
    reason     TEXT
) STRICT;
INSERT INTO autonomy_state (id, paused) VALUES (1, 0);

-- An owner reset of a rule's circuit breaker.
CREATE TABLE rule_circuit_resets (
    rule_id  TEXT NOT NULL,
    reset_at INTEGER NOT NULL,
    reset_by TEXT NOT NULL,
    PRIMARY KEY (rule_id, reset_at)
) STRICT;

-- An owner, or a later verified run, clearing the unattended block of an OutcomeUnknown execution.
CREATE TABLE unknown_resolutions (
    execution_id TEXT PRIMARY KEY REFERENCES executions (execution_id),
    resolved_at  INTEGER NOT NULL,
    resolved_by  TEXT NOT NULL,
    how          TEXT NOT NULL,
    note         TEXT
) STRICT;
