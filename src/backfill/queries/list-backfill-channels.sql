-- every channels row is a message-bearing channel, so no kind/degraded filter is needed
SELECT id, last_message_id FROM channels;
