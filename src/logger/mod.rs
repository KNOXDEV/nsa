mod queries;

use chrono::NaiveDateTime;
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::channel::{Channel, Message, ReactionType};
use serenity::model::gateway::Ready;
use serenity::model::user::User;

use crate::logger::queries::Database;

pub struct DiscordLogger {
    database: Database,
}

impl DiscordLogger {
    pub async fn new(postgres_client: tokio_postgres::Client) -> Self {
        DiscordLogger {
            database: Database::new(postgres_client).await,
        }
    }

    async fn log_user(&self, user: &User) -> Result<u64, tokio_postgres::Error> {
        self.database
            .insert_user(user.id.0 as i64, &user.name)
            .await
    }

    async fn log_message(
        &self,
        ctx: &Context,
        message: &Message,
    ) -> Result<u64, tokio_postgres::Error> {
        let id = message.id.0 as i64;
        let timestamp = NaiveDateTime::from_timestamp(message.timestamp.unix_timestamp(), 0);
        let edit_timestamp = message.edited_timestamp.map_or(timestamp, |ts| {
            NaiveDateTime::from_timestamp(ts.unix_timestamp(), 0)
        });
        let channel_id = message.channel_id.0 as i64;

        // if we haven't logged this user before
        self.log_user(&message.author).await?;

        // if this message was sent in a GuildChannel, record both
        if let Channel::Guild(channel) = message
            .channel(ctx)
            .await
            .expect("message not sent in channel")
        {
            // TODO: resolve guild in a way non-reliant on cache
            if let Some(guild) = channel.guild(ctx) {
                self.database
                    .insert_guild(guild.id.0 as i64, Some(&guild.name))
                    .await?;
            } else {
                self.database
                    .insert_guild(channel.guild_id.0 as i64, None)
                    .await?;
            }
            self.database
                .insert_channel(
                    channel_id,
                    Some(&channel.name),
                    Some(channel.guild_id.0 as i64),
                    id,
                    id,
                )
                .await?;
        }
        // otherwise, only record the channel
        else {
            self.database
                .insert_channel(channel_id, None, None, id, id)
                .await?;
        }

        // if we haven't logged this message before
        self.database
            .insert_message(
                message.id.0 as i64,
                &message.content,
                timestamp,
                edit_timestamp,
                message.author.id.0 as i64,
                message.channel_id.0 as i64,
            )
            .await

        // TODO: log reactions, attachments, etc.
    }
}

#[async_trait]
impl EventHandler for DiscordLogger {
    // log every message and its reactions, etc
    async fn message(&self, ctx: Context, new_message: Message) {
        println!(
            "[{}] {} in #{}: {}",
            new_message.timestamp,
            new_message.author.name,
            new_message.channel_id,
            new_message.content
        );

        self.log_message(&ctx, &new_message)
            .await
            .expect("failed to save logged message to database");

        // randomly react to incoming messages 0.5% of the time
        if rand::random::<f64>() < 0.005 {
            new_message
                .react(ctx, ReactionType::Unicode(String::from("👀")))
                .await
                .expect("failed to react creepily");
        }
    }

    async fn ready(&self, _ctx: Context, _data_about_bot: Ready) {
        println!("client is now ready");
    }
}
