-- One block of one source on one host: the hub's record of the apply, confirm and rollback
-- phases. Phases, authorization kinds and flow states are validated in Rust, not by CHECK, so
-- a new value needs no migration.
CREATE TABLE containment_blocks (
    block_id           TEXT PRIMARY KEY,
    host_id            TEXT NOT NULL,
    operation_id       TEXT NOT NULL,
    step_id            TEXT NOT NULL,
    source             TEXT NOT NULL CHECK (length(source) > 0),
    ttl_s              INTEGER NOT NULL CHECK (ttl_s > 30),
    plan_hash          TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    phase              TEXT NOT NULL,
    authorization_kind TEXT NOT NULL,
    assurance          TEXT NOT NULL,
    confirm_execution  TEXT UNIQUE,
    rollback_execution TEXT UNIQUE,
    applied_at         INTEGER,
    deadline           INTEGER,
    existing_flows     TEXT,
    bridge_gap         INTEGER,
    failure_token      TEXT,
    failure_message    TEXT,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    version            INTEGER NOT NULL DEFAULT 1 CHECK (version > 0)
) STRICT;
CREATE INDEX containment_blocks_host ON containment_blocks (host_id, phase);
