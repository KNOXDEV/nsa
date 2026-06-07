use std::ops::Range;
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
use serenity::prelude::Mentionable;

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

/// Byte ranges in `content` wrapped in Discord spoiler tags (`||...||`).
/// Delimiters are paired greedily left-to-right; an unmatched trailing `||` is
/// not a spoiler and is ignored.
fn spoiler_ranges(content: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while let Some(open) = content[cursor..].find("||") {
        let open = cursor + open;
        let after_open = open + 2;
        let Some(close) = content[after_open..].find("||") else {
            break;
        };
        let close = after_open + close;
        ranges.push(open..close + 2);
        cursor = close + 2;
    }
    ranges
}

/// The result of scanning a message for links we rewrite.
struct Rewrites {
    /// The rewritten links, in order, each spoilered if its original was.
    links: Vec<String>,
    /// `true` when the message held no *words* outside the links we rewrote —
    /// i.e. stripping those links leaves nothing but whitespace and markdown
    /// formatting (`**`, `||`, `~~`, …). The original is then redundant and can
    /// be deleted. A non-rewritten URL or any prose counts as words and keeps
    /// this `false`.
    only_rewritten_links: bool,
}

/// Extract and rewrite every supported link in `content`. A link sitting inside
/// a Discord spoiler (`||...||`) yields a spoilered rewrite so we don't reveal
/// something the author deliberately hid.
fn rewrite_links(content: &str) -> Rewrites {
    let spoilers = spoiler_ranges(content);

    let mut links = Vec::new();
    let mut rewritten_spans: Vec<Range<usize>> = Vec::new();
    for caps in URL_REGEX.captures_iter(content) {
        let span = caps.get(0).unwrap().range();
        let spoilered = spoilers
            .iter()
            .any(|r| r.start <= span.start && span.end <= r.end);
        if let Some(link) = rewrite_captured_link(caps) {
            links.push(if spoilered {
                format!("||{link}||")
            } else {
                link
            });
            rewritten_spans.push(span);
        }
    }

    // Build what's left after removing the rewritten links. `captures_iter`
    // yields non-overlapping matches in order, so the spans are sorted.
    let mut leftover = String::with_capacity(content.len());
    let mut cursor = 0;
    for span in &rewritten_spans {
        leftover.push_str(&content[cursor..span.start]);
        cursor = span.end;
    }
    leftover.push_str(&content[cursor..]);

    // Only *words* outside the links keep the original alive. Markdown
    // formatting (`**`, `||`, `~~`, …) and whitespace carry no alphanumerics, so
    // a wrapped-but-wordless link still counts as link-only; prose or a
    // non-rewritten URL carries alphanumerics and does not.
    let only_rewritten_links = !links.is_empty() && !leftover.chars().any(char::is_alphanumeric);

    Rewrites {
        links,
        only_rewritten_links,
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

        // parse the message for links and rewrite the ones we care about,
        // preserving any spoiler wrapping on the original link
        let Rewrites {
            links: rewritten_links,
            only_rewritten_links,
        } = rewrite_links(new_message.content.as_str());

        rewritten_links
            .iter()
            .for_each(|link| println!("sending rewritten link: {}", link));

        // nothing we care about; leave the message untouched
        if rewritten_links.is_empty() {
            return;
        }

        // when the message was nothing but the links we rewrote, the original is
        // redundant: replace it outright with our rewrite, mentioning the author
        // so the lost attribution is preserved.
        if only_rewritten_links {
            // a single link reads better inline; multiple get their own lines
            let separator = if rewritten_links.len() == 1 { " " } else { "\n" };
            new_message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "{} sent:{}{}",
                        new_message.author.mention(),
                        separator,
                        rewritten_links.join("\n")
                    ),
                )
                .await
                .expect("Failed to send rewritten links");
            new_message
                .delete(&ctx.http)
                .await
                .expect("failed to delete original message");
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

        // keep the original message; just strip its embed-unfriendly link preview
        // to avoid a duplicate, ugly embed beside our rewrite.
        //
        // NOTE: embed suppression is message-wide (Discord's SUPPRESS_EMBEDS flag
        // is all-or-nothing — there's no per-embed removal, and on another user's
        // message the flag is the only field we can edit). So in the rare case of
        // a message mixing a rewritten link with a foreign embeddable one (e.g. a
        // twitter link *and* a youtube link), we also strip the foreign embed. We
        // accept that edge case to keep this simple.
        suppress_original_embeds(&ctx, &mut new_message, updates).await;
    }

    async fn ready(&self, _ctx: Context, _data_about_bot: Ready) {
        println!("link rewriter is online");
    }
}

