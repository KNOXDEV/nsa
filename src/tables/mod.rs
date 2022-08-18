use tokio::join;
use tokio_postgres::{Client, Error};

const GUILDS_TABLE: &str = include_str!("./guilds.sql");
const CHANNELS_TABLE: &str = include_str!("./channels.sql");
const CHANNELS_ALTERATION: &str = include_str!("./channels1.sql");
const USERS_TABLE: &str = include_str!("./users.sql");
const MEMBERS_TABLE: &str = include_str!("./members.sql");
const EMOJI_TABLE: &str = include_str!("./emojis.sql");
const REACTIONS_TABLE: &str = include_str!("./reactions.sql");
const ATTACHMENTS_TABLE: &str = include_str!("./attachments.sql");
const MESSAGES_TABLE: &str = include_str!("./messages.sql");

pub async fn init_tables(client: &Client) -> Result<(), Error> {
    client.query_opt(GUILDS_TABLE, &[]).await?;
    client.query_opt(USERS_TABLE, &[]).await?;
    client.query_opt(EMOJI_TABLE, &[]).await?;

    client.query_opt(CHANNELS_TABLE, &[]).await?;
    client.query_opt(MEMBERS_TABLE, &[]).await?;

    client.query_opt(MESSAGES_TABLE, &[]).await?;

    client.query_opt(CHANNELS_ALTERATION, &[]).await?;
    client.query_opt(REACTIONS_TABLE, &[]).await?;
    client.query_opt(ATTACHMENTS_TABLE, &[]).await?;

    Ok(())
}
