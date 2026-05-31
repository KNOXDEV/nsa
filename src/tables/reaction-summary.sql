-- Reconciles the per-user log (current_reactions) with the userless sweep aggregate
-- (reaction_counts): known reactors plus an anonymous remainder for reactions the bot never saw
-- an event for. reaction_counts is a point-in-time snapshot (as of observed_at) and can go stale
-- if reactions change after the sweep; GREATEST(.., 0) clamps the case where live per-user events
-- later record more reactors than the snapshot saw. Treat observed_count as best-effort and
-- current_reactions as the live-accurate "who".
CREATE OR REPLACE VIEW reaction_summary AS
SELECT
    COALESCE(r.message_id, c.message_id) AS message_id,
    COALESCE(r.emoji_id,   c.emoji_id)   AS emoji_id,
    COALESCE(r.emoji_name, c.emoji_name) AS emoji_name,
    COALESCE(r.known_users, 0)                                              AS known_users,
    c.observed_count,
    GREATEST(COALESCE(c.observed_count, 0) - COALESCE(r.known_users, 0), 0) AS anonymous_count
FROM (
    SELECT message_id, emoji_id, emoji_name, COUNT(*) AS known_users
    FROM current_reactions
    GROUP BY message_id, emoji_id, emoji_name
) r
FULL OUTER JOIN (
    SELECT message_id, emoji_id, emoji_name, count AS observed_count
    FROM reaction_counts
) c
  ON  c.message_id = r.message_id
  AND c.emoji_id IS NOT DISTINCT FROM r.emoji_id            -- customs match on id
  AND (c.emoji_id IS NOT NULL OR c.emoji_name = r.emoji_name);  -- unicode matches on name
