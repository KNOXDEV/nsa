SELECT oldest_backfilled_id, complete FROM backfill_state WHERE channel_id = $1;
