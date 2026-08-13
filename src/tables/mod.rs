use tokio_postgres::{Client, Error};

const GUILDS_TABLE: &str = include_str!("./guilds.sql");
const CHANNELS_TABLE: &str = include_str!("./channels.sql");
const USERS_TABLE: &str = include_str!("./users.sql");
const MEMBERS_TABLE: &str = include_str!("./members.sql");
const EMOJI_TABLE: &str = include_str!("./emojis.sql");
const REACTION_EVENTS_TABLE: &str = include_str!("./reaction_events.sql");
const CURRENT_REACTIONS_VIEW: &str = include_str!("./current-reactions.sql");
const REACTION_COUNTS_TABLE: &str = include_str!("./reaction_counts.sql");
const REACTION_SUMMARY_VIEW: &str = include_str!("./reaction-summary.sql");
const ATTACHMENTS_TABLE: &str = include_str!("./attachments.sql");
const MESSAGES_TABLE: &str = include_str!("./messages.sql");
const PIN_EVENTS_TABLE: &str = include_str!("./pin_events.sql");
const CURRENT_PINS_VIEW: &str = include_str!("./current-pins.sql");
const BACKFILL_STATE_TABLE: &str = include_str!("./backfill_state.sql");

pub async fn init_tables(client: &Client) -> Result<(), Error> {
    client.query_opt(GUILDS_TABLE, &[]).await?;
    client.query_opt(USERS_TABLE, &[]).await?;
    client.query_opt(EMOJI_TABLE, &[]).await?;

    client.query_opt(CHANNELS_TABLE, &[]).await?;
    client.query_opt(MEMBERS_TABLE, &[]).await?;

    client.query_opt(MESSAGES_TABLE, &[]).await?;

    client.query_opt(REACTION_EVENTS_TABLE, &[]).await?;
    // view over reaction_events; must run after that table is created
    client.query_opt(CURRENT_REACTIONS_VIEW, &[]).await?;
    // userless aggregate counts; depends on emojis + messages (FK). batch_execute because the file
    // holds the table plus its two partial unique indexes (the extended protocol query_opt uses
    // allows only one command).
    client.batch_execute(REACTION_COUNTS_TABLE).await?;
    // reconciliation view; must run after current_reactions + reaction_counts exist
    client.query_opt(REACTION_SUMMARY_VIEW, &[]).await?;
    client.query_opt(ATTACHMENTS_TABLE, &[]).await?;

    // pin observation log; FKs on messages + channels. view after the table.
    client.query_opt(PIN_EVENTS_TABLE, &[]).await?;
    client.query_opt(CURRENT_PINS_VIEW, &[]).await?;

    // internal checkpoint for the historical sweep (no FK; keyed by channel id)
    client.query_opt(BACKFILL_STATE_TABLE, &[]).await?;

    Ok(())
}
