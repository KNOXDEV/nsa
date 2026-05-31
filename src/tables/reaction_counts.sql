-- Userless aggregate reaction counts observed by the historical sweep. The GetMessages payload
-- carries Message.reactions (emoji + count) but not WHO reacted, so this answers "how many"
-- (vs current_reactions' per-user "who"). One row per (message, emoji).
CREATE TABLE IF NOT EXISTS reaction_counts (
    message_id  bigint    NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    emoji_id    bigint    REFERENCES emojis(id) ON DELETE CASCADE,  -- custom only; NULL for unicode
    emoji_name  text      NOT NULL,                                 -- unicode char OR custom name
    count       integer   NOT NULL,
    observed_at timestamp NOT NULL                                  -- when a sweep last saw this
);
-- emoji identified by id (custom) or name (unicode). Postgres 14 (prod) has no NULLS NOT DISTINCT,
-- so uniqueness is two partial indexes rather than a composite PK over a nullable column.
CREATE UNIQUE INDEX IF NOT EXISTS reaction_counts_custom_uniq
    ON reaction_counts (message_id, emoji_id)   WHERE emoji_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS reaction_counts_unicode_uniq
    ON reaction_counts (message_id, emoji_name) WHERE emoji_id IS NULL;
