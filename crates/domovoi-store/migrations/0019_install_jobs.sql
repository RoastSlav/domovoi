-- Package installation: the index of imported archives, install jobs with one target per host
-- and attempt, and what each host last reported as installed. Statuses, kinds, sources and
-- key states are validated in Rust, not by CHECK, so a new value needs no migration.
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
