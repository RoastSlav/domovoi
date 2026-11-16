-- Approval binding columns: which plan family an action uses, the exact hash a human was
-- shown, and where that approval was made. A NULL plan_hash_v2 means the action keeps the
-- legacy v1 binding; there is no separate version column.

ALTER TABLE actions ADD COLUMN plan_hash_v2 TEXT
    CHECK (plan_hash_v2 IS NULL OR (length(plan_hash_v2) = 64 AND plan_hash_v2 NOT GLOB '*[^0-9a-f]*'));
ALTER TABLE actions ADD COLUMN approved_plan_hash TEXT
    CHECK (approved_plan_hash IS NULL OR (length(approved_plan_hash) = 64 AND approved_plan_hash NOT GLOB '*[^0-9a-f]*'));
ALTER TABLE actions ADD COLUMN approval_session_ref TEXT;
ALTER TABLE actions ADD COLUMN approval_client TEXT;
ALTER TABLE actions ADD COLUMN authorization_kind TEXT
    CHECK (authorization_kind IS NULL OR authorization_kind IN ('human', 'policy'));

-- Every row that already has an approver was approved by a human: the approve path is the
-- only writer of approved_by, and it has no policy branch before this change. The displayed
-- hash is not backfilled: NULL means the hub cannot honestly claim which value was shown.
UPDATE actions SET authorization_kind = 'human' WHERE approved_by IS NOT NULL;
