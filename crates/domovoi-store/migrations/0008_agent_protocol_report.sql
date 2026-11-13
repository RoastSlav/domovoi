-- What an agent reported after it proved its key (protocol compatibility, RW-17).
ALTER TABLE agents ADD COLUMN reported_protocol      TEXT;     -- 'major.minor', e.g. '1.1'
ALTER TABLE agents ADD COLUMN reported_release       TEXT;
ALTER TABLE agents ADD COLUMN last_authenticated_at  INTEGER;  -- unix ms UTC
ALTER TABLE agents ADD COLUMN incompatible_since     INTEGER;  -- unix ms UTC; NULL when compatible
