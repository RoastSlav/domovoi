-- Rebuild advisories without the kind CHECK once, so a new advisory kind no longer needs a
-- table rebuild. Kinds stay validated by the program.
CREATE TABLE advisories_new (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    host_id     TEXT,
    title       TEXT NOT NULL,
    detail      TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    resolved_at INTEGER
) STRICT;
INSERT INTO advisories_new
SELECT id, kind, host_id, title, detail, created_at, resolved_at FROM advisories;
DROP TABLE advisories;
ALTER TABLE advisories_new RENAME TO advisories;
CREATE INDEX advisories_open ON advisories (resolved_at, created_at DESC);