#[cfg(test)]
mod test {
    use crate::link_rewriter::{rewrite_captured_link, rewrite_links, URL_REGEX};

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
    fn spoilered_link_yields_spoilered_rewrite() {
        let result = rewrite_links("||https://twitter.com/FAKEURL||");
        assert_eq!(result.links, vec!["||https://fxtwitter.com/FAKEURL||"]);
    }

    #[test]
    fn spoilered_link_with_surrounding_text() {
        let result = rewrite_links("look at this ||https://x.com/FAKEURL|| crazy");
        assert_eq!(result.links, vec!["||https://fxtwitter.com/FAKEURL||"]);
    }

    #[test]
    fn unspoilered_link_is_not_wrapped() {
        let result = rewrite_links("https://twitter.com/FAKEURL");
        assert_eq!(result.links, vec!["https://fxtwitter.com/FAKEURL"]);
    }

    #[test]
    fn mixed_spoilered_and_plain_links() {
        let result = rewrite_links("||https://twitter.com/A|| and https://bsky.app/B");
        assert_eq!(
            result.links,
            vec!["||https://fxtwitter.com/A||", "https://fxbsky.app/B"]
        );
    }

    #[test]
    fn unmatched_spoiler_delimiter_is_ignored() {
        // a lone trailing `||` doesn't open a spoiler region
        let result = rewrite_links("https://twitter.com/FAKEURL ||");
        assert_eq!(result.links, vec!["https://fxtwitter.com/FAKEURL"]);
    }

    #[test]
    fn bare_link_is_only_rewritten_links() {
        assert!(rewrite_links("https://twitter.com/FAKEURL").only_rewritten_links);
    }

    #[test]
    fn bare_link_with_surrounding_whitespace_is_only_rewritten_links() {
        assert!(rewrite_links("  https://twitter.com/FAKEURL\n").only_rewritten_links);
    }

    #[test]
    fn multiple_bare_links_are_only_rewritten_links() {
        assert!(rewrite_links("https://twitter.com/A https://bsky.app/B").only_rewritten_links);
    }

    #[test]
    fn spoilered_bare_link_is_only_rewritten_links() {
        // spoiler delimiters are formatting, not words
        assert!(rewrite_links("||https://twitter.com/FAKEURL||").only_rewritten_links);
    }

    #[test]
    fn formatted_bare_link_is_only_rewritten_links() {
        // bold markers are formatting, not words
        assert!(rewrite_links("**https://twitter.com/FAKEURL**").only_rewritten_links);
    }

    #[test]
    fn link_with_extra_text_is_not_only_rewritten_links() {
        assert!(!rewrite_links("look at this https://twitter.com/FAKEURL").only_rewritten_links);
    }

    #[test]
    fn link_with_text_inside_spoiler_is_not_only_rewritten_links() {
        assert!(!rewrite_links("||secret https://twitter.com/FAKEURL||").only_rewritten_links);
    }

    #[test]
    fn rewritten_link_alongside_foreign_link_is_not_only_rewritten_links() {
        // the unrewritten link is additional content we must not delete
        let result = rewrite_links("https://twitter.com/A https://youtube.com/B");
        assert_eq!(result.links, vec!["https://fxtwitter.com/A"]);
        assert!(!result.only_rewritten_links);
    }

    #[test]
    fn no_rewritten_links_is_not_only_rewritten_links() {
        assert!(!rewrite_links("just some text, no links").only_rewritten_links);
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
