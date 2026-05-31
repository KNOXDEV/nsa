CREATE TABLE IF NOT EXISTS reaction_events (
    message_id bigint NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    emoji_id bigint REFERENCES emojis(id) ON DELETE CASCADE,   -- custom emoji only; NULL for unicode
    emoji_name text NOT NULL,                                  -- unicode char OR custom emoji name
    user_id bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    removed boolean NOT NULL DEFAULT FALSE,
    reacted_at timestamp NOT NULL
);
