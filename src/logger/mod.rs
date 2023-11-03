mod queries;

use chrono::NaiveDateTime;
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::futures::future::join_all;
use serenity::model::channel::{Channel, Message, ReactionType};
use serenity::model::gateway::{Activity, Ready};
use serenity::model::guild::Guild;

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
        let user_id = message.author.id.0 as i64;
        let username = &message.author.name;

        // if we haven't logged this user before
        self.database.insert_user(user_id, username).await?;

        // if this message was sent in a GuildChannel, record both the guild and the channel
        if let Channel::Guild(channel) = message
            .channel(ctx)
            .await
            .expect("failed to get message's guild info")
        {
            // this does not store the guild name, but if its already there will not overwrite it
            self.database
                .insert_guild(channel.guild_id.0 as i64, None)
                .await?;
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
        // otherwise, only record the channel (which is probably a private message)
        else {
            self.database
                .insert_channel(channel_id, None, None, id, id)
                .await?;
        }

        // finally, don't forget to log the message
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
    // when joining a new guild, store its information
    async fn guild_create(&self, _ctx: Context, guild: Guild) {
        self.database
            .insert_guild(guild.id.0 as i64, Some(&guild.name))
            .await
            .expect("failed to log guild");

        println!("logged new guild: {}", guild.name)
    }

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

        // randomly react to incoming messages 0.1% of the time
        if rand::random::<f64>() < 0.001 {
            new_message
                .react(ctx, ReactionType::Unicode(String::from("👀")))
                .await
                .expect("failed to react creepily");
        }
    }

    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        // set creepy status
        ctx.set_activity(Activity::watching("all of us")).await;

        // insert all guilds
        // technically, get_guilds will fail to get all guilds after # > 100
        println!("logging all current guilds");
        join_all(
            ctx.http
                .get_guilds(None, None)
                .await
                .expect("failed to fetch guild info")
                .iter()
                .map(|guild| {
                    self.database
                        .insert_guild(guild.id.0 as i64, Some(&guild.name))
                }),
        )
        .await;

        println!("client is now ready and listening");
    }
}
