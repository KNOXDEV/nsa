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
        ) = tokio::try_join!(
            postgres_client.prepare(INSERT_MESSAGE_QUERY),
            postgres_client.prepare(INSERT_USER_QUERY),
            postgres_client.prepare(INSERT_GUILD_QUERY),
            postgres_client.prepare(INSERT_CHANNEL_QUERY),
            postgres_client.prepare(INSERT_ATTACHMENT_QUERY),
            postgres_client.prepare(INSERT_EMOJI_QUERY),
            postgres_client.prepare(INSERT_REACTION_QUERY),
            postgres_client.prepare(MESSAGE_EXISTS_QUERY),
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
}
