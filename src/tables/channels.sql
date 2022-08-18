/* only interested in guild_channels */
CREATE TABLE IF NOT EXISTS channels (
    id bigint PRIMARY KEY NOT NULL,
    name text,
    guild_id bigint REFERENCES guilds(id),
    /* notice that we don't put REFERENCES here because this is more for
       checking if we've scraped all the messages in the channel
    */
    first_message_id bigint NOT NULL,
    last_message_id bigint NOT NULL
);