// Historical message backfill. Two passes, both running on every boot (unless BACKFILL_DISABLE=1),
// both persisting through the same path as live capture (logger::persist_message), both paging with
// the exclusive `before` cursor at one REST call per page:
//
//   * catch-up  (§3b) restores the gap created by the downtime that just ended: page each channel
//                from its current head down to the pre-gateway high-water snapshot (the floor).
//   * sweep     (§3a) the one-time deep download: page each channel newest -> oldest down to channel
//                start, checkpointed in backfill_state and never re-run once `complete`.
//
// Pure orchestration: holds no prepared statements (those stay centralized in Database). Every
// failure soft-fails (log + abort that channel for this boot, no checkpoint advance) so a transient
// REST/DB error costs at most one re-paged page, never a dropped message.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serenity::builder::GetMessages;
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::id::{ChannelId, MessageId};
use tokio::time::sleep;

use crate::logger::queries::Database;
use crate::logger::{persist_message, persist_reaction_counts};

const DEFAULT_PAGE_DELAY_MS: u64 = 750;
// Discord caps GetMessages at 100; one page == one REST GET.
const PAGE_LIMIT: u8 = 100;
// emit a sweep progress line every this many pages so a huge channel isn't silent for hours
const HEARTBEAT_PAGES: u64 = 25;

pub struct Config {
    pub page_delay: Duration,
}

impl Config {
    // Never panics: an unparseable BACKFILL_PAGE_DELAY_MS is logged and falls back to the default,
    // so a typo in a deploy env can't silently disable backfill.
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

// Persist an entire page (message + reaction counts) before any checkpoint advances. A single
// observed_at for the page matches log_reaction's clock semantics.
async fn persist_page(db: &Database, messages: &[Message]) -> Result<(), tokio_postgres::Error> {
    let observed_at = Utc::now().naive_utc();
    for message in messages {
        // skip reaction counts for a skipped message: its messages row was never inserted, so the
        // reaction_counts FK would violate and abort (and stall) the whole sweep.
        if persist_message(db, message).await? {
            persist_reaction_counts(db, message, observed_at).await?;
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

// §3a — the one-time deep download. Pages newest -> oldest to channel start, checkpointing
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

// §3b — restores the downtime gap. Pages from the live head down to `floor` (the channels
// .last_message_id snapshot taken in main.rs before the gateway connected), so it fetches exactly
// (floor, head]. Interruption-safe: the floor only advances (step 4) after the gap is fully paged.
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

    let mut cursor: Option<i64> = None; // first page has no `before` -> the current head
    let mut head: Option<i64> = None; // set only from the first non-empty page (the live head)
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
        if min_id(&batch) <= floor {
            break;
        }
        cursor = Some(min_id(&batch));
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
