-- Knowledge store: typed facts, relations and owner-invented entities, plus the rename of
-- `digest_facts` (which holds metric samples) to `metric_samples`.
--
-- `digest_facts` stays as a view with an INSTEAD OF INSERT trigger for one release so the
-- previous binary keeps working. A later migration removes the view and trigger and raises
-- MIN_READER.

CREATE TABLE metric_samples (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id     TEXT NOT NULL,
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    value       TEXT NOT NULL,
    unit        TEXT,
    observed_at INTEGER NOT NULL
) STRICT;
INSERT INTO metric_samples (id, host_id, key, label, value, unit, observed_at)
    SELECT id, host_id, key, label, value, unit, observed_at FROM digest_facts ORDER BY id;
CREATE INDEX metric_samples_host_observed ON metric_samples (host_id, observed_at, id);
DROP TABLE digest_facts;
CREATE VIEW digest_facts AS
    SELECT id, host_id, key, label, value, unit, observed_at FROM metric_samples;
CREATE TRIGGER digest_facts_insert INSTEAD OF INSERT ON digest_facts BEGIN
    INSERT INTO metric_samples (host_id, key, label, value, unit, observed_at)
    VALUES (new.host_id, new.key, new.label, new.value, new.unit, new.observed_at);
END;

-- Subjects the owner invents ("VPN", "NAS pool"). Never inventory.
CREATE TABLE entities (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE UNIQUE INDEX entities_name ON entities (lower(name));

-- Typed facts. Enums (predicate, source, status) are validated in Rust, never with CHECK.
-- `live` is derived in Rust (status is current, stale or disputed). `value` and `search_text`
-- are NULL after the retention purge.
CREATE TABLE facts (
    id                TEXT PRIMARY KEY,
    subject_ref       TEXT NOT NULL,
    host_id           TEXT,
    predicate         TEXT NOT NULL,
    value             TEXT,
    search_text       TEXT,
    live              INTEGER NOT NULL,
    source            TEXT NOT NULL,
    status            TEXT NOT NULL,
    observed_at       INTEGER NOT NULL,
    valid_until       INTEGER,
    last_verified_at  INTEGER,
    supersedes_id     TEXT REFERENCES facts (id),
    evidence_ref      TEXT,
    actor             TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    retracted_at      INTEGER,
    retracted_by      TEXT,
    content_purged_at INTEGER,
    version           INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX facts_subject ON facts (subject_ref, predicate, live);
CREATE INDEX facts_host ON facts (host_id, predicate, live);
CREATE INDEX facts_retracted ON facts (status, retracted_at);
CREATE INDEX facts_created ON facts (created_at DESC, id DESC);
-- One live row per subject, predicate and canonical value (race protection for the service).
CREATE UNIQUE INDEX facts_one_live_value ON facts (subject_ref, predicate, value)
    WHERE live = 1 AND value IS NOT NULL;

CREATE VIRTUAL TABLE facts_fts USING fts5 (
    subject_ref, predicate, search_text,
    content = 'facts', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
CREATE TRIGGER facts_fts_insert AFTER INSERT ON facts BEGIN
    INSERT INTO facts_fts (rowid, subject_ref, predicate, search_text)
    SELECT new.rowid, new.subject_ref, new.predicate, new.search_text WHERE new.live = 1;
END;
CREATE TRIGGER facts_fts_update AFTER UPDATE ON facts BEGIN
    INSERT INTO facts_fts (facts_fts, rowid, subject_ref, predicate, search_text)
    SELECT 'delete', old.rowid, old.subject_ref, old.predicate, old.search_text WHERE old.live = 1;
    INSERT INTO facts_fts (rowid, subject_ref, predicate, search_text)
    SELECT new.rowid, new.subject_ref, new.predicate, new.search_text WHERE new.live = 1;
END;
CREATE TRIGGER facts_fts_delete AFTER DELETE ON facts BEGIN
    INSERT INTO facts_fts (facts_fts, rowid, subject_ref, predicate, search_text)
    SELECT 'delete', old.rowid, old.subject_ref, old.predicate, old.search_text WHERE old.live = 1;
END;

CREATE TABLE relations (
    id                TEXT PRIMARY KEY,
    from_ref          TEXT,
    to_ref            TEXT,
    kind              TEXT NOT NULL,
    source            TEXT NOT NULL,
    status            TEXT NOT NULL,
    live              INTEGER NOT NULL,
    observed_at       INTEGER NOT NULL,
    evidence_ref      TEXT,
    actor             TEXT NOT NULL,
    created_at        INTEGER NOT NULL,
    retracted_at      INTEGER,
    retracted_by      TEXT,
    content_purged_at INTEGER,
    version           INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE UNIQUE INDEX relations_one_live_edge ON relations (from_ref, kind, to_ref) WHERE live = 1;
CREATE INDEX relations_from ON relations (from_ref, kind);
CREATE INDEX relations_to ON relations (to_ref, kind);
CREATE INDEX relations_retracted ON relations (status, retracted_at);
CREATE INDEX relations_created ON relations (created_at DESC, id DESC);

-- The owner's confirmation of a management path. `evidence` is opaque here.
CREATE TABLE management_path_confirmations (
    fact_id      TEXT PRIMARY KEY REFERENCES facts (id),
    host_id      TEXT NOT NULL,
    confirmed_by TEXT NOT NULL,
    confirmed_at INTEGER NOT NULL,
    evidence     TEXT NOT NULL
) STRICT;
CREATE INDEX management_path_confirmations_host ON management_path_confirmations (host_id);
