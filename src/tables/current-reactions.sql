/* reactions is an append-only event log: every add/remove is its own row, and gateway RESUME
   redelivery or add->remove->add cycles append duplicate/contradictory rows. This view collapses
   that log to the *current* state -- the latest event per (message, user, emoji), keeping only
   those whose latest event is an add. Read this (not COUNT(*) over reactions) for "who is currently
   reacting"; emoji_id is NULL for unicode, so it is part of the key alongside emoji_name. */
CREATE OR REPLACE VIEW current_reactions AS
SELECT message_id, emoji_id, emoji_name, user_id, reacted_at
FROM (
    SELECT DISTINCT ON (message_id, user_id, emoji_id, emoji_name)
        message_id, emoji_id, emoji_name, user_id, reacted_at, removed
    FROM reactions
    ORDER BY message_id, user_id, emoji_id, emoji_name, reacted_at DESC
) latest
WHERE NOT removed;
