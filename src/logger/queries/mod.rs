use chrono::NaiveDateTime;
use tokio_postgres::{Client, Error, Statement};

const INSERT_MESSAGE_QUERY: &str = include_str!("./insert-message.sql");
const INSERT_USER_QUERY: &str = include_str!("./insert-user.sql");
const INSERT_GUILD_QUERY: &str = include_str!("./insert-guild.sql");
const INSERT_CHANNEL_QUERY: &str = include_str!("./insert-channel.sql");
const INSERT_ATTACHMENT_QUERY: &str = include_str!("./insert-attachment.sql");
const INSERT_EMOJI_QUERY: &str = include_str!("./insert-emoji.sql");
const INSERT_REACTION_QUERY: &str = include_str!("./insert-reaction.sql");
const MESSAGE_EXISTS_QUERY: &str = include_str!("./message-exists.sql");
const INSERT_PIN_EVENT_QUERY: &str = include_str!("./insert-pin-event.sql");
const CURRENT_PINS_FOR_CHANNEL_QUERY: &str = include_str!("./current-pins-for-channel.sql");
// backfill query SQL lives next to the backfill module (CLAUDE.md's "SQL next to its module"),
// but the prepared statements over it stay centralized in this one Database struct.
const LIST_BACKFILL_CHANNELS_QUERY: &str =
    include_str!("../../backfill/queries/list-backfill-channels.sql");
const GET_BACKFILL_STATE_QUERY: &str =
    include_str!("../../backfill/queries/get-backfill-state.sql");
const UPSERT_BACKFILL_STATE_QUERY: &str =
    include_str!("../../backfill/queries/upsert-backfill-state.sql");
const BUMP_LAST_MESSAGE_ID_QUERY: &str =
    include_str!("../../backfill/queries/bump-last-message-id.sql");
const UPSERT_REACTION_COUNT_CUSTOM_QUERY: &str =
    include_str!("../../backfill/queries/upsert-reaction-count-custom.sql");
const UPSERT_REACTION_COUNT_UNICODE_QUERY: &str =
    include_str!("../../backfill/queries/upsert-reaction-count-unicode.sql");

pub struct Database {
    postgres_client: Client,
    insert_message_statement: Statement,
    insert_user_statement: Statement,
    insert_guild_statement: Statement,
    insert_channel_statement: Statement,
    insert_attachment_statement: Statement,
    insert_emoji_statement: Statement,
    insert_reaction_statement: Statement,
    message_exists_statement: Statement,
    insert_pin_event_statement: Statement,
    current_pins_for_channel_statement: Statement,
    list_backfill_channels_statement: Statement,
    get_backfill_state_statement: Statement,
    upsert_backfill_state_statement: Statement,
    bump_last_message_id_statement: Statement,
    upsert_reaction_count_custom_statement: Statement,
    upsert_reaction_count_unicode_statement: Statement,
}

impl Database {
    pub async fn new(postgres_client: Client) -> Self {
        let (
            insert_message_statement,
            insert_user_statement,
            insert_guild_statement,
            insert_channel_statement,
            insert_attachment_statement,
            insert_emoji_statement,
            insert_reaction_statement,
            message_exists_statement,
            insert_pin_event_statement,
            current_pins_for_channel_statement,
            list_backfill_channels_statement,
            get_backfill_state_statement,
            upsert_backfill_state_statement,
            bump_last_message_id_statement,
            upsert_reaction_count_custom_statement,
            upsert_reaction_count_unicode_statement,
        ) = tokio::try_join!(
            postgres_client.prepare(INSERT_MESSAGE_QUERY),
            postgres_client.prepare(INSERT_USER_QUERY),
            postgres_client.prepare(INSERT_GUILD_QUERY),
            postgres_client.prepare(INSERT_CHANNEL_QUERY),
            postgres_client.prepare(INSERT_ATTACHMENT_QUERY),
            postgres_client.prepare(INSERT_EMOJI_QUERY),
            postgres_client.prepare(INSERT_REACTION_QUERY),
            postgres_client.prepare(MESSAGE_EXISTS_QUERY),
            postgres_client.prepare(INSERT_PIN_EVENT_QUERY),
            postgres_client.prepare(CURRENT_PINS_FOR_CHANNEL_QUERY),
            postgres_client.prepare(LIST_BACKFILL_CHANNELS_QUERY),
            postgres_client.prepare(GET_BACKFILL_STATE_QUERY),
            postgres_client.prepare(UPSERT_BACKFILL_STATE_QUERY),
            postgres_client.prepare(BUMP_LAST_MESSAGE_ID_QUERY),
            postgres_client.prepare(UPSERT_REACTION_COUNT_CUSTOM_QUERY),
            postgres_client.prepare(UPSERT_REACTION_COUNT_UNICODE_QUERY),
        )
        .expect("failed to generate prepared statements");

        Database {
            postgres_client,
            insert_message_statement,
            insert_user_statement,
            insert_guild_statement,
            insert_channel_statement,
            insert_attachment_statement,
            insert_emoji_statement,
            insert_reaction_statement,
            message_exists_statement,
            insert_pin_event_statement,
            current_pins_for_channel_statement,
            list_backfill_channels_statement,
            get_backfill_state_statement,
            upsert_backfill_state_statement,
            bump_last_message_id_statement,
            upsert_reaction_count_custom_statement,
            upsert_reaction_count_unicode_statement,
        }
    }

