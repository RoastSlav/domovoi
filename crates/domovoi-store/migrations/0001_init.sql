-- The Domovoi schema. Migrations are immutable from the first release (ADR 0039): every later
-- change is a new numbered file next to this one.

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
, reported_protocol      TEXT, reported_release       TEXT, last_authenticated_at  INTEGER, incompatible_since     INTEGER) STRICT;
CREATE UNIQUE INDEX agents_one_live_per_host ON agents (host_id) WHERE revoked_at IS NULL;
CREATE TABLE enrollment_tokens (
    token_hash BLOB PRIMARY KEY CHECK (length(token_hash) = 32),
    host_id    TEXT NOT NULL REFERENCES hosts (id),
    expires_at INTEGER NOT NULL,
    used_at    INTEGER
) STRICT;
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
, version INTEGER NOT NULL DEFAULT 1) STRICT;
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
/* incidents_fts(title,summary_text) */
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
CREATE TABLE audit_log (
    seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    at      INTEGER NOT NULL,
    actor   TEXT NOT NULL,
    kind    TEXT NOT NULL,
    subject TEXT NOT NULL,
    detail  TEXT NOT NULL DEFAULT 'null'
, prev_hash BLOB CHECK (prev_hash IS NULL OR length(prev_hash) = 32), hash      BLOB CHECK (hash      IS NULL OR length(hash)      = 32)) STRICT;
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
, created_at INTEGER, created_by TEXT) STRICT;
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
, version INTEGER NOT NULL DEFAULT 1) STRICT;
CREATE INDEX notes_host ON notes (host_id, updated_at DESC);
CREATE VIRTUAL TABLE notes_fts USING fts5 (
    title, body, tags,
    content = 'notes', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
/* notes_fts(title,body,tags) */
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
CREATE TABLE notify_state (
    incident_id TEXT PRIMARY KEY,
    held        INTEGER NOT NULL CHECK (held IN (0, 1)),
    state       TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX notify_state_held ON notify_state (held, updated_at DESC);
CREATE TABLE journal (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    aggregate_type   TEXT    NOT NULL,
    aggregate_id     TEXT    NOT NULL,
    kind             TEXT    NOT NULL,
    payload_v        INTEGER NOT NULL,
    payload          TEXT    NOT NULL,
    actor            TEXT    NOT NULL,
    occurred_at      INTEGER NOT NULL,
    -- incident_events.seq this row mirrors; NULL for rows that mirror no incident event.
    incident_event_seq INTEGER
) STRICT;
CREATE INDEX journal_aggregate ON journal (aggregate_type, aggregate_id, seq);
CREATE INDEX journal_occurred  ON journal (occurred_at);
CREATE UNIQUE INDEX journal_incident_event ON journal (incident_event_seq)
    WHERE incident_event_seq IS NOT NULL;
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
CREATE TABLE schema_compat (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    min_reader  TEXT    NOT NULL,
    written_by  TEXT    NOT NULL,
    written_at  INTEGER NOT NULL
) STRICT;
CREATE TABLE "advisories" (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    host_id     TEXT,
    title       TEXT NOT NULL,
    detail      TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    resolved_at INTEGER
) STRICT;
CREATE INDEX advisories_open ON advisories (resolved_at, created_at DESC);
CREATE TABLE audit_anchor (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    install_id     TEXT    NOT NULL,
    anchor_hash    BLOB    NOT NULL CHECK (length(anchor_hash) = 32),
    created_at     INTEGER NOT NULL
) STRICT;
CREATE TRIGGER audit_anchor_no_update BEFORE UPDATE ON audit_anchor BEGIN
    SELECT RAISE(ABORT, 'audit_anchor is append-only');
END;
CREATE TRIGGER audit_anchor_no_delete BEFORE DELETE ON audit_anchor BEGIN
    SELECT RAISE(ABORT, 'audit_anchor is append-only');
END;
CREATE TABLE llm_admission (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    purpose    TEXT NOT NULL CHECK (purpose IN ('triage', 'question')),
    started_at INTEGER NOT NULL
) STRICT;
CREATE INDEX llm_admission_started ON llm_admission (started_at);
CREATE TABLE "llm_usage" (
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
CREATE INDEX llm_usage_at ON llm_usage (at);
CREATE TABLE operations (
    id           TEXT    PRIMARY KEY,
    kind         TEXT    NOT NULL,
    goal         TEXT    NOT NULL,
    -- The status name; a paused row also keeps its reason in pause_reason.
    status       TEXT    NOT NULL,
    pause_reason TEXT,
    cycle        INTEGER NOT NULL DEFAULT 0,
    budgets      TEXT    NOT NULL,
    pinned       TEXT    NOT NULL,
    parent_id    TEXT    REFERENCES operations(id),
    created_at   INTEGER NOT NULL,
    started_at   INTEGER,
    ended_at     INTEGER,
    updated_at   INTEGER NOT NULL,
    version      INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX operations_created ON operations (created_at DESC, id DESC);
CREATE INDEX operations_status  ON operations (status, created_at, id);
CREATE TABLE operation_incidents (
    operation_id TEXT    NOT NULL REFERENCES operations(id),
    incident_id  TEXT    NOT NULL,
    linked_at    INTEGER NOT NULL,
    PRIMARY KEY (operation_id, incident_id)
) STRICT;
CREATE INDEX operation_incidents_incident ON operation_incidents (incident_id, operation_id);
CREATE TABLE steps (
    id             TEXT    PRIMARY KEY,
    operation_id   TEXT    NOT NULL REFERENCES operations(id),
    ordinal        INTEGER NOT NULL,
    kind           TEXT    NOT NULL,
    phase          TEXT    NOT NULL,
    parent_step_id TEXT    REFERENCES steps(id),
    cycle          INTEGER NOT NULL,
    intent         TEXT    NOT NULL,
    created_at     INTEGER NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'pending',
    started_at     INTEGER,
    ended_at       INTEGER,
    summary        TEXT,
    reason         TEXT,
    output         TEXT,
    UNIQUE (operation_id, ordinal)
) STRICT;
CREATE INDEX steps_status ON steps (status, operation_id);
CREATE TRIGGER steps_intent_immutable
BEFORE UPDATE OF id, operation_id, ordinal, kind, phase, parent_step_id, cycle, intent, created_at ON steps
BEGIN
    SELECT RAISE(ABORT, 'a step''s intent is immutable');
END;
CREATE TABLE step_edges (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    from_step    TEXT NOT NULL REFERENCES steps(id),
    to_step      TEXT NOT NULL REFERENCES steps(id),
    kind         TEXT NOT NULL,
    PRIMARY KEY (from_step, to_step, kind),
    CHECK (from_step <> to_step)
) STRICT;
CREATE INDEX step_edges_operation ON step_edges (operation_id);
CREATE INDEX step_edges_to        ON step_edges (to_step);
CREATE TRIGGER step_edges_point_backwards
BEFORE INSERT ON step_edges
WHEN (SELECT operation_id FROM steps WHERE id = NEW.from_step) IS NOT NEW.operation_id
  OR (SELECT operation_id FROM steps WHERE id = NEW.to_step) IS NOT NEW.operation_id
  OR (SELECT ordinal FROM steps WHERE id = NEW.to_step) >= (SELECT ordinal FROM steps WHERE id = NEW.from_step)
BEGIN
    SELECT RAISE(ABORT, 'an edge must point to an earlier step of the same operation');
END;
CREATE TABLE executions (
    execution_id         TEXT    PRIMARY KEY REFERENCES steps(id),
    operation_id         TEXT    NOT NULL REFERENCES operations(id),
    host_id              TEXT    NOT NULL,
    contract             TEXT    NOT NULL,
    contract_version     INTEGER NOT NULL,
    check_key            TEXT    NOT NULL,
    target               TEXT,
    args                 TEXT    NOT NULL,
    plan_hash            TEXT    NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    expires_at           INTEGER NOT NULL,
    authorization_kind   TEXT,
    approved_by          TEXT,
    approved_channel     TEXT,
    approved_at          INTEGER,
    approved_plan_hash   TEXT    CHECK (approved_plan_hash IS NULL OR (length(approved_plan_hash) = 64 AND approved_plan_hash NOT GLOB '*[^0-9a-f]*')),
    approval_session_ref TEXT,
    approval_client      TEXT,
    waiting_since        INTEGER,
    agent_epoch          TEXT,
    dispatched_at        INTEGER,
    settled_at           INTEGER,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    version              INTEGER NOT NULL DEFAULT 1, chat_token_hash BLOB
    CHECK (chat_token_hash IS NULL OR length(chat_token_hash) = 32), risk TEXT,
    CHECK ((approved_by IS NULL) = (approved_at IS NULL)),
    CHECK ((agent_epoch IS NULL) = (dispatched_at IS NULL))
) STRICT;
CREATE INDEX executions_operation     ON executions (operation_id);
CREATE INDEX executions_host_contract ON executions (host_id, contract, dispatched_at);
CREATE TABLE journal_floor (
    id                 INTEGER PRIMARY KEY CHECK (id = 1),
    pruned_through_seq INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX executions_chat_token ON executions (chat_token_hash)
    WHERE chat_token_hash IS NOT NULL;
CREATE TABLE policy_versions (
    id             TEXT PRIMARY KEY,
    status         TEXT NOT NULL,
    number         INTEGER,
    draft_revision INTEGER NOT NULL DEFAULT 1,
    based_on       INTEGER,
    rules_hash     TEXT NOT NULL CHECK (length(rules_hash) = 64 AND rules_hash NOT GLOB '*[^0-9a-f]*'),
    rule_count     INTEGER NOT NULL,
    locked_count   INTEGER NOT NULL,
    shadow_count   INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    created_by     TEXT NOT NULL,
    updated_at     INTEGER NOT NULL,
    updated_by     TEXT NOT NULL,
    published_at   INTEGER,
    published_by   TEXT,
    CHECK ((status = 'published') = (number IS NOT NULL)),
    CHECK ((status = 'published') = (published_at IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX policy_versions_number    ON policy_versions (number) WHERE number IS NOT NULL;
CREATE UNIQUE INDEX policy_versions_one_draft ON policy_versions (status) WHERE status = 'draft';
CREATE TABLE policy_rules (
    version_id TEXT NOT NULL REFERENCES policy_versions(id),
    rule_id    TEXT NOT NULL,
    ordinal    INTEGER NOT NULL,
    enabled    INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    shadow     INTEGER NOT NULL CHECK (shadow  IN (0, 1)),
    locked     INTEGER NOT NULL CHECK (locked  IN (0, 1)),
    body       TEXT NOT NULL,
    PRIMARY KEY (version_id, rule_id),
    UNIQUE (version_id, ordinal)
) STRICT;
CREATE TRIGGER policy_rules_frozen_when_published
BEFORE UPDATE ON policy_rules
WHEN (SELECT status FROM policy_versions WHERE id = OLD.version_id) = 'published'
BEGIN
    SELECT RAISE(ABORT, 'a published policy version is immutable');
END;
CREATE TRIGGER policy_rules_no_delete_when_published
BEFORE DELETE ON policy_rules
WHEN (SELECT status FROM policy_versions WHERE id = OLD.version_id) = 'published'
BEGIN
    SELECT RAISE(ABORT, 'a published policy version is immutable');
END;
CREATE TABLE policy_acknowledgements (
    version_id      TEXT NOT NULL REFERENCES policy_versions(id),
    finding_key     TEXT NOT NULL,
    kind            TEXT NOT NULL,
    first_rule      TEXT NOT NULL,
    second_rule     TEXT NOT NULL,
    resolution      TEXT,
    note            TEXT NOT NULL,
    acknowledged_by TEXT NOT NULL,
    acknowledged_at INTEGER NOT NULL,
    PRIMARY KEY (version_id, finding_key)
) STRICT;
CREATE TABLE policy_shadow_decisions (
    execution_id     TEXT PRIMARY KEY,
    operation_id     TEXT NOT NULL,
    host_id          TEXT NOT NULL,
    contract         TEXT NOT NULL,
    step_created_at  INTEGER NOT NULL,
    evaluated_at     INTEGER NOT NULL,
    status           TEXT NOT NULL,
    reason           TEXT,
    policy_version   INTEGER NOT NULL,
    rules_hash       TEXT NOT NULL,
    enforced_outcome TEXT,
    shadow_outcome   TEXT,
    degraded         INTEGER NOT NULL CHECK (degraded IN (0, 1)),
    candidate        TEXT NOT NULL,
    inputs           TEXT,
    enforced         TEXT,
    shadow           TEXT,
    gaps             TEXT NOT NULL
) STRICT;
CREATE INDEX policy_shadow_evaluated ON policy_shadow_decisions (evaluated_at DESC, execution_id DESC);
CREATE INDEX policy_shadow_host      ON policy_shadow_decisions (host_id, contract, evaluated_at);
CREATE INDEX policy_shadow_version   ON policy_shadow_decisions (policy_version, evaluated_at);
CREATE TABLE policy_shadow_state (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    last_sweep_at     INTEGER,
    last_error        TEXT,
    total_evaluated   INTEGER NOT NULL DEFAULT 0,
    total_unevaluable INTEGER NOT NULL DEFAULT 0,
    total_skipped     INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE metric_samples (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id     TEXT NOT NULL,
    key         TEXT NOT NULL,
    label       TEXT NOT NULL,
    value       TEXT NOT NULL,
    unit        TEXT,
    observed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX metric_samples_host_observed ON metric_samples (host_id, observed_at, id);
CREATE VIEW digest_facts AS
    SELECT id, host_id, key, label, value, unit, observed_at FROM metric_samples;
/* digest_facts(id,host_id,"key",label,value,unit,observed_at) */
CREATE TRIGGER digest_facts_insert INSTEAD OF INSERT ON digest_facts BEGIN
    INSERT INTO metric_samples (host_id, key, label, value, unit, observed_at)
    VALUES (new.host_id, new.key, new.label, new.value, new.unit, new.observed_at);
END;
CREATE TABLE entities (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE UNIQUE INDEX entities_name ON entities (lower(name));
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
CREATE UNIQUE INDEX facts_one_live_value ON facts (subject_ref, predicate, value)
    WHERE live = 1 AND value IS NOT NULL;
CREATE VIRTUAL TABLE facts_fts USING fts5 (
    subject_ref, predicate, search_text,
    content = 'facts', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
/* facts_fts(subject_ref,predicate,search_text) */
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
CREATE TABLE management_path_confirmations (
    fact_id      TEXT PRIMARY KEY REFERENCES facts (id),
    host_id      TEXT NOT NULL,
    confirmed_by TEXT NOT NULL,
    confirmed_at INTEGER NOT NULL,
    evidence     TEXT NOT NULL
) STRICT;
CREATE INDEX management_path_confirmations_host ON management_path_confirmations (host_id);
CREATE TABLE resolution_summary_fields (
    operation_id TEXT    NOT NULL REFERENCES operations(id),
    field        TEXT    NOT NULL,                 -- a summary field name, validated in Rust
    revision     INTEGER NOT NULL,                 -- 1-based, dense per (operation_id, field)
    value        TEXT    NOT NULL,                 -- canonical JSON, redacted before it gets here
    source       TEXT    NOT NULL,                 -- 'code' | 'model' | 'user', validated in Rust
    actor        TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (operation_id, field, revision),
    CHECK (revision >= 1),
    CHECK (length(value) <= 16384)
) STRICT;
CREATE TRIGGER resolution_summary_fields_append_only_update
BEFORE UPDATE ON resolution_summary_fields
BEGIN
    SELECT RAISE(ABORT, 'summary revisions are append-only');
END;
CREATE TRIGGER resolution_summary_fields_append_only_delete
BEFORE DELETE ON resolution_summary_fields
BEGIN
    SELECT RAISE(ABORT, 'summary revisions are append-only');
END;
CREATE TABLE operation_resolutions (
    operation_id TEXT    PRIMARY KEY REFERENCES operations(id),
    goal         TEXT    NOT NULL,                 -- copied from operations.goal (already redacted)
    outcome      TEXT    NOT NULL,
    verified     INTEGER NOT NULL DEFAULT 0,       -- 0 or 1
    problem      TEXT    NOT NULL DEFAULT '',      -- newest revision's text, '' when none
    cause        TEXT    NOT NULL DEFAULT '',
    fix          TEXT    NOT NULL DEFAULT '',
    subjects     TEXT    NOT NULL DEFAULT '',      -- words of the pinned subjects
    contracts    TEXT    NOT NULL DEFAULT '',      -- words of the executed contracts
    tags         TEXT    NOT NULL DEFAULT '',      -- space-separated outcome words
    ended_at     INTEGER NOT NULL,                 -- copied from operations.ended_at
    updated_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX operation_resolutions_ended ON operation_resolutions (ended_at DESC, operation_id DESC);
CREATE TABLE operation_resolution_hosts (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    host_id      TEXT NOT NULL,
    PRIMARY KEY (operation_id, host_id)
) STRICT;
CREATE INDEX operation_resolution_hosts_host ON operation_resolution_hosts (host_id, operation_id);
CREATE TABLE operation_resolution_subjects (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    host_id      TEXT NOT NULL,
    check_key    TEXT NOT NULL,
    target       TEXT NOT NULL DEFAULT '',         -- '' when the subject has no target
    PRIMARY KEY (operation_id, host_id, check_key, target)
) STRICT;
CREATE INDEX operation_resolution_subjects_lookup
    ON operation_resolution_subjects (host_id, check_key, target, operation_id);
CREATE VIRTUAL TABLE operation_resolutions_fts USING fts5 (
    goal, problem, cause, fix, subjects, contracts, tags,
    content = 'operation_resolutions', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 0'
);
/* operation_resolutions_fts(goal,problem,cause,fix,subjects,contracts,tags) */
CREATE TRIGGER operation_resolutions_fts_insert AFTER INSERT ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES (new.rowid, new.goal, new.problem, new.cause, new.fix, new.subjects, new.contracts, new.tags);
END;
CREATE TRIGGER operation_resolutions_fts_delete AFTER DELETE ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (operation_resolutions_fts, rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES ('delete', old.rowid, old.goal, old.problem, old.cause, old.fix, old.subjects, old.contracts, old.tags);
END;
CREATE TRIGGER operation_resolutions_fts_update AFTER UPDATE ON operation_resolutions BEGIN
    INSERT INTO operation_resolutions_fts (operation_resolutions_fts, rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES ('delete', old.rowid, old.goal, old.problem, old.cause, old.fix, old.subjects, old.contracts, old.tags);
    INSERT INTO operation_resolutions_fts (rowid, goal, problem, cause, fix, subjects, contracts, tags)
    VALUES (new.rowid, new.goal, new.problem, new.cause, new.fix, new.subjects, new.contracts, new.tags);
END;
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
CREATE TABLE package_artifacts (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    version        TEXT NOT NULL,
    archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*'),
    key_id         TEXT NOT NULL CHECK (length(key_id) = 16 AND key_id NOT GLOB '*[^0-9a-f]*'),
    archive_size   INTEGER NOT NULL CHECK (archive_size > 0),
    signature_toml TEXT NOT NULL,
    signer_label   TEXT,
    source         TEXT NOT NULL,
    imported_by    TEXT NOT NULL,
    imported_at    INTEGER NOT NULL,
    UNIQUE (name, version)
) STRICT;
CREATE INDEX package_artifacts_digest ON package_artifacts (archive_sha256);
CREATE TABLE install_jobs (
    id             TEXT PRIMARY KEY,
    kind           TEXT NOT NULL,
    name           TEXT NOT NULL,
    version        TEXT NOT NULL,
    archive_sha256 TEXT CHECK (archive_sha256 IS NULL OR (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*')),
    key_id         TEXT CHECK (key_id IS NULL OR length(key_id) = 16),
    requested_by   TEXT NOT NULL,
    created_at     INTEGER NOT NULL,
    job_plan_hash  TEXT NOT NULL CHECK (length(job_plan_hash) = 64 AND job_plan_hash NOT GLOB '*[^0-9a-f]*')
) STRICT;
CREATE INDEX install_jobs_created ON install_jobs (created_at DESC, id DESC);
CREATE TABLE install_job_targets (
    id                   TEXT PRIMARY KEY,
    job_id               TEXT NOT NULL REFERENCES install_jobs (id) ON DELETE CASCADE,
    host_id              TEXT NOT NULL,
    attempt              INTEGER NOT NULL CHECK (attempt >= 1),
    execution_id         TEXT NOT NULL UNIQUE,
    status               TEXT NOT NULL,
    plan_hash            TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    approved_by          TEXT NOT NULL,
    approval_channel     TEXT NOT NULL,
    approved_at          INTEGER NOT NULL,
    approval_session_ref TEXT,
    approval_client      TEXT NOT NULL,
    authorization_kind   TEXT NOT NULL CHECK (authorization_kind = 'human'),
    expires_at           INTEGER NOT NULL,
    artifact_transfer    TEXT,
    signature_transfer   TEXT,
    dispatched_session   TEXT,
    dispatched_epoch     TEXT,
    failure_code         TEXT,
    failure_message      TEXT,
    outputs              TEXT,
    outputs_confirmed    INTEGER NOT NULL DEFAULT 0,
    queued_at            INTEGER NOT NULL,
    started_at           INTEGER,
    finished_at          INTEGER,
    version              INTEGER NOT NULL DEFAULT 1,
    UNIQUE (job_id, host_id, attempt)
) STRICT;
CREATE INDEX install_targets_job ON install_job_targets (job_id);
CREATE INDEX install_targets_status ON install_job_targets (status, queued_at);
CREATE UNIQUE INDEX install_targets_one_active_per_host ON install_job_targets (host_id)
    WHERE status IN ('queued', 'transferring', 'dispatched');
CREATE TABLE host_packages (
    host_id            TEXT NOT NULL,
    name               TEXT NOT NULL,
    version            TEXT NOT NULL,
    key_id             TEXT NOT NULL,
    archive_sha256     TEXT NOT NULL,
    installed_at       INTEGER NOT NULL,
    status             TEXT NOT NULL,
    key_state          TEXT NOT NULL,
    skills             TEXT NOT NULL DEFAULT '[]',
    privilege_boundary TEXT NOT NULL,
    observed_at        INTEGER NOT NULL,
    observed_by        TEXT NOT NULL,
    PRIMARY KEY (host_id, name)
) STRICT;
CREATE TABLE helper_keys (
    host_id     TEXT PRIMARY KEY,
    public_key  BLOB NOT NULL CHECK (length(public_key) = 32),
    fingerprint TEXT NOT NULL CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    pinned_at   INTEGER NOT NULL,
    pinned_by   TEXT NOT NULL
) STRICT;
CREATE TABLE helper_attestation (
    host_id          TEXT PRIMARY KEY,
    latest_outcome   TEXT NOT NULL,
    latest_reason    TEXT,
    latest_detail    TEXT,
    latest_at        INTEGER NOT NULL,
    fingerprint      TEXT CHECK (fingerprint IS NULL OR (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*')),
    report           BLOB CHECK (report IS NULL OR length(report) <= 65536),
    generation       INTEGER CHECK (generation IS NULL OR generation >= 1),
    boot_id          TEXT,
    config_hash      TEXT CHECK (config_hash IS NULL OR (length(config_hash) = 64 AND config_hash NOT GLOB '*[^0-9a-f]*')),
    route_state_hash TEXT CHECK (route_state_hash IS NULL OR (length(route_state_hash) = 64 AND route_state_hash NOT GLOB '*[^0-9a-f]*')),
    nonce_issued_at  INTEGER,
    accepted_at      INTEGER,
    version          INTEGER NOT NULL DEFAULT 1,
    CHECK ((report IS NULL) = (generation IS NULL) AND (report IS NULL) = (nonce_issued_at IS NULL)
           AND (report IS NULL) = (fingerprint IS NULL) AND (report IS NULL) = (boot_id IS NULL)
           AND (report IS NULL) = (config_hash IS NULL) AND (report IS NULL) = (route_state_hash IS NULL)
           AND (report IS NULL) = (accepted_at IS NULL))
) STRICT;
CREATE TABLE helper_high_water (
    host_id          TEXT NOT NULL,
    fingerprint      TEXT NOT NULL CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    generation       INTEGER NOT NULL CHECK (generation >= 1),
    config_hash      TEXT NOT NULL CHECK (length(config_hash) = 64 AND config_hash NOT GLOB '*[^0-9a-f]*'),
    route_state_hash TEXT NOT NULL CHECK (length(route_state_hash) = 64 AND route_state_hash NOT GLOB '*[^0-9a-f]*'),
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (host_id, fingerprint)
) STRICT;
CREATE TABLE execution_permits (
    execution_id   TEXT PRIMARY KEY REFERENCES executions (execution_id),
    decision_id    TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    ceiling_hash   TEXT NOT NULL,
    plan_hash      TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    issued_at      INTEGER NOT NULL,
    expires_at     INTEGER NOT NULL,
    decision       TEXT NOT NULL
) STRICT;
CREATE TABLE execution_permit_rules (
    execution_id TEXT NOT NULL REFERENCES execution_permits (execution_id),
    rule_id      TEXT NOT NULL,
    PRIMARY KEY (execution_id, rule_id)
) STRICT;
CREATE INDEX execution_permit_rules_rule ON execution_permit_rules (rule_id);
CREATE INDEX execution_permits_issued    ON execution_permits (issued_at);
CREATE TABLE autonomy_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    paused     INTEGER NOT NULL CHECK (paused IN (0, 1)),
    changed_by TEXT,
    changed_at INTEGER,
    reason     TEXT
) STRICT;
CREATE TABLE rule_circuit_resets (
    rule_id  TEXT NOT NULL,
    reset_at INTEGER NOT NULL,
    reset_by TEXT NOT NULL,
    PRIMARY KEY (rule_id, reset_at)
) STRICT;
CREATE TABLE unknown_resolutions (
    execution_id TEXT PRIMARY KEY REFERENCES executions (execution_id),
    resolved_at  INTEGER NOT NULL,
    resolved_by  TEXT NOT NULL,
    how          TEXT NOT NULL,
    note         TEXT
) STRICT;
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

-- Rows the schema needs to exist before the first write.
INSERT INTO journal_floor VALUES (1, 0);
INSERT INTO policy_shadow_state VALUES (1, NULL, NULL, 0, 0, 0);
INSERT INTO autonomy_state VALUES (1, 0, NULL, NULL, NULL);
