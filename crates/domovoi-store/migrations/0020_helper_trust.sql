-- Helper trust: the key the owner pinned for each host's privileged helper, the latest
-- attestation outcome with the verified state it left, and the highest accepted generation per
-- host and key. Outcomes and reasons are validated in Rust, not by CHECK, so a new value
-- needs no migration; only hex and blob lengths and positive generations are checked here.

-- The key the owner pinned for each host's helper. One row per host; replaced only by an owner re-pin.
CREATE TABLE helper_keys (
    host_id     TEXT PRIMARY KEY,
    public_key  BLOB NOT NULL CHECK (length(public_key) = 32),
    fingerprint TEXT NOT NULL CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    pinned_at   INTEGER NOT NULL,
    pinned_by   TEXT NOT NULL
) STRICT;

-- The latest attestation outcome per host and, when the latest outcome is verified, the verified state.
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

-- The highest accepted identity per host and key (rollback and repeat detection).
CREATE TABLE helper_high_water (
    host_id          TEXT NOT NULL,
    fingerprint      TEXT NOT NULL CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    generation       INTEGER NOT NULL CHECK (generation >= 1),
    config_hash      TEXT NOT NULL CHECK (length(config_hash) = 64 AND config_hash NOT GLOB '*[^0-9a-f]*'),
    route_state_hash TEXT NOT NULL CHECK (length(route_state_hash) = 64 AND route_state_hash NOT GLOB '*[^0-9a-f]*'),
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (host_id, fingerprint)
) STRICT;
