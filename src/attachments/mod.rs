// Local storage of message attachment files, run alongside the message log. Two entry points:
//   * live  — the message handler spawns store_attachment while the gateway-delivered CDN URL is
//             still fresh (Discord signs attachment URLs; they expire after ~24h).
//   * drain — a boot-time historical pass over every attachments row without an attachment_files
//             row: the stored URLs have long expired, so it re-fetches each message via REST
//             (one get_message refreshes the URLs for all of that message's attachments) and
//             downloads newest-first with keyset pagination.
// attachment_files is the resolution log: stored / oversized / gone. No row at all means
// not-yet-attempted or transient failure — the next boot's drain retries, so the queue is a pure
// anti-join and needs no checkpoint table. Transient failures (network, IO, disk-full) soft-fail
// with a log line; only DB errors propagate, matching the rest of the codebase.
//
// The drain runs as its own task, concurrent with backfill rather than a fourth sequential pass:
// the first sweep can take days, and the newest URLs are the ones most worth preserving. Backfill's
// sweep pages carry fresh URLs too, but persist_message deliberately stays download-free (its
// zero-side-work contract keeps backfill paging predictable); the drain covers those rows instead.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use reqwest::StatusCode;
use serenity::futures::StreamExt;
use serenity::http::{Http, HttpError};
use serenity::model::channel::Attachment;
use serenity::model::id::{ChannelId, MessageId};
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;

use crate::logger::queries::Database;

const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024; // 100 MiB
const DEFAULT_FETCH_DELAY_MS: u64 = 750; // matches backfill's page delay
const HEARTBEAT_MESSAGES: u64 = 50; // drain progress cadence

pub struct Config {
    pub dir: PathBuf,
    pub max_bytes: u64,
    pub fetch_delay: Duration,
}

impl Config {
    // None = feature off (ATTACHMENTS_DISABLE=1 or ATTACHMENTS_DIR unset), so existing
    // deployments without the var keep working. Bad numeric values fall back to their
    // defaults rather than panicking, mirroring backfill::Config::from_env.
    pub fn from_env() -> Option<Self> {
        if std::env::var("ATTACHMENTS_DISABLE").as_deref() == Ok("1") {
            println!("attachments: disabled via ATTACHMENTS_DISABLE");
            return None;
        }
        let dir = match std::env::var("ATTACHMENTS_DIR") {
            Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => {
                println!("attachments: ATTACHMENTS_DIR unset, local attachment storage disabled");
                return None;
            }
        };
        let max_bytes = match std::env::var("ATTACHMENTS_MAX_BYTES") {
            Ok(raw) => raw.parse().unwrap_or_else(|_| {
                eprintln!(
                    "attachments: bad ATTACHMENTS_MAX_BYTES={raw}, using default {DEFAULT_MAX_BYTES}"
                );
                DEFAULT_MAX_BYTES
            }),
            Err(_) => DEFAULT_MAX_BYTES,
        };
        let fetch_delay_ms = match std::env::var("ATTACHMENTS_FETCH_DELAY_MS") {
            Ok(raw) => raw.parse().unwrap_or_else(|_| {
                eprintln!(
                    "attachments: bad ATTACHMENTS_FETCH_DELAY_MS={raw}, using default {DEFAULT_FETCH_DELAY_MS}ms"
                );
                DEFAULT_FETCH_DELAY_MS
            }),
            Err(_) => DEFAULT_FETCH_DELAY_MS,
        };
        Some(Config {
            dir,
            max_bytes,
            fetch_delay: Duration::from_millis(fetch_delay_ms),
        })
    }
}

pub struct AttachmentStore {
    config: Config,
    // one pooled client for every download (Attachment::download would build a fresh
    // client per call and buffer the whole file in memory)
    client: reqwest::Client,
    db: Arc<Database>,
}

impl AttachmentStore {
    pub fn new(config: Config, db: Arc<Database>) -> std::io::Result<Self> {
        std::fs::create_dir_all(&config.dir)?;
        Ok(AttachmentStore {
            config,
            client: reqwest::Client::new(),
            db,
        })
    }

    // Download one attachment using the URL already in hand; the caller guarantees freshness
    // (live gateway payload, or a message the drain just re-fetched).
    //   Ok(true)  — a row was written (stored / oversized / gone)
    //   Ok(false) — transient failure (HTTP non-4xx / IO / disk-full), logged; no row, so the
    //               next boot's drain retries with a refreshed URL
    //   Err       — DB error (propagates, like every other write path here)
    pub(crate) async fn store_attachment(
        &self,
        attachment: &Attachment,
    ) -> Result<bool, tokio_postgres::Error> {
        let id = attachment.id.get() as i64;
        let content_type = attachment.content_type.as_deref();

        // pre-download gate on Discord's reported size (zero network). The recorded size keeps
        // the row out of the pending set until the cap is raised above it.
        if attachment.size as u64 > self.config.max_bytes {
            self.db
                .upsert_attachment_file(
                    id,
                    "oversized",
                    None,
                    Some(attachment.size as i64),
                    content_type,
                    Utc::now().naive_utc(),
                )
                .await?;
            println!(
                "attachments: {id}: oversized ({} bytes), skipping",
                attachment.size
            );
            return Ok(true);
        }

        let name = sanitize_filename(&attachment.filename);
        let dir = self.config.dir.join(id.to_string());
        let final_path = dir.join(&name);
        // temp lives in the same directory => same filesystem => the rename below is atomic
        let temp_path = dir.join(format!("{name}.part"));

        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            eprintln!("attachments: {id}: failed to create dir: {e}");
            return Ok(false);
        }

