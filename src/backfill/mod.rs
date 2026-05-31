// Historical message backfill, run on every boot (unless BACKFILL_DISABLE=1). Two passes, both
// persisting through the live capture path (persist_message) and paging with the exclusive `before`
// cursor, one REST GET per page:
//   * catch-up — restores the just-ended downtime gap: page each channel from its head down to the
//                pre-gateway high-water snapshot (the floor).
//   * sweep    — the one-time deep download: page newest -> oldest to channel start, checkpointed in
//                backfill_state and never re-run once `complete`.
// Every failure soft-fails (log + abort that channel for this boot, no checkpoint advance), so a
// transient error re-pages at most one page rather than dropping a message.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{NaiveDateTime, Utc};
use serenity::builder::GetMessages;
use serenity::http::Http;
use serenity::model::channel::{Message, ReactionType};
use serenity::model::id::{ChannelId, MessageId};
use tokio::time::sleep;

use crate::logger::persist_message;
use crate::logger::queries::Database;

const DEFAULT_PAGE_DELAY_MS: u64 = 750;
const PAGE_LIMIT: u8 = 100; // Discord's GetMessages cap
const HEARTBEAT_PAGES: u64 = 25; // sweep progress cadence, so a huge channel isn't silent for hours

pub struct Config {
    pub page_delay: Duration,
}

impl Config {
    // A bad BACKFILL_PAGE_DELAY_MS falls back to the default rather than panicking.
    pub fn from_env() -> Self {
        let page_delay_ms = match std::env::var("BACKFILL_PAGE_DELAY_MS") {
            Ok(raw) => raw.parse().unwrap_or_else(|_| {
                eprintln!(
                    "backfill: bad BACKFILL_PAGE_DELAY_MS={raw}, using default {DEFAULT_PAGE_DELAY_MS}ms"
                );
                DEFAULT_PAGE_DELAY_MS
            }),
            Err(_) => DEFAULT_PAGE_DELAY_MS,
        };
        Config {
            page_delay: Duration::from_millis(page_delay_ms),
        }
    }
}

pub async fn run(http: Arc<Http>, db: Arc<Database>, cfg: Config, floors: HashMap<i64, i64>) {
    let channels = match db.list_backfill_channels().await {
        Ok(channels) => channels,
        Err(e) => {
            eprintln!("backfill: failed to list channels: {e}");
            return;
        }
    };
    println!(
        "backfill: starting — {} channels, page_delay={}ms",
        channels.len(),
        cfg.page_delay.as_millis()
    );

    // catch-up first (restores the just-ended downtime gap), then the long historical sweep.
    for (channel_id, _) in &channels {
        catch_up_channel(
            &http,
            &db,
            *channel_id,
            floors.get(channel_id).copied(),
            &cfg,
        )
        .await;
    }
    for (channel_id, _) in &channels {
        sweep_channel(&http, &db, *channel_id, &cfg).await;
    }

    println!("backfill: finished");
}

// Fetch one page (newest-first), oldest-exclusive `before` cursor. None = start at the live head.
async fn fetch_page(
    http: &Http,
    channel_id: ChannelId,
    before: Option<i64>,
) -> serenity::Result<Vec<Message>> {
    let mut builder = GetMessages::new().limit(PAGE_LIMIT);
    if let Some(before) = before {
        builder = builder.before(MessageId::new(before as u64));
    }
    channel_id.messages(http, builder).await
}

// Persist a whole page before any checkpoint advances. One observed_at per page matches log_reaction.
async fn persist_page(db: &Database, messages: &[Message]) -> Result<(), tokio_postgres::Error> {
    let observed_at = Utc::now().naive_utc();
    for message in messages {
        // a skipped message has no messages row, so its reaction_counts would FK-violate and stall
        // the sweep; only record counts once the message itself persisted.
        if persist_message(db, message).await? {
            persist_reaction_counts(db, message, observed_at).await?;
        }
    }
    Ok(())
}

