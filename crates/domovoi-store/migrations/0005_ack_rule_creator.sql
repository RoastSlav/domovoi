-- When and by whom a known condition was created. Rules made before this are NULL.
ALTER TABLE ack_rules ADD COLUMN created_at INTEGER;
ALTER TABLE ack_rules ADD COLUMN created_by TEXT;
