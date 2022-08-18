INSERT INTO guilds (id, name)
VALUES ($1, $2) ON CONFLICT (id) DO NOTHING;