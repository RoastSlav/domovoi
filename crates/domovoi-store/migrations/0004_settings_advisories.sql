-- Advisories for secrets that cannot be decrypted and for settings that could not be
-- applied.
CREATE TABLE advisories_new (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN (
        'heartbeat_missing', 'triage_degraded', 'stale_image', 'stack_without_source',
        'secret_exposed', 'baseline_entry_expired', 'agent_outdated', 'delivery_failed',
        'stored_secret_unreadable', 'settings_apply_failed'
    )),
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