        let response = match self.client.get(&attachment.url).send().await {
            Ok(response) => response,
            Err(e) => {
                eprintln!("attachments: {id}: request error: {e}");
                return Ok(false);
            }
        };
        let status = response.status();
        // the URL is fresh, so a CDN refusal means the object is gone upstream for good
        if status == StatusCode::NOT_FOUND || status == StatusCode::FORBIDDEN {
            self.db
                .upsert_attachment_file(
                    id,
                    "gone",
                    None,
                    None,
                    content_type,
                    Utc::now().naive_utc(),
                )
                .await?;
            println!("attachments: {id}: gone upstream ({status})");
            return Ok(true);
        }
        if !status.is_success() {
            eprintln!("attachments: {id}: http {status}, will retry next boot");
            return Ok(false);
        }
        // belt-and-braces: Discord's size field isn't load-bearing, check the header too
        if let Some(len) = response.content_length() {
            if len > self.config.max_bytes {
                self.db
                    .upsert_attachment_file(
                        id,
                        "oversized",
                        None,
                        Some(len as i64),
                        content_type,
                        Utc::now().naive_utc(),
                    )
                    .await?;
                println!("attachments: {id}: oversized per content-length ({len} bytes), skipping");
                return Ok(true);
            }
        }

        // stream to the temp file, counting bytes against the cap as a final backstop.
        // File::create truncates any stale .part left by a crashed run.
        let mut file = match tokio::fs::File::create(&temp_path).await {
            Ok(file) => file,
            Err(e) => {
                eprintln!("attachments: {id}: failed to create temp file: {e}");
                return Ok(false);
            }
        };
        let mut stream = response.bytes_stream();
        let mut written: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(e) => {
                    eprintln!("attachments: {id}: stream error: {e}");
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Ok(false);
                }
            };
            written += chunk.len() as u64;
            if written > self.config.max_bytes {
                // record the observed overrun, not Discord's (smaller) claimed size — otherwise
                // the same cap would immediately re-admit the row and loop forever
                let _ = tokio::fs::remove_file(&temp_path).await;
                self.db
                    .upsert_attachment_file(
                        id,
                        "oversized",
                        None,
                        Some(written as i64),
                        content_type,
                        Utc::now().naive_utc(),
                    )
                    .await?;
                eprintln!("attachments: {id}: stream exceeded cap at {written} bytes, aborted");
                return Ok(true);
            }
            if let Err(e) = file.write_all(&chunk).await {
                eprintln!("attachments: {id}: write error: {e}");
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Ok(false);
            }
        }
        if let Err(e) = file.sync_all().await {
            eprintln!("attachments: {id}: sync error: {e}");
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Ok(false);
        }
        drop(file);
        // atomic publish: a crash never leaves a half-file at the final path
        if let Err(e) = tokio::fs::rename(&temp_path, &final_path).await {
            eprintln!("attachments: {id}: rename error: {e}");
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Ok(false);
        }

        // path is stored relative to ATTACHMENTS_DIR so the directory can be relocated
        let rel_path = format!("{id}/{name}");
        self.db
            .upsert_attachment_file(
                id,
                "stored",
                Some(&rel_path),
                Some(written as i64),
                content_type,
                Utc::now().naive_utc(),
            )
            .await?;
        println!("attachments: stored {rel_path} ({written} bytes)");
        Ok(true)
    }
}

