-- The operation aggregate: a durable record of one piece of work on hosts, made of immutable
-- steps joined by edges that only point backwards. Additive: no existing table changes and
-- every table starts empty. Names of statuses, kinds and outcomes are validated in Rust when
-- they are read, so only formats are checked here.

CREATE TABLE operations (
    id           TEXT    PRIMARY KEY,
    kind         TEXT    NOT NULL,
    goal         TEXT    NOT NULL,
    -- The status name; a paused row also keeps its reason in pause_reason.
    status       TEXT    NOT NULL,
    pause_reason TEXT,
    cycle        INTEGER NOT NULL DEFAULT 0,
    budgets      TEXT    NOT NULL,
    pinned       TEXT    NOT NULL,
    parent_id    TEXT    REFERENCES operations(id),
    created_at   INTEGER NOT NULL,
    started_at   INTEGER,
    ended_at     INTEGER,
    updated_at   INTEGER NOT NULL,
    version      INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX operations_created ON operations (created_at DESC, id DESC);
CREATE INDEX operations_status  ON operations (status, created_at, id);

-- Which incidents an operation is about. No foreign key on incident_id: an incident may be
-- purged while the operation that studied it stays.
CREATE TABLE operation_incidents (
    operation_id TEXT    NOT NULL REFERENCES operations(id),
    incident_id  TEXT    NOT NULL,
    linked_at    INTEGER NOT NULL,
    PRIMARY KEY (operation_id, incident_id)
) STRICT;
CREATE INDEX operation_incidents_incident ON operation_incidents (incident_id, operation_id);

-- A step's intent never changes. Its progress (status and what follows) is a projection the
-- operation writer updates in the same transaction as the event behind it.
CREATE TABLE steps (
    id             TEXT    PRIMARY KEY,
    operation_id   TEXT    NOT NULL REFERENCES operations(id),
    ordinal        INTEGER NOT NULL,
    kind           TEXT    NOT NULL,
    phase          TEXT    NOT NULL,
    parent_step_id TEXT    REFERENCES steps(id),
    cycle          INTEGER NOT NULL,
    intent         TEXT    NOT NULL,
    created_at     INTEGER NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'pending',
    started_at     INTEGER,
    ended_at       INTEGER,
    summary        TEXT,
    reason         TEXT,
    output         TEXT,
    UNIQUE (operation_id, ordinal)
) STRICT;
CREATE INDEX steps_status ON steps (status, operation_id);
CREATE TRIGGER steps_intent_immutable
BEFORE UPDATE OF id, operation_id, ordinal, kind, phase, parent_step_id, cycle, intent, created_at ON steps
BEGIN
    SELECT RAISE(ABORT, 'a step''s intent is immutable');
END;

-- Edges run from a dependent step to a prerequisite, so an operation's graph cannot hold a
-- cycle: a prerequisite always has a lower ordinal in the same operation.
CREATE TABLE step_edges (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    from_step    TEXT NOT NULL REFERENCES steps(id),
    to_step      TEXT NOT NULL REFERENCES steps(id),
    kind         TEXT NOT NULL,
    PRIMARY KEY (from_step, to_step, kind),
    CHECK (from_step <> to_step)
) STRICT;
CREATE INDEX step_edges_operation ON step_edges (operation_id);
CREATE INDEX step_edges_to        ON step_edges (to_step);
CREATE TRIGGER step_edges_point_backwards
BEFORE INSERT ON step_edges
WHEN (SELECT operation_id FROM steps WHERE id = NEW.from_step) IS NOT NEW.operation_id
  OR (SELECT operation_id FROM steps WHERE id = NEW.to_step) IS NOT NEW.operation_id
  OR (SELECT ordinal FROM steps WHERE id = NEW.to_step) >= (SELECT ordinal FROM steps WHERE id = NEW.from_step)
BEGIN
    SELECT RAISE(ABORT, 'an edge must point to an earlier step of the same operation');
END;

-- What was approved and how far the run got, one row per Execution step. The row's id is the
-- step's id; it never equals an action id.
CREATE TABLE executions (
    execution_id         TEXT    PRIMARY KEY REFERENCES steps(id),
    operation_id         TEXT    NOT NULL REFERENCES operations(id),
    host_id              TEXT    NOT NULL,
    contract             TEXT    NOT NULL,
    contract_version     INTEGER NOT NULL,
    check_key            TEXT    NOT NULL,
    target               TEXT,
    args                 TEXT    NOT NULL,
    plan_hash            TEXT    NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    expires_at           INTEGER NOT NULL,
    authorization_kind   TEXT,
    approved_by          TEXT,
    approved_channel     TEXT,
    approved_at          INTEGER,
    approved_plan_hash   TEXT    CHECK (approved_plan_hash IS NULL OR (length(approved_plan_hash) = 64 AND approved_plan_hash NOT GLOB '*[^0-9a-f]*')),
    approval_session_ref TEXT,
    approval_client      TEXT,
    waiting_since        INTEGER,
    agent_epoch          TEXT,
    dispatched_at        INTEGER,
    settled_at           INTEGER,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    version              INTEGER NOT NULL DEFAULT 1,
    CHECK ((approved_by IS NULL) = (approved_at IS NULL)),
    CHECK ((agent_epoch IS NULL) = (dispatched_at IS NULL))
) STRICT;
CREATE INDEX executions_operation     ON executions (operation_id);
CREATE INDEX executions_host_contract ON executions (host_id, contract, dispatched_at);
