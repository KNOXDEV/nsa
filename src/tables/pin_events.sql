/* append-only pin observation log: sync_channel_pins writes one row per state change it
   observes (pinned = became pinned, NOT pinned = no longer pinned). Duplicates from
   concurrent syncs are tolerated; current_pins collapses to the latest observation. */
CREATE TABLE IF NOT EXISTS pin_events (
    message_id bigint NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    channel_id bigint NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    pinned boolean NOT NULL,
    observed_at timestamp NOT NULL
);
