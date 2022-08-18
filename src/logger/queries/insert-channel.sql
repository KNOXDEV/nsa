INSERT INTO channels (id, name, guild_id, first_message_id, last_message_id)
VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING;