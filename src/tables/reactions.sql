CREATE TABLE IF NOT EXISTS reactions (
    message_id bigint NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    emoji_id bigint NOT NULL REFERENCES emojis(id) ON DELETE CASCADE,
    user_id bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    removed boolean NOT NULL DEFAULT FALSE
);