// Boot-time historical drain: walk messages newest-first that still owe a download, refresh their
// CDN URLs with one get_message per message, and download via the shared store. Single pass per
// boot. Keyset pagination (strictly-descending cursor) rather than re-querying the newest pending
// page: a transiently-failing attachment writes no row, so a "newest 100 pending" loop would spin
// on the same failing head all boot; with the cursor the pass always terminates, and stragglers
// (plus rows the backfill sweep inserts behind the cursor) are picked up next boot.
pub async fn run(http: Arc<Http>, store: Arc<AttachmentStore>) {
    let max_bytes = store.config.max_bytes as i64;
    let mut cursor = i64::MAX;
    let mut total_messages: u64 = 0;
    let mut total_files: u64 = 0;
    println!(
        "attachments: drain starting — fetch_delay={}ms, max_bytes={}",
        store.config.fetch_delay.as_millis(),
        store.config.max_bytes
    );

    loop {
        let batch = match store
            .db
            .pending_attachment_messages(cursor, max_bytes)
            .await
        {
            Ok(batch) => batch,
            Err(e) => {
                eprintln!("attachments: drain: failed to list pending messages: {e}");
                return;
            }
        };
        if batch.is_empty() {
            break;
        }

        for (message_id, channel_id) in &batch {
            match drain_message(&http, &store, *message_id, *channel_id).await {
                Ok(files) => total_files += files,
                Err(e) => {
                    eprintln!(
                        "attachments: drain {message_id}: db error, stopping for this boot: {e}"
                    );
                    return;
                }
            }
            total_messages += 1;
            if total_messages.is_multiple_of(HEARTBEAT_MESSAGES) {
                println!(
                    "attachments: drain — {total_messages} messages, {total_files} files (ongoing)"
                );
            }
            // rate limit: one REST get_message per tick, independent of backfill's budget
            sleep(store.config.fetch_delay).await;
        }

        cursor = batch
            .iter()
            .map(|(message_id, _)| *message_id)
            .min()
            .unwrap();
    }

    println!("attachments: drain finished — {total_messages} messages, {total_files} files");
}

// Refresh one message's attachment URLs via REST and download whatever is still pending.
// Returns the number of attachments resolved; REST failures soft-fail (transient errors leave
// no row for the next boot, a 404 marks every pending attachment gone). Only DB errors propagate.
async fn drain_message(
    http: &Http,
    store: &AttachmentStore,
    message_id: i64,
    channel_id: i64,
) -> Result<u64, tokio_postgres::Error> {
    let max_bytes = store.config.max_bytes as i64;
    let message = match http
        .get_message(
            ChannelId::new(channel_id as u64),
            MessageId::new(message_id as u64),
        )
        .await
    {
        Ok(message) => message,
        Err(serenity::Error::Http(HttpError::UnsuccessfulRequest(response)))
            if response.status_code == StatusCode::NOT_FOUND =>
        {
            // message (or channel) deleted upstream: fresh URLs are unobtainable forever
            let now = Utc::now().naive_utc();
            let pending = store
                .db
                .pending_attachments_for_message(message_id, max_bytes)
                .await?;
            for attachment_id in &pending {
                store
                    .db
                    .upsert_attachment_file(*attachment_id, "gone", None, None, None, now)
                    .await?;
            }
            println!(
                "attachments: drain {message_id}: message gone, marked {} attachment(s) gone",
                pending.len()
            );
            return Ok(0);
        }
        Err(e) => {
            // transient (rate limit, network, or 403 — lost channel access may be reinstated)
            eprintln!("attachments: drain {message_id}: fetch error: {e}");
            return Ok(0);
        }
    };

    // skip anything the live path (or an earlier partial drain of this message) already resolved
    let pending: HashSet<i64> = store
        .db
        .pending_attachments_for_message(message_id, max_bytes)
        .await?
        .into_iter()
        .collect();
    let mut files = 0;
    for attachment in &message.attachments {
        if !pending.contains(&(attachment.id.get() as i64)) {
            continue;
        }
        if store.store_attachment(attachment).await? {
            files += 1;
        }
    }
    Ok(files)
}

// Defense in depth against path traversal / unrepresentable names; the per-attachment-id
// directory already guarantees uniqueness, so post-sanitization collisions don't matter.
fn sanitize_filename(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            other => other,
        })
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return String::from("attachment");
    }
    // Linux NAME_MAX is 255 bytes; 200 leaves headroom for the ".part" suffix
    if cleaned.len() > 200 {
        let mut end = 200;
        while !cleaned.is_char_boundary(end) {
            end -= 1;
        }
        cleaned.truncate(end);
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::sanitize_filename;

    #[test]
    fn passes_normal_names_through() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(
            sanitize_filename("übersicht 2024.png"),
            "übersicht 2024.png"
        );
    }

    #[test]
    fn replaces_separators_and_nul() {
        assert_eq!(sanitize_filename("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_filename("a\\b/c"), "a_b_c");
        assert_eq!(sanitize_filename("nul\0byte"), "nul_byte");
    }

    #[test]
    fn falls_back_on_degenerate_names() {
        assert_eq!(sanitize_filename(""), "attachment");
        assert_eq!(sanitize_filename("."), "attachment");
        assert_eq!(sanitize_filename(".."), "attachment");
    }

    #[test]
    fn truncates_long_names_on_char_boundaries() {
        // 'ü' is 2 bytes; 150 of them = 300 bytes, truncated to <= 200 without splitting a char
        let long = "ü".repeat(150);
        let cleaned = sanitize_filename(&long);
        assert!(cleaned.len() <= 200);
        assert_eq!(cleaned, "ü".repeat(100));
    }
}
