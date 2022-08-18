/* only interested in guild_channels */
CREATE TABLE IF NOT EXISTS channels (
    id bigint PRIMARY KEY NOT NULL,
    name text NOT NULL
);