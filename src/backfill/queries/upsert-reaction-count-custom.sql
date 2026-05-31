INSERT INTO reaction_counts (message_id, emoji_id, emoji_name, count, observed_at)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (message_id, emoji_id) WHERE emoji_id IS NOT NULL
DO UPDATE SET count = EXCLUDED.count, observed_at = EXCLUDED.observed_at;
