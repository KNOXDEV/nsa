CREATE TABLE IF NOT EXISTS attachments (
    id bigint PRIMARY KEY NOT NULL,
    filename text NOT NULL,
    url text NOT NULL,
    message_id bigint REFERENCES messages(id)
);