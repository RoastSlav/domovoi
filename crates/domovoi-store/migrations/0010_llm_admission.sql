-- The persisted hourly model-run window: triage runs, follow-up questions and chat all
-- record a start here before a model call, so the hourly cap survives a restart.
CREATE TABLE llm_admission (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    purpose    TEXT NOT NULL CHECK (purpose IN ('triage', 'question')),
    started_at INTEGER NOT NULL
) STRICT;
CREATE INDEX llm_admission_started ON llm_admission (started_at);

-- `cost_micros` was NOT NULL, and SQLite cannot drop that constraint in place. Rebuild the
-- table so an unknown price is stored as NULL instead of a false zero, and add per-call
-- attribution: model, outcome, usage source and latency.
CREATE TABLE llm_usage_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    provider     TEXT NOT NULL,
    model        TEXT,
    outcome      TEXT NOT NULL DEFAULT 'ok',
    tokens_in    INTEGER NOT NULL,
    tokens_out   INTEGER NOT NULL,
    cost_micros  INTEGER,
    usage_source TEXT CHECK (usage_source IN ('reported', 'estimated', 'unknown')),
    latency_ms   INTEGER,
    at           INTEGER NOT NULL
) STRICT;
INSERT INTO llm_usage_new (id, provider, outcome, tokens_in, tokens_out, cost_micros, at)
    SELECT id, provider, 'ok', tokens_in, tokens_out, cost_micros, at FROM llm_usage;
DROP TABLE llm_usage;
ALTER TABLE llm_usage_new RENAME TO llm_usage;
CREATE INDEX llm_usage_at ON llm_usage (at);
