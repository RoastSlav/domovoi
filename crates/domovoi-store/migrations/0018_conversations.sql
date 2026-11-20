-- Operator conversations: one row per conversation and one per message. Message text lives only here,
-- never in the journal, so retention can delete it (messages are removed after retention.conversations_days).
CREATE TABLE conversations (
    id           TEXT PRIMARY KEY,
    version      INTEGER NOT NULL,
    title        TEXT NOT NULL CHECK (length(title) <= 400),
    scope_kind   TEXT NOT NULL,
    host_id      TEXT,
    operation_id TEXT,
    last_ordinal INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX conversations_updated   ON conversations (updated_at DESC, id);
CREATE INDEX conversations_host      ON conversations (host_id) WHERE host_id IS NOT NULL;
CREATE INDEX conversations_operation ON conversations (operation_id) WHERE operation_id IS NOT NULL;

CREATE TABLE messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    ordinal         INTEGER NOT NULL,
    role            TEXT NOT NULL,
    status          TEXT NOT NULL,
    turn_id         TEXT NOT NULL,
    text            TEXT NOT NULL DEFAULT '',
    failure         TEXT,
    failure_detail  TEXT,
    tools           TEXT NOT NULL DEFAULT '[]',
    created_at      INTEGER NOT NULL,
    completed_at    INTEGER,
    UNIQUE (conversation_id, ordinal)
) STRICT;
CREATE UNIQUE INDEX messages_turn_role ON messages (conversation_id, turn_id, role);
CREATE UNIQUE INDEX messages_one_pending ON messages (conversation_id) WHERE status = 'pending';
CREATE INDEX messages_created ON messages (created_at);
