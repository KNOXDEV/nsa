UPDATE channels SET last_message_id = GREATEST(last_message_id, $2) WHERE id = $1;