// Record the userless aggregate reaction counts carried in a GetMessages payload (Message.reactions
// has emoji + count, but not who reacted). Sweep-only: live messages arrive with no reactions and
// per-user events keep current_reactions accurate. Customs get an emojis row first to satisfy the FK.
async fn persist_reaction_counts(
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

fn min_id(messages: &[Message]) -> i64 {
    messages.iter().map(|m| m.id.get() as i64).min().unwrap()
}

fn max_id(messages: &[Message]) -> i64 {
    messages.iter().map(|m| m.id.get() as i64).max().unwrap()
}

// The one-time deep download. Pages newest -> oldest to channel start, checkpointing
// oldest_backfilled_id each page; once it reaches the start it marks `complete` and never re-runs.
async fn sweep_channel(http: &Http, db: &Database, channel_id: i64, cfg: &Config) {
    let cid = ChannelId::new(channel_id as u64);

    let (partial_existed, mut cursor) = match db.backfill_state(channel_id).await {
        Ok(Some((_, true))) => return, // already complete — silent steady-state no-op
        Ok(Some((oldest, false))) => (true, oldest),
        Ok(None) => (false, None),
        Err(e) => {
            eprintln!("backfill: sweep {channel_id}: failed to read state: {e}");
            return;
        }
    };

    if partial_existed {
        println!("backfill: sweep {channel_id}: resume from {cursor:?}");
    } else {
        println!("backfill: sweep {channel_id}: starting");
    }

    let mut wrote_any = false;
    let mut pages: u64 = 0;
    let mut total: u64 = 0;

    loop {
        let batch = match fetch_page(http, cid, cursor).await {
            Ok(batch) => batch,
            Err(e) => {
                eprintln!("backfill: sweep {channel_id}: fetch error, resuming next boot: {e}");
                return;
            }
        };

        if batch.is_empty() {
            // Completion guard: only stamp complete if we actually paged something — either this
            // run wrote a message or we're finishing an earlier partial sweep. A never-swept channel
            // whose very first page is empty (permission blip / transient Ok(empty)) is left
            // un-checkpointed so the next boot retries, rather than abandoning its history forever.
            if wrote_any || partial_existed {
                if let Err(e) = db.upsert_backfill_state(channel_id, cursor, true).await {
                    eprintln!("backfill: sweep {channel_id}: failed to mark complete: {e}");
                    return;
                }
                println!("backfill: sweep {channel_id}: complete — {total} msgs, {pages} pages");
            } else {
                eprintln!(
                    "backfill: sweep {channel_id}: empty first page, leaving un-checkpointed for retry"
                );
            }
            return;
        }

        // persist the entire batch BEFORE advancing the checkpoint (no-gap guarantee)
        if let Err(e) = persist_page(db, &batch).await {
            eprintln!("backfill: sweep {channel_id}: persist error, resuming next boot: {e}");
            return;
        }
        wrote_any = true;
        total += batch.len() as u64;
        pages += 1;

        cursor = Some(min_id(&batch));
        if let Err(e) = db.upsert_backfill_state(channel_id, cursor, false).await {
            eprintln!("backfill: sweep {channel_id}: failed to checkpoint: {e}");
            return;
        }

        if pages.is_multiple_of(HEARTBEAT_PAGES) {
            println!("backfill: sweep {channel_id}: {total} msgs, {pages} pages (ongoing)");
        }
        sleep(cfg.page_delay).await;
    }
}

// Restores the downtime gap. Pages from the live head down to `floor` (the channels.last_message_id
// snapshot taken in main.rs before the gateway connected), fetching exactly (floor, head].
// Interruption-safe: the floor only advances after the gap is fully paged.
async fn catch_up_channel(
    http: &Http,
    db: &Database,
    channel_id: i64,
    floor: Option<i64>,
    cfg: &Config,
) {
    // unknown at boot -> nothing to catch up; the historical pass (or next boot) covers it
    let Some(floor) = floor else {
        return;
    };
    let cid = ChannelId::new(channel_id as u64);

    let mut cursor: Option<i64> = None; // None -> first page starts at the live head
    let mut head: Option<i64> = None; // the live head, captured from the first non-empty page
    let mut pages: u64 = 0;
    let mut total: u64 = 0;

    loop {
        let batch = match fetch_page(http, cid, cursor).await {
            Ok(batch) => batch,
            Err(e) => {
                eprintln!("backfill: catch-up {channel_id}: fetch error, resuming next boot: {e}");
                return;
            }
        };

        if batch.is_empty() {
            break; // reached the end (or empty first page -> head stays None -> no bump)
        }

        if head.is_none() {
            head = Some(max_id(&batch));
        }

        if let Err(e) = persist_page(db, &batch).await {
            eprintln!("backfill: catch-up {channel_id}: persist error, resuming next boot: {e}");
            return;
        }
        total += batch.len() as u64;
        pages += 1;

        // reached known-contiguous territory: everything older than this is already captured
        let oldest = min_id(&batch);
        if oldest <= floor {
            break;
        }
        cursor = Some(oldest);
        sleep(cfg.page_delay).await;
    }

    // Bump the floor to the head we just reached so a quiet channel doesn't re-page the same gap
    // each restart. Empty first page (head == None) -> no bump, next boot retries the empty page.
    if let Some(head) = head {
        if let Err(e) = db.bump_last_message_id(channel_id, head).await {
            eprintln!("backfill: catch-up {channel_id}: failed to bump floor: {e}");
            return;
        }
        // steady-state stops in a single head page; only report when an actual gap was paged
        if pages > 1 {
            println!("backfill: catch-up {channel_id}: {total} msgs, {pages} pages, head {head}");
        }
    }
}
