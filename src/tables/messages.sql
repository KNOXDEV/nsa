CREATE TABLE IF NOT EXISTS messages (
    id bigint PRIMARY KEY NOT NULL,
    content text NOT NULL,
    sent_time timestamp NOT NULL,
    edit_time timestamp,
    user_id bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id bigint NOT NULL REFERENCES channels(id) ON DELETE CASCADE
);