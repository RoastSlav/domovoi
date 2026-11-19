-- The single-use chat approval token of a step awaiting approval (hash only) and the skill's risk
-- when the step was proposed. Both are validated in Rust; a NULL risk means unknown.
ALTER TABLE executions ADD COLUMN chat_token_hash BLOB
    CHECK (chat_token_hash IS NULL OR length(chat_token_hash) = 32);
ALTER TABLE executions ADD COLUMN risk TEXT;
CREATE UNIQUE INDEX executions_chat_token ON executions (chat_token_hash)
    WHERE chat_token_hash IS NOT NULL;
