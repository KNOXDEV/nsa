mod queries;

use chrono::{DateTime, Utc};
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::futures::future::join_all;
use serenity::gateway::ActivityData;
use serenity::model::channel::{Channel, Message, Reaction, ReactionType};
use serenity::model::gateway::Ready;
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
    ) -> Result<(), tokio_postgres::Error> {
        let id = message.id.get() as i64;
        let timestamp = DateTime::from_timestamp(message.timestamp.unix_timestamp(), 0)
            .expect("invalid message timestamp")
            .naive_utc();
        let edit_timestamp = message.edited_timestamp.map_or(timestamp, |ts| {
            DateTime::from_timestamp(ts.unix_timestamp(), 0)
                .expect("invalid message edit timestamp")
                .naive_utc()
        });
        let channel_id = message.channel_id.get() as i64;
        let user_id = message.author.id.get() as i64;
        let username = &message.author.name;

        // if we haven't logged this user before
        self.database.insert_user(user_id, username).await?;

        // if this message was sent in a GuildChannel, record both the guild and the channel.
        // the channel fetch is non-fatal: on failure we still insert a bare channel row so the
        // message FK holds, logging the message with degraded channel metadata rather than
        // panicking. insert_channel is ON CONFLICT DO NOTHING, so an enriched row is never clobbered.
        match message.channel(ctx).await {
            Ok(Channel::Guild(channel)) => {
                // this does not store the guild name, but if its already there will not overwrite it
                self.database
                    .insert_guild(channel.guild_id.get() as i64, None)
                    .await?;
                self.database
                    .insert_channel(
                        channel_id,
                        Some(&channel.name),
                        Some(channel.guild_id.get() as i64),
                        id,
                        id,
                    )
                    .await?;
            }
            // a genuine non-guild channel (probably a private message): record a bare row.
            Ok(_) => {
                self.database
                    .insert_channel(channel_id, None, None, id, id)
                    .await?;
            }
            // fetch failed: fall back to the guild_id the gateway already put on the message so
            // a guild message still records its real guild instead of a poisoned NULL. (Note
            // REST-fetched/backfilled messages omit guild_id, hence the classifier above runs
            // first.) insert_channel's enrichment upsert fills the name in on a later success.
            Err(e) => {
                eprintln!("failed to fetch channel info for message {id}: {e}");
                let guild_id = message.guild_id.map(|g| g.get() as i64);
                if let Some(guild_id) = guild_id {
                    self.database.insert_guild(guild_id, None).await?;
                }
                self.database
                    .insert_channel(channel_id, None, guild_id, id, id)
                    .await?;
            }
        }

        // finally, don't forget to log the message
        self.database
            .insert_message(
                id,
                &message.content,
                timestamp,
                edit_timestamp,
                user_id,
                channel_id,
            )
            .await?;

        // record any attachments (we keep the recoverable CDN url, not the file itself)
        for attachment in &message.attachments {
            self.database
                .insert_attachment(
                    attachment.id.get() as i64,
                    &attachment.filename,
                    &attachment.url,
                    id,
                )
                .await?;
        }

        Ok(())
    }

    async fn log_reaction(
        &self,
        ctx: &Context,
        reaction: &Reaction,
        removed: bool,
    ) -> Result<(), tokio_postgres::Error> {
        // reacting user (gateway sends this for add/remove)
        let Some(user_id) = reaction.user_id else {
            eprintln!("reaction with no user_id, skipping");
            return Ok(());
        };
        let user_id_i64 = user_id.get() as i64;

        // username: prefer member (present on guild adds), else fetch via REST
        let username = match &reaction.member {
            Some(member) => member.user.name.clone(),
            None => match user_id.to_user(&ctx).await {
                Ok(user) => user.name,
                Err(e) => {
                    eprintln!("failed to resolve reacting user {user_id_i64}: {e}");
                    return Ok(());
                }
            },
        };
        self.database.insert_user(user_id_i64, &username).await?;

        // backfill the message if never logged (reaction.message_id FK)
        let message_id = reaction.message_id.get() as i64;
        if !self.database.message_exists(message_id).await? {
            match reaction.channel_id.message(&ctx, reaction.message_id).await {
                Ok(message) => self.log_message(ctx, &message).await?,
                Err(e) => {
                    eprintln!("failed to backfill message {message_id}: {e}");
                    return Ok(());
                }
            }
        }

        // resolve emoji: custom -> catalog + id; unicode -> name only.
        // ReactionType is #[non_exhaustive] => catch-all arm required.
        let (emoji_id, emoji_name) = match &reaction.emoji {
            ReactionType::Custom { id, name, .. } => {
                let eid = id.get() as i64;
                let ename = name.clone().unwrap_or_else(|| id.to_string());
                self.database.insert_emoji(eid, &ename).await?;
                (Some(eid), ename)
            }
            ReactionType::Unicode(unicode) => (None, unicode.clone()),
            other => {
                eprintln!("unhandled reaction type {other:?}, skipping");
                return Ok(());
            }
        };

        // the gateway carries no reaction timestamp and these events are always live,
        // so the logging time is effectively the reaction time
        let reacted_at = Utc::now().naive_utc();
        self.database
            .insert_reaction(
                message_id,
                emoji_id,
                &emoji_name,
                user_id_i64,
                removed,
                reacted_at,
            )
            .await?;
        Ok(())
    }
}

#[async_trait]
impl EventHandler for DiscordLogger {
    // when joining a new guild, store its information
    async fn guild_create(&self, _ctx: Context, guild: Guild, _is_new: Option<bool>) {
        self.database
            .insert_guild(guild.id.get() as i64, Some(&guild.name))
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

    async fn reaction_add(&self, ctx: Context, add_reaction: Reaction) {
        // soft-fail: a transient DB error should drop this event, not panic the task
        // (matches log_reaction's own recoverable-error handling)
        if let Err(e) = self.log_reaction(&ctx, &add_reaction, false).await {
            eprintln!("failed to log added reaction: {e}");
        }
    }

    async fn reaction_remove(&self, ctx: Context, removed_reaction: Reaction) {
        if let Err(e) = self.log_reaction(&ctx, &removed_reaction, true).await {
            eprintln!("failed to log removed reaction: {e}");
        }
    }

    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        // set creepy status
        ctx.set_activity(Some(ActivityData::watching("all of us")));

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
                        .insert_guild(guild.id.get() as i64, Some(&guild.name))
                }),
        )
        .await;

        println!("client is now ready and listening");
    }
}
