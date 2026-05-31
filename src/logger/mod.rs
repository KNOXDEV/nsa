pub(crate) mod queries;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::futures::future::join_all;
use serenity::gateway::ActivityData;
use serenity::model::channel::{Channel, Message, Reaction, ReactionType};
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;

use crate::logger::queries::Database;

pub struct DiscordLogger {
    database: Arc<Database>,
    // channel_id -> last_message_id at boot, snapshotted in main.rs before the gateway connected
    // (load-bearing: live events bump last_message_id within ms of connect, so the catch-up floor
    // must be read while still disconnected)
    catch_up_floors: HashMap<i64, i64>,
    // run-once guard: ready re-fires on every reconnect, the swap keeps the backfill spawn idempotent
    started: AtomicBool,
}

impl DiscordLogger {
    pub async fn new(
        postgres_client: tokio_postgres::Client,
        catch_up_floors: HashMap<i64, i64>,
    ) -> Self {
        DiscordLogger {
            database: Arc::new(Database::new(postgres_client).await),
            catch_up_floors,
            started: AtomicBool::new(false),
        }
    }

    async fn log_message(
        &self,
        ctx: &Context,
        message: &Message,
    ) -> Result<(), tokio_postgres::Error> {
        let id = message.id.get() as i64;
        let channel_id = message.channel_id.get() as i64;

        // channel fetch is non-fatal: on failure we still record a degraded channel row so the
        // message FK holds. insert_channel COALESCE-upserts, so a later fetch enriches the row.
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
            // non-guild channel (e.g. DM): bare row, no guild.
            Ok(_) => {
                self.database
                    .insert_channel(channel_id, None, None, id, id)
                    .await?;
            }
            // fetch failed: fall back to the gateway's guild_id so a guild message still records
            // its guild rather than NULL (None for DMs/backfills). a later fetch enriches the name.
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

        // user + message + attachments (no channel resolution); shared with the backfill path
        persist_message(&self.database, message).await
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

// Persist user + message + attachments. No channel resolution and no Context, so it issues zero
// extra REST calls — the single most important rate-limit decision for backfill, which only runs
// over channels already in `channels` (the message FK is already satisfied). Shared with the live
// path via log_message.
//
// Timestamp conversion soft-fails (skip + log, no panic) rather than .expect()-ing: a single bad
// historical message can't kill the rest of a sweep, and a bad live timestamp no longer crashes the
// process (strictly-better live behavior).
pub(crate) async fn persist_message(
    db: &Database,
    message: &Message,
) -> Result<(), tokio_postgres::Error> {
    let id = message.id.get() as i64;
    let Some(timestamp) = DateTime::from_timestamp(message.timestamp.unix_timestamp(), 0) else {
        eprintln!("skipping message {id}: out-of-range timestamp");
        return Ok(());
    };
    let timestamp = timestamp.naive_utc();
    // edit_time defaults to sent_time when absent; a bad edit timestamp falls back to sent_time
    // rather than dropping an otherwise-valid message.
    let edit_timestamp = match message.edited_timestamp {
        Some(ts) => {
            DateTime::from_timestamp(ts.unix_timestamp(), 0).map_or(timestamp, |dt| dt.naive_utc())
        }
        None => timestamp,
    };
    let channel_id = message.channel_id.get() as i64;
    let user_id = message.author.id.get() as i64;
    let username = &message.author.name;

    db.insert_user(user_id, username).await?;
    db.insert_message(
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
        db.insert_attachment(
            attachment.id.get() as i64,
            &attachment.filename,
            &attachment.url,
            id,
        )
        .await?;
    }

    Ok(())
}

// Record the userless aggregate reaction counts that ride along in a GetMessages payload
// (Message.reactions carries emoji + count, but not who reacted) into reaction_counts. Costs zero
// extra REST calls. The live path never calls this — a freshly-received live message has no
// reactions and per-user events keep current_reactions accurate; only a sweep observes the
// anonymous remainder in pre-bot history. Customs get an emojis row first to satisfy the FK.
pub(crate) async fn persist_reaction_counts(
    db: &Database,
    message: &Message,
    observed_at: NaiveDateTime,
) -> Result<(), tokio_postgres::Error> {
    let message_id = message.id.get() as i64;
    for reaction in &message.reactions {
        let count = reaction.count as i32;
        match &reaction.reaction_type {
            ReactionType::Custom { id, name, .. } => {
                let eid = id.get() as i64;
                let ename = name.clone().unwrap_or_else(|| id.to_string());
                db.insert_emoji(eid, &ename).await?;
                db.upsert_reaction_count_custom(message_id, eid, &ename, count, observed_at)
                    .await?;
            }
            ReactionType::Unicode(unicode) => {
                db.upsert_reaction_count_unicode(message_id, unicode, count, observed_at)
                    .await?;
            }
            other => {
                eprintln!("unhandled reaction type {other:?}, skipping count");
            }
        }
    }
    Ok(())
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
        // soft-fail: drop the event on a transient DB error rather than panicking the task
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

        // spawn backfill once per process (ready re-fires on reconnect; the swap is idempotent).
        // No opt-in flag — both passes are normal operating behavior; only BACKFILL_DISABLE=1
        // suppresses them. Spawning keeps the gateway + live logging responsive while it runs.
        let disabled = std::env::var("BACKFILL_DISABLE").as_deref() == Ok("1");
        if disabled {
            println!("backfill: disabled via BACKFILL_DISABLE");
        } else if !self.started.swap(true, Ordering::SeqCst) {
            let http = ctx.http.clone(); // Arc<Http>
            let db = self.database.clone(); // Arc<Database>
            let floors = self.catch_up_floors.clone(); // snapshot from boot
            let cfg = crate::backfill::Config::from_env();
            tokio::spawn(async move {
                crate::backfill::run(http, db, cfg, floors).await;
            });
        }
    }
}
