/* reaction_events is an append-only event log (RESUME redelivery and add/remove cycles leave
   duplicate and contradictory rows). This view is the current state -- latest event per
   (message, user, emoji), adds only. */
CREATE OR REPLACE VIEW current_reactions AS
SELECT message_id, emoji_id, emoji_name, user_id, reacted_at
FROM (
    SELECT DISTINCT ON (message_id, user_id, emoji_id, emoji_name)
        message_id, emoji_id, emoji_name, user_id, reacted_at, removed
    FROM reaction_events
    ORDER BY message_id, user_id, emoji_id, emoji_name, reacted_at DESC
) latest
WHERE NOT removed;
