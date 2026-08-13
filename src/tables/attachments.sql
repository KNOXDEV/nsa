CREATE TABLE IF NOT EXISTS attachments (
    id bigint PRIMARY KEY NOT NULL,
    filename text NOT NULL,
    url text NOT NULL,
    message_id bigint REFERENCES messages(id)
);

-- the attachment drain's anti-join and per-message lookups; IF NOT EXISTS makes this
-- prod-safe on the already-deployed table (unlike ALTER, which would need a hand migration)
CREATE INDEX IF NOT EXISTS attachments_message_id_idx ON attachments (message_id);