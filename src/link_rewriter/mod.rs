use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::channel::Message;
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

    return match caps.get(1) {
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
    };
}

fn should_delete_og_message(links: &Vec<String>, original_message: &str) -> bool {
    links.len() == 1 && !original_message.trim().contains(" ")
}

pub(crate) struct DiscordLinkRewriter;

impl DiscordLinkRewriter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventHandler for DiscordLinkRewriter {
    async fn message(&self, ctx: Context, new_message: Message) {
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

        let should_delete =
            should_delete_og_message(&rewritten_links, new_message.content.as_str());

        let reply_message = if should_delete {
            format!(
                "{} sent:\n {}",
                new_message.author.mention(),
                rewritten_links.join("\n")
            )
        } else {
            rewritten_links.join("\n")
        };

        // send reply with rewritten links
        if !rewritten_links.is_empty() {
            new_message
                .reply(&ctx.http, reply_message)
                .await
                .expect("Failed to reply to message containing links");
        }

        if should_delete {
            new_message
                .delete(&ctx.http)
                .await
                .expect("failed to delete OG message");
        }
    }

    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        println!("link rewriter is online");
    }
}

#[cfg(test)]
mod test {
    use crate::link_rewriter::{rewrite_captured_link, should_delete_og_message, URL_REGEX};

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
    fn dont_rewrite_fxtwitter() {
        let test_link = "https://fxtwitter.com/FAKEURL";

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

    #[test]
    fn correctly_determine_og_message_should_be_deleted() {
        let test_link = "https://www.nytimes.com/FAKEURL";

        let rewritten_links: Vec<String> = URL_REGEX
            .captures_iter(test_link)
            .filter_map(rewrite_captured_link)
            .collect();

        let should_delete = should_delete_og_message(&rewritten_links, test_link);

        assert_eq!(should_delete, true)
    }

    #[test]
    fn correctly_determine_og_message_should_not_be_deleted() {
        let test_link = "Here is a cool link: https://www.pixiv.net/FAKEURL";

        let rewritten_links: Vec<String> = URL_REGEX
            .captures_iter(test_link)
            .filter_map(rewrite_captured_link)
            .collect();

        let should_delete = should_delete_og_message(&rewritten_links, test_link);

        assert_eq!(should_delete, false)
    }
}
