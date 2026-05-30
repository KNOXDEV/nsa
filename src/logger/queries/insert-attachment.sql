INSERT INTO attachments (id, filename, url, message_id)
VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO NOTHING;
