INSERT INTO channels (id, name, guild_id, first_message_id, last_message_id)
VALUES ($1, $2, $3, $4, $5)
-- enrich a previously-degraded row: a fetch failure (or a DM) records a bare row, and a
-- later successful fetch fills in the name/guild (and picks up channel renames). EXCLUDED is
-- preferred, but a NULL from a degraded insert falls back to the stored value, so a real value
-- is never clobbered back to NULL.
ON CONFLICT (id) DO UPDATE SET
    name = COALESCE(EXCLUDED.name, channels.name),
    guild_id = COALESCE(EXCLUDED.guild_id, channels.guild_id);
