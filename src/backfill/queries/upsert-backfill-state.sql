INSERT INTO backfill_state (channel_id, oldest_backfilled_id, complete, updated_at)
VALUES ($1, $2, $3, now())
ON CONFLICT (channel_id) DO UPDATE
  SET oldest_backfilled_id = EXCLUDED.oldest_backfilled_id,
      complete            = EXCLUDED.complete,
      updated_at          = now();
