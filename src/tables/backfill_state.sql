CREATE TABLE IF NOT EXISTS backfill_state (
    channel_id bigint PRIMARY KEY NOT NULL,
    oldest_backfilled_id bigint,
    complete boolean NOT NULL DEFAULT FALSE,
    updated_at timestamptz NOT NULL DEFAULT now()
);
