CREATE TABLE IF NOT EXISTS members (
    /* in our table, a null guild means a DM */
    guild_id bigint REFERENCES guilds(id) ON DELETE CASCADE,
    user_id bigint NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    nickname text
);