    pub async fn insert_user(&self, id: i64, username: &str) -> Result<u64, Error> {
        self.postgres_client
            .execute(&self.insert_user_statement, &[&id, &username])
            .await
    }

    pub async fn insert_message(
        &self,
        id: i64,
        content: &str,
        sent_time: NaiveDateTime,
        edit_time: NaiveDateTime,
        user_id: i64,
        channel_id: i64,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.insert_message_statement,
                &[&id, &content, &sent_time, &edit_time, &user_id, &channel_id],
            )
            .await
    }

    pub async fn insert_channel(
        &self,
        id: i64,
        name: Option<&str>,
        guild_id: Option<i64>,
        first_message_id: i64,
        last_message_id: i64,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.insert_channel_statement,
                &[&id, &name, &guild_id, &first_message_id, &last_message_id],
            )
            .await
    }

    pub async fn insert_guild(&self, id: i64, name: Option<&str>) -> Result<u64, Error> {
        self.postgres_client
            .execute(&self.insert_guild_statement, &[&id, &name])
            .await
    }

    pub async fn insert_attachment(
        &self,
        id: i64,
        filename: &str,
        url: &str,
        message_id: i64,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.insert_attachment_statement,
                &[&id, &filename, &url, &message_id],
            )
            .await
    }

    pub async fn insert_emoji(&self, id: i64, name: &str) -> Result<u64, Error> {
        self.postgres_client
            .execute(&self.insert_emoji_statement, &[&id, &name])
            .await
    }

    pub async fn insert_reaction(
        &self,
        message_id: i64,
        emoji_id: Option<i64>,
        emoji_name: &str,
        user_id: i64,
        removed: bool,
        reacted_at: NaiveDateTime,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.insert_reaction_statement,
                &[
                    &message_id,
                    &emoji_id,
                    &emoji_name,
                    &user_id,
                    &removed,
                    &reacted_at,
                ],
            )
            .await
    }

    pub async fn message_exists(&self, id: i64) -> Result<bool, Error> {
        Ok(self
            .postgres_client
            .query_opt(&self.message_exists_statement, &[&id])
            .await?
            .is_some())
    }

    pub async fn insert_pin_event(
        &self,
        message_id: i64,
        channel_id: i64,
        pinned: bool,
        observed_at: NaiveDateTime,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.insert_pin_event_statement,
                &[&message_id, &channel_id, &pinned, &observed_at],
            )
            .await
    }

    // message ids currently pinned in this channel, per the latest observations
    pub async fn current_pins_for_channel(&self, channel_id: i64) -> Result<Vec<i64>, Error> {
        Ok(self
            .postgres_client
            .query(&self.current_pins_for_channel_statement, &[&channel_id])
            .await?
            .iter()
            .map(|row| row.get(0))
            .collect())
    }

    // every channel we've seen a message in, with its high-water last_message_id (the catch-up floor)
    pub async fn list_backfill_channels(&self) -> Result<Vec<(i64, i64)>, Error> {
        Ok(self
            .postgres_client
            .query(&self.list_backfill_channels_statement, &[])
            .await?
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect())
    }

    // historical-sweep checkpoint: (oldest_backfilled_id, complete), or None if never swept
    pub async fn backfill_state(
        &self,
        channel_id: i64,
    ) -> Result<Option<(Option<i64>, bool)>, Error> {
        Ok(self
            .postgres_client
            .query_opt(&self.get_backfill_state_statement, &[&channel_id])
            .await?
            .map(|row| (row.get(0), row.get(1))))
    }

    pub async fn upsert_backfill_state(
        &self,
        channel_id: i64,
        oldest: Option<i64>,
        complete: bool,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.upsert_backfill_state_statement,
                &[&channel_id, &oldest, &complete],
            )
            .await
    }

    pub async fn bump_last_message_id(
        &self,
        channel_id: i64,
        message_id: i64,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.bump_last_message_id_statement,
                &[&channel_id, &message_id],
            )
            .await
    }

    pub async fn upsert_reaction_count_custom(
        &self,
        message_id: i64,
        emoji_id: i64,
        emoji_name: &str,
        count: i32,
        observed_at: NaiveDateTime,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.upsert_reaction_count_custom_statement,
                &[&message_id, &emoji_id, &emoji_name, &count, &observed_at],
            )
            .await
    }

    pub async fn upsert_reaction_count_unicode(
        &self,
        message_id: i64,
        emoji_name: &str,
        count: i32,
        observed_at: NaiveDateTime,
    ) -> Result<u64, Error> {
        self.postgres_client
            .execute(
                &self.upsert_reaction_count_unicode_statement,
                &[&message_id, &emoji_name, &count, &observed_at],
            )
            .await
    }
}
