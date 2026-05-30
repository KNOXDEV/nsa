INSERT INTO channels (id, name, guild_id, first_message_id, last_message_id)
VALUES ($1, $2, $3, $4, $5)
-- self-healing upsert: COALESCE enriches a degraded bare row on a later fetch without clobbering
-- a real value to NULL; the WHERE guard skips no-op rewrites (insert_channel runs per message).
ON CONFLICT (id) DO UPDATE SET
    name = COALESCE(EXCLUDED.name, channels.name),
    guild_id = COALESCE(EXCLUDED.guild_id, channels.guild_id)
WHERE channels.name IS DISTINCT FROM COALESCE(EXCLUDED.name, channels.name)
   OR channels.guild_id IS DISTINCT FROM COALESCE(EXCLUDED.guild_id, channels.guild_id);
