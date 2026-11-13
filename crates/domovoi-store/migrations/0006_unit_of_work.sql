-- Optimistic versions for the aggregates the unit of work writes with a version check.
ALTER TABLE incidents ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE notes     ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

-- The operational journal. Written only through the unit of work. Not trigger-protected:
-- retention deletes old rows. Rows outlive the aggregate they describe.
CREATE TABLE journal (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    aggregate_type   TEXT    NOT NULL,
    aggregate_id     TEXT    NOT NULL,
    kind             TEXT    NOT NULL,
    payload_v        INTEGER NOT NULL,
    payload          TEXT    NOT NULL,
    actor            TEXT    NOT NULL,
    occurred_at      INTEGER NOT NULL,
    -- incident_events.seq this row mirrors during the dual-write period; NULL otherwise.
    legacy_event_seq INTEGER
) STRICT;
CREATE INDEX journal_aggregate ON journal (aggregate_type, aggregate_id, seq);
CREATE INDEX journal_occurred  ON journal (occurred_at);
CREATE UNIQUE INDEX journal_legacy_event ON journal (legacy_event_seq)
    WHERE legacy_event_seq IS NOT NULL;

-- The work outbox. Lifecycle columns exist now; the lifecycle itself is implemented later.
CREATE TABLE effects (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    id               TEXT    NOT NULL UNIQUE,
    kind             TEXT    NOT NULL,
    aggregate_type   TEXT    NOT NULL,
    aggregate_id     TEXT    NOT NULL,
    payload          TEXT    NOT NULL,
    status           TEXT    NOT NULL DEFAULT 'pending',
    not_before       INTEGER NOT NULL,
    attempt          INTEGER NOT NULL DEFAULT 0,
    max_attempts     INTEGER NOT NULL,
    dedup_key        TEXT,
    claim_token      TEXT,
    claim_generation INTEGER NOT NULL DEFAULT 0,
    lease_until      INTEGER,
    runner_id        TEXT,
    last_error       TEXT,
    created_at       INTEGER NOT NULL,
    completed_at     INTEGER
) STRICT;
CREATE INDEX effects_due ON effects (status, not_before, seq);
CREATE INDEX effects_aggregate ON effects (aggregate_type, aggregate_id);
CREATE UNIQUE INDEX effects_dedup ON effects (dedup_key)
    WHERE dedup_key IS NOT NULL AND status IN ('pending', 'claimed');

-- The lowest binary version that can read this schema. One row, id = 1, written by Store::open.
CREATE TABLE schema_compat (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    min_reader  TEXT    NOT NULL,
    written_by  TEXT    NOT NULL,
    written_at  INTEGER NOT NULL
) STRICT;