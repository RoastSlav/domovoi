-- Hash chain fields. NULL on rows written before the chain existed (the "legacy" rows).
ALTER TABLE audit_log ADD COLUMN prev_hash BLOB CHECK (prev_hash IS NULL OR length(prev_hash) = 32);
ALTER TABLE audit_log ADD COLUMN hash      BLOB CHECK (hash      IS NULL OR length(hash)      = 32);

-- One row, written by Store::open (the digest needs SHA-256, which SQLite lacks).
CREATE TABLE audit_anchor (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    install_id     TEXT    NOT NULL,
    legacy_last_id INTEGER NOT NULL,
    legacy_count   INTEGER NOT NULL,
    legacy_digest  BLOB    NOT NULL CHECK (length(legacy_digest) = 32),
    anchor_hash    BLOB    NOT NULL CHECK (length(anchor_hash) = 32),
    created_at     INTEGER NOT NULL
) STRICT;
CREATE TRIGGER audit_anchor_no_update BEFORE UPDATE ON audit_anchor BEGIN
    SELECT RAISE(ABORT, 'audit_anchor is append-only');
END;
CREATE TRIGGER audit_anchor_no_delete BEFORE DELETE ON audit_anchor BEGIN
    SELECT RAISE(ABORT, 'audit_anchor is append-only');
END;
