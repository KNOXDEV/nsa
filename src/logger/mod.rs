pub(crate) mod queries;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::futures::future::join_all;
use serenity::gateway::ActivityData;
use serenity::http::Http;
use serenity::model::channel::{Channel, Message, Reaction, ReactionType};
use serenity::model::event::ChannelPinsUpdateEvent;
use serenity::model::gateway::Ready;
use serenity::model::guild::Guild;
use serenity::model::id::ChannelId;

use crate::logger::queries::Database;

pub struct DiscordLogger {
    database: Arc<Database>,
    // channel_id -> last_message_id at boot, snapshotted in main.rs before the gateway connected
    // (load-bearing: live events bump last_message_id within ms of connect, so the catch-up floor
    // must be read while still disconnected)
    catch_up_floors: HashMap<i64, i64>,
    // run-once guard: ready re-fires on every reconnect, the swap keeps the backfill spawn idempotent
    started: AtomicBool,
    // present iff ATTACHMENTS_DIR is configured and ATTACHMENTS_DISABLE != 1
    attachment_store: Option<Arc<crate::attachments::AttachmentStore>>,
}

impl DiscordLogger {
    pub async fn new(
        postgres_client: tokio_postgres::Client,
        catch_up_floors: HashMap<i64, i64>,
    ) -> Self {
        let database = Arc::new(Database::new(postgres_client).await);
        let attachment_store = crate::attachments::Config::from_env().map(|cfg| {
            Arc::new(
                crate::attachments::AttachmentStore::new(cfg, database.clone())
                    .expect("failed to create ATTACHMENTS_DIR"),
            )
        });
        DiscordLogger {
            database,
            catch_up_floors,
            started: AtomicBool::new(false),
            attachment_store,
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
        persist_message(&self.database, message).await.map(|_| ())
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

// Persist user + message + attachments. Shared by the live and backfill paths; issues zero extra
// REST calls (no channel resolution), so backfill stays within rate limits. Bad timestamps soft-fail
// (skip + log, no panic). Returns Ok(false) when the message was skipped, so callers don't write
// child rows (reaction_counts) that would FK-violate against the missing messages row.
pub(crate) async fn persist_message(
    db: &Database,
    message: &Message,
) -> Result<bool, tokio_postgres::Error> {
    let id = message.id.get() as i64;
    let Some(timestamp) = DateTime::from_timestamp(message.timestamp.unix_timestamp(), 0) else {
        eprintln!("skipping message {id}: out-of-range timestamp");
        return Ok(false);
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

    Ok(true)
}

// Reconcile a channel's pin state against Discord: fetch the live pin list (<=50 messages), diff it
// against the current_pins view, append pinned=true rows for new pins and pinned=false rows for
// vanished ones. Idempotent — identical sets write nothing, so RESUME duplicates and every-boot
// syncs are no-ops. Shared by the live channel_pins_update handler and the startup pass in backfill
// (which is also why this takes &Http rather than &Context).
pub(crate) async fn sync_channel_pins(
    http: &Http,
    db: &Database,
    channel_id: i64,
    guild_id: Option<i64>,
) -> Result<(), tokio_postgres::Error> {
    // soft-fail on REST error (missing access / deleted channel), like other REST paths
    let fetched_messages = match ChannelId::new(channel_id as u64).pins(http).await {
        Ok(pins) => pins,
        Err(e) => {
            eprintln!("pins: failed to fetch pins for channel {channel_id}: {e}");
            return Ok(());
        }
    };

    let stored: HashSet<i64> = db
        .current_pins_for_channel(channel_id)
        .await?
        .into_iter()
        .collect();
    let fetched: HashSet<i64> = fetched_messages.iter().map(|m| m.id.get() as i64).collect();
    if stored == fetched {
        return Ok(()); // steady state: no rows written
    }

    // like reactions, observation time stands in for the (unavailable) pin time
    let observed_at = Utc::now().naive_utc();

    // new pins: backfill the message row first (pin_events.message_id FK)
    for message in &fetched_messages {
        let message_id = message.id.get() as i64;
        if stored.contains(&message_id) {
            continue;
        }
        if !db.message_exists(message_id).await? {
            // degraded channel/guild rows so the messages FK holds (mirrors log_message's
            // fetch-failed branch; pins() messages carry no guild_id, so use the event's).
            // A later live message enriches name/guild via the COALESCE upsert.
            if let Some(guild_id) = guild_id {
                db.insert_guild(guild_id, None).await?;
            }
            db.insert_channel(channel_id, None, guild_id, message_id, message_id)
                .await?;
            if !persist_message(db, message).await? {
                // out-of-range timestamp skip: no messages row, so no pin row either
                eprintln!("pins: skipping pin for unpersistable message {message_id}");
                continue;
            }
        }
        db.insert_pin_event(message_id, channel_id, true, observed_at)
            .await?;
    }

    // vanished pins: these ids came from pin_events, so the messages row already exists
    for message_id in stored.difference(&fetched) {
        db.insert_pin_event(*message_id, channel_id, false, observed_at)
            .await?;
    }
    Ok(())
}

#[async_trait]
impl EventHandler for DiscordLogger {
    // fires per guild on every (re)connect now that the GUILDS intent is on; the upsert is
    // idempotent, and a transient DB error drops the event rather than panicking the task
    async fn guild_create(&self, _ctx: Context, guild: Guild, _is_new: Option<bool>) {
        if let Err(e) = self
            .database
            .insert_guild(guild.id.get() as i64, Some(&guild.name))
            .await
        {
            eprintln!("failed to log guild {}: {e}", guild.name);
            return;
        }

        println!("logged new guild: {}", guild.name)
    }

    // CHANNEL_PINS_UPDATE carries no message id and no pin/unpin discriminator,
    // so every event triggers a full fetch-and-diff of the channel's pin list.
    async fn channel_pins_update(&self, ctx: Context, pin: ChannelPinsUpdateEvent) {
        let channel_id = pin.channel_id.get() as i64;
        let guild_id = pin.guild_id.map(|g| g.get() as i64);
        // soft-fail: drop the event on a transient DB error rather than panicking the task
        if let Err(e) = sync_channel_pins(&ctx.http, &self.database, channel_id, guild_id).await {
            eprintln!("pins: failed to sync channel {channel_id}: {e}");
        }
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

        // download while the gateway-delivered CDN URL is still fresh (signed, ~24h); spawned so
        // a slow/large download never blocks the event loop, and after log_message so the metadata
        // rows are durable first. Failures leave no attachment_files row, so the next boot's drain
        // (which refreshes the URL via REST) is the retry path.
        if let Some(store) = &self.attachment_store {
            if !new_message.attachments.is_empty() {
                let store = store.clone();
                let attachments = new_message.attachments.clone();
                tokio::spawn(async move {
                    for attachment in &attachments {
                        if let Err(e) = store.store_attachment(attachment).await {
                            eprintln!("attachments: live download {}: {e}", attachment.id);
                        }
                    }
                });
            }
        }

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

        // spawn the boot-time tasks once per process (ready re-fires on reconnect; the swap is
        // idempotent). No opt-in flags — these are normal operating behavior; BACKFILL_DISABLE=1
        // and ATTACHMENTS_DISABLE=1 suppress them individually. Spawning keeps the gateway +
        // live logging responsive while they run.
        if !self.started.swap(true, Ordering::SeqCst) {
            if std::env::var("BACKFILL_DISABLE").as_deref() == Ok("1") {
                println!("backfill: disabled via BACKFILL_DISABLE");
            } else {
                let http = ctx.http.clone();
                let db = self.database.clone();
                let floors = self.catch_up_floors.clone();
                let cfg = crate::backfill::Config::from_env();
                tokio::spawn(async move {
                    crate::backfill::run(http, db, cfg, floors).await;
                });
            }
            // historical attachment drain, concurrent with backfill (own rate knob); the two
            // together stay around 2 REST GETs per delay tick, and serenity's per-route rate
            // limiter backstops any contention
            if let Some(store) = &self.attachment_store {
                let http = ctx.http.clone();
                let store = store.clone();
                tokio::spawn(async move {
                    crate::attachments::run(http, store).await;
                });
            }
        }
    }
}
