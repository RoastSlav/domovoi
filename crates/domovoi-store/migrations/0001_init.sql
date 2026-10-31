-- Domovoi v0.1 schema. Append-only once merged: change it with a new migration file.
--
-- Conventions: ids are TEXT (UUID, hyphenated), timestamps are INTEGER unix milliseconds
-- UTC, hashes are 32-byte BLOBs (fingerprints and plan hashes are lowercase hex TEXT),
-- structured fields are TEXT JSON, enums are TEXT with CHECK constraints.

CREATE TABLE hosts (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    kind       TEXT NOT NULL CHECK (kind IN ('Local', 'Remote', 'External')),
    labels     TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE agents (
    agent_id    TEXT PRIMARY KEY,
    host_id     TEXT NOT NULL REFERENCES hosts (id),
    public_key  BLOB NOT NULL CHECK (length(public_key) = 32),
    version     TEXT NOT NULL,
    enrolled_at INTEGER NOT NULL,
    revoked_at  INTEGER
) STRICT;
-- A host has at most one agent that is not revoked.
CREATE UNIQUE INDEX agents_one_live_per_host ON agents (host_id) WHERE revoked_at IS NULL;

CREATE TABLE enrollment_tokens (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    host_id    TEXT NOT NULL REFERENCES hosts (id),
    expires_at INTEGER NOT NULL,
    used_at    INTEGER
) STRICT;

-- Incidents keep their full state as JSON in `body`; the other columns index it.
CREATE TABLE incidents (
    id           TEXT PRIMARY KEY,
    fingerprint  TEXT NOT NULL,
    host_id      TEXT NOT NULL,
    check_key    TEXT NOT NULL,
    target       TEXT,
    title        TEXT NOT NULL,
    summary_text TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL CHECK (status IN ('pending', 'open', 'acked', 'muted', 'resolved')),
    severity     TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    opened_at    INTEGER NOT NULL,
    resolved_at  INTEGER,
    body         TEXT NOT NULL
) STRICT;
-- At most one active incident per fingerprint.
CREATE UNIQUE INDEX incidents_one_active_per_fingerprint
    ON incidents (fingerprint) WHERE status <> 'resolved';
CREATE INDEX incidents_fingerprint_resolved ON incidents (fingerprint, resolved_at);
CREATE INDEX incidents_opened ON incidents (opened_at DESC, id DESC);
CREATE INDEX incidents_host_opened ON incidents (host_id, opened_at DESC, id DESC);
CREATE INDEX incidents_status ON incidents (status);

CREATE VIRTUAL TABLE incidents_fts USING fts5 (
    title, summary_text,
    content = 'incidents', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
CREATE TRIGGER incidents_fts_insert AFTER INSERT ON incidents BEGIN
    INSERT INTO incidents_fts (rowid, title, summary_text)
    VALUES (new.rowid, new.title, new.summary_text);
END;
CREATE TRIGGER incidents_fts_delete AFTER DELETE ON incidents BEGIN
    INSERT INTO incidents_fts (incidents_fts, rowid, title, summary_text)
    VALUES ('delete', old.rowid, old.title, old.summary_text);
END;
CREATE TRIGGER incidents_fts_update AFTER UPDATE ON incidents BEGIN
    INSERT INTO incidents_fts (incidents_fts, rowid, title, summary_text)
    VALUES ('delete', old.rowid, old.title, old.summary_text);
    INSERT INTO incidents_fts (rowid, title, summary_text)
    VALUES (new.rowid, new.title, new.summary_text);
END;

-- Signals keep their full content as JSON in `body`.
CREATE TABLE signals (
    id          TEXT PRIMARY KEY,
    host_id     TEXT NOT NULL,
    check_key   TEXT NOT NULL,
    target      TEXT,
    status      TEXT NOT NULL CHECK (status IN ('firing', 'resolved')),
    severity    TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    observed_at INTEGER NOT NULL,
    body        TEXT NOT NULL
) STRICT;
CREATE INDEX signals_observed ON signals (observed_at);

CREATE TABLE incident_signals (
    incident_id TEXT NOT NULL,
    signal_id   TEXT NOT NULL REFERENCES signals (id) ON DELETE CASCADE,
    PRIMARY KEY (incident_id, signal_id)
) STRICT;
CREATE INDEX incident_signals_signal ON incident_signals (signal_id);

-- The timeline, in append order.
CREATE TABLE incident_events (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    incident_id TEXT NOT NULL REFERENCES incidents (id) ON DELETE CASCADE,
    at          INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    summary     TEXT NOT NULL,
    detail      TEXT NOT NULL DEFAULT 'null'
) STRICT;
CREATE INDEX incident_events_incident ON incident_events (incident_id, seq);
CREATE INDEX incident_events_at ON incident_events (at);

CREATE TABLE actions (
    id               TEXT PRIMARY KEY,
    incident_id      TEXT NOT NULL,
    host_id          TEXT NOT NULL,
    skill            TEXT NOT NULL,
    risk             TEXT NOT NULL CHECK (risk IN ('read', 'write', 'destructive')),
    args             TEXT NOT NULL,
    plan_hash        TEXT NOT NULL,
    status           TEXT NOT NULL,
    proposed_by_kind TEXT NOT NULL CHECK (proposed_by_kind IN ('human', 'system')),
    proposed_by      TEXT NOT NULL,
    approved_by      TEXT,
    approved_channel TEXT,
    approved_at      INTEGER,
    expires_at       INTEGER NOT NULL,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    chat_token_hash  BLOB UNIQUE CHECK (chat_token_hash IS NULL OR length(chat_token_hash) = 32),
    CHECK ((approved_by IS NULL) = (approved_at IS NULL))
) STRICT;
CREATE INDEX actions_created ON actions (created_at DESC, id DESC);
CREATE INDEX actions_incident ON actions (incident_id, created_at DESC, id DESC);
CREATE INDEX actions_host ON actions (host_id, created_at DESC, id DESC);

-- Append-only (INV-7): the triggers below reject every UPDATE and DELETE.
CREATE TABLE audit_log (
    seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    at      INTEGER NOT NULL,
    actor   TEXT NOT NULL,
    kind    TEXT NOT NULL,
    subject TEXT NOT NULL,
    detail  TEXT NOT NULL DEFAULT 'null'
) STRICT;
CREATE INDEX audit_log_actor ON audit_log (actor, seq);
CREATE INDEX audit_log_kind ON audit_log (kind, seq);
CREATE INDEX audit_log_subject ON audit_log (subject, seq);
CREATE INDEX audit_log_at ON audit_log (at);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;
-- `INSERT OR REPLACE` deletes the old row without firing delete triggers, so an insert
-- that reuses an existing sequence number is refused too.
CREATE TRIGGER audit_log_no_replace BEFORE INSERT ON audit_log
WHEN EXISTS (SELECT 1 FROM audit_log WHERE seq = new.seq) BEGIN
    SELECT RAISE(ABORT, 'audit_log is append-only');
END;

CREATE TABLE ack_rules (
    id             TEXT PRIMARY KEY,
    host_id        TEXT,
    check_key      TEXT NOT NULL,
    target_pattern TEXT,
    predicates     TEXT NOT NULL DEFAULT '[]',
    note           TEXT NOT NULL,
    expires_at     INTEGER
) STRICT;

CREATE TABLE maintenance_windows (
    host_id   TEXT NOT NULL,
    starts_at INTEGER NOT NULL,
    ends_at   INTEGER NOT NULL,
    note      TEXT NOT NULL,
    UNIQUE (host_id, starts_at, ends_at, note)
) STRICT;
CREATE INDEX maintenance_windows_ends ON maintenance_windows (ends_at);

CREATE TABLE baselines (
    host_id      TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('container', 'unit', 'port')),
    key          TEXT NOT NULL,
    criticality  TEXT NOT NULL CHECK (criticality IN ('critical', 'warning')),
    learned_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    managed      INTEGER NOT NULL CHECK (managed IN (0, 1)),
    PRIMARY KEY (host_id, kind, key)
) STRICT;

CREATE TABLE notes (
    id         TEXT PRIMARY KEY,
    host_id    TEXT,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',
    updated_at INTEGER NOT NULL
) STRICT;
CREATE INDEX notes_host ON notes (host_id, updated_at DESC);

CREATE VIRTUAL TABLE notes_fts USING fts5 (
    title, body, tags,
    content = 'notes', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
CREATE TRIGGER notes_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts (rowid, title, body, tags)
    VALUES (new.rowid, new.title, new.body, new.tags);
END;
CREATE TRIGGER notes_fts_delete AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body, tags)
    VALUES ('delete', old.rowid, old.title, old.body, old.tags);
END;
CREATE TRIGGER notes_fts_update AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts (notes_fts, rowid, title, body, tags)
    VALUES ('delete', old.rowid, old.title, old.body, old.tags);
    INSERT INTO notes_fts (rowid, title, body, tags)
    VALUES (new.rowid, new.title, new.body, new.tags);
END;

CREATE TABLE triage_cache (
    fingerprint     TEXT NOT NULL,
    evidence_digest BLOB NOT NULL CHECK (length(evidence_digest) = 32),
    report          TEXT NOT NULL,
    stored_at       INTEGER NOT NULL,
    PRIMARY KEY (fingerprint, evidence_digest)
) STRICT;
CREATE INDEX triage_cache_stored ON triage_cache (stored_at);

CREATE TABLE llm_usage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    provider    TEXT NOT NULL,
    tokens_in   INTEGER NOT NULL,
    tokens_out  INTEGER NOT NULL,
    cost_micros INTEGER NOT NULL,
    at          INTEGER NOT NULL
) STRICT;
CREATE INDEX llm_usage_at ON llm_usage (at);

CREATE TABLE digest_facts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id     TEXT NOT NULL,
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    value       TEXT NOT NULL,
    unit        TEXT,
    observed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX digest_facts_host_observed ON digest_facts (host_id, observed_at, id);

CREATE TABLE advisories (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN (
        'heartbeat_missing', 'triage_degraded', 'stale_image', 'stack_without_source',
        'secret_exposed', 'baseline_entry_expired', 'agent_outdated'
    )),
    host_id     TEXT,
    title       TEXT NOT NULL,
    detail      TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    resolved_at INTEGER
) STRICT;
CREATE INDEX advisories_open ON advisories (resolved_at, created_at DESC);

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at    INTEGER NOT NULL
) STRICT;

CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    user_id    TEXT NOT NULL,
    csrf_hash  BLOB NOT NULL CHECK (length(csrf_hash) = 32),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX sessions_user ON sessions (user_id);
CREATE INDEX sessions_expires ON sessions (expires_at);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
