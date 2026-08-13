/* latest observation per message, pins only. DISTINCT ON (message_id) is enough:
   a message id is globally unique and can only be pinned in its own channel. */
CREATE OR REPLACE VIEW current_pins AS
SELECT message_id, channel_id, observed_at
FROM (
    SELECT DISTINCT ON (message_id)
        message_id, channel_id, observed_at, pinned
    FROM pin_events
    ORDER BY message_id, observed_at DESC
) latest
WHERE pinned;
