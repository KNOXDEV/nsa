use std::time::Duration;

use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serenity::async_trait;
use serenity::builder::EditMessage;
use serenity::client::{Context, EventHandler};
use serenity::collector::collect;
use serenity::futures::{Stream, StreamExt};
use serenity::model::channel::Message;
use serenity::model::event::Event;
use serenity::model::gateway::Ready;

lazy_static! {
    static ref URL_REGEX: Regex = Regex::new(
        r"https?:\/\/(?:www\.)?([-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-zA-Z0-9()]{1,6})\b[-a-zA-Z0-9()@:%_\+.~#?&//=]*"
    ).unwrap();
}

fn rewrite_captured_link(caps: Captures) -> Option<String> {
    let original_message = caps.get(0).unwrap();

    println!(
        "matched message: {} - {}",
        original_message.as_str(),
        caps.get(1).unwrap().as_str()
    );

    match caps.get(1) {
        Some(mat) => match mat.as_str() {
            "twitter.com" | "x.com" => {
                let original_url = original_message.as_str().to_owned();
                Some(
                    [
                        &original_url[..(mat.start() - original_message.start())],
                        "fxtwitter.com",
                        &original_url[(mat.end() - original_message.start())..],
                    ]
                    .join(""),
                )
            }
            "bsky.app" => {
                let original_url = original_message.as_str().to_owned();
                Some(
                    [
                        &original_url[..(mat.start() - original_message.start())],
                        "fxbsky.app",
                        &original_url[(mat.end() - original_message.start())..],
                    ]
                    .join(""),
                )
            }
            "pixiv.net" => {
                let original_url = original_message.as_str().to_owned();
                Some(
                    [
                        &original_url[..(mat.start() - original_message.start())],
                        "phixiv.net",
                        &original_url[(mat.end() - original_message.start())..],
                    ]
                    .join(""),
                )
            }
            "atlantic.com" | "nytimes.com" => {
                let mut url = "https://yeet.knx.pw/".to_owned();
                url.push_str(caps.get(0).unwrap().as_str());
                Some(url)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Suppress the link-preview embeds on the original message so we don't end up
/// with a duplicate, embed-unfriendly preview sitting next to our rewrite.
///
/// Discord generates link embeds asynchronously *after* a message is created and
/// can add more later (slow links, multiple links, server load). `updates` is a
/// stream of embed-bearing `MessageUpdate`s for this message, registered by the
/// caller *before* the reply so we don't miss the first one. We suppress on each
/// update — re-applying the flag is idempotent — and give up once 10s pass with
/// no further update.
///
/// Suppressing another user's message requires the `MANAGE_MESSAGES` permission.
async fn suppress_original_embeds<S>(ctx: &Context, message: &mut Message, updates: S)
where
    S: Stream<Item = ()>,
{
    tokio::pin!(updates);

    // a cached embed for a previously-seen link ships in the create payload with
    // no MessageUpdate to follow, so suppress it up front
    if !message.embeds.is_empty() {
        suppress_embeds_now(ctx, message).await;
    }

    // otherwise embeds arrive asynchronously: suppress on each update, giving up
    // once 10s pass with no further update
    while let Ok(Some(())) = tokio::time::timeout(Duration::from_secs(10), updates.next()).await {
        suppress_embeds_now(ctx, message).await;
    }
}

/// Apply the `SUPPRESS_EMBEDS` flag to a message, logging on failure.
async fn suppress_embeds_now(ctx: &Context, message: &mut Message) {
    if let Err(why) = message
        .edit(&ctx.http, EditMessage::new().suppress_embeds(true))
        .await
    {
        eprintln!("failed to suppress embeds on original message: {why}");
    }
}

pub(crate) struct DiscordLinkRewriter;

impl DiscordLinkRewriter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventHandler for DiscordLinkRewriter {
    async fn message(&self, ctx: Context, mut new_message: Message) {
        // never rewrite yourself (or other bots)
        if new_message.author.bot {
            return;
        }

        println!(
            "processing message: {} - {}",
            new_message.author, new_message.content
        );

        // parse the message for links
        // and check for links we care about
        let rewritten_links: Vec<String> = URL_REGEX
            .captures_iter(new_message.content.as_str())
            .filter_map(rewrite_captured_link)
            .collect();

        rewritten_links
            .iter()
            .for_each(|link| println!("sending rewritten link: {}", link));

        // nothing we care about; leave the message untouched
        if rewritten_links.is_empty() {
            return;
        }

        // register the embed-update collector BEFORE replying so we don't miss
        // the MessageUpdate that carries Discord's async-generated link preview;
        // only match updates that actually carry embeds
        let msg_id = new_message.id;
        let updates = collect(&ctx.shard, move |ev| match ev {
            Event::MessageUpdate(update)
                if update.id == msg_id
                    && update
                        .embeds
                        .as_ref()
                        .is_some_and(|embeds| !embeds.is_empty()) =>
            {
                Some(())
            }
            _ => None,
        });

        // send reply with rewritten links
        new_message
            .reply(&ctx.http, rewritten_links.join("\n"))
            .await
            .expect("Failed to reply to message containing links");

        // always keep the original message; just strip its embed-unfriendly link
        // preview to avoid a duplicate, ugly embed beside our rewrite
        suppress_original_embeds(&ctx, &mut new_message, updates).await;
    }

    async fn ready(&self, _ctx: Context, _data_about_bot: Ready) {
        println!("link rewriter is online");
    }
}

#[cfg(test)]
mod test {
    use crate::link_rewriter::{rewrite_captured_link, URL_REGEX};

    #[test]
    fn rewrite_twitter() {
        let test_link = "https://twitter.com/FAKEURL";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap()).unwrap();
        assert_eq!(result_link, "https://fxtwitter.com/FAKEURL")
    }

    #[test]
    fn rewrite_twitter_with_prefix_and_suffix() {
        let test_link = "Heres some stuff: https://twitter.com/FAKEURL and here's even more stuff";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap()).unwrap();
        assert_eq!(result_link, "https://fxtwitter.com/FAKEURL")
    }

    #[test]
    fn rewrite_xcom() {
        let test_link = "https://x.com/FAKEURL";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap()).unwrap();
        assert_eq!(result_link, "https://fxtwitter.com/FAKEURL")
    }

    #[test]
    fn rewrite_bluesky() {
        let test_link = "https://bsky.app/profile/FAKEHANDLE.bsky.social/post/FAKEPOST";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap()).unwrap();
        assert_eq!(
            result_link,
            "https://fxbsky.app/profile/FAKEHANDLE.bsky.social/post/FAKEPOST"
        )
    }

    #[test]
    fn dont_rewrite_fxtwitter() {
        let test_link = "https://fxtwitter.com/FAKEURL";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap());
        assert_eq!(result_link, None)
    }

    #[test]
    fn dont_rewrite_fxbsky() {
        let test_link = "https://fxbsky.app/profile/FAKEHANDLE.bsky.social/post/FAKEPOST";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap());
        assert_eq!(result_link, None)
    }

    #[test]
    fn rewrite_pixiv() {
        let test_link = "https://www.pixiv.net/FAKEURL";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap()).unwrap();
        assert_eq!(result_link, "https://www.phixiv.net/FAKEURL")
    }

    #[test]
    fn rewrite_nytimes() {
        let test_link = "https://www.nytimes.com/FAKEURL";

        let result_link = rewrite_captured_link(URL_REGEX.captures(test_link).unwrap()).unwrap();
        assert_eq!(
            result_link,
            "https://yeet.knx.pw/https://www.nytimes.com/FAKEURL"
        )
    }
}
