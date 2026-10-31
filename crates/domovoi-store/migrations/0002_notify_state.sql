-- What the notification policy remembers per incident: card and thread references,
-- reminders, folded replies and alerts held for the end of quiet hours.
CREATE TABLE notify_state (
    incident_id TEXT PRIMARY KEY,
    held        INTEGER NOT NULL CHECK (held IN (0, 1)),
    state       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX notify_state_held ON notify_state (held, updated_at DESC);
