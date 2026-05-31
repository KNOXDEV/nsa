use serenity::async_trait;
use serenity::model::prelude::*;
use serenity::prelude::*;
use std::collections::HashMap;
use std::env;

mod backfill;
mod link_rewriter;
mod logger;
mod tables;

use crate::link_rewriter::DiscordLinkRewriter;
use logger::DiscordLogger;
use tables::init_tables;

// Serenity's ClientBuilder::event_handler replaces rather than appends; the
// last call wins. Wrapping both handlers behind one EventHandler keeps both
// alive.
struct Handlers {
    logger: DiscordLogger,
    link_rewriter: DiscordLinkRewriter,
}

#[async_trait]
impl EventHandler for Handlers {
    async fn message(&self, ctx: Context, new_message: Message) {
        self.link_rewriter
            .message(ctx.clone(), new_message.clone())
            .await;
        self.logger.message(ctx, new_message).await;
    }

    async fn ready(&self, ctx: Context, data: Ready) {
        self.link_rewriter.ready(ctx.clone(), data.clone()).await;
        self.logger.ready(ctx, data).await;
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        self.logger.guild_create(ctx, guild, is_new).await;
    }

    async fn reaction_add(&self, ctx: Context, add_reaction: Reaction) {
        self.logger.reaction_add(ctx, add_reaction).await;
    }

    async fn reaction_remove(&self, ctx: Context, removed_reaction: Reaction) {
        self.logger.reaction_remove(ctx, removed_reaction).await;
    }
}

#[tokio::main]
async fn main() {
    // connect to locally running postgres
    let pg_host = env::var("PG_HOST").expect("no environment variable PG_HOST provided");
    let pg_password =
        env::var("PG_PASSWORD").expect("no environment variable PG_PASSWORD provided");
    let pg_user = env::var("PG_USER").expect("no environment variable PG_USER provided");
    let config = format!("host={} user={} password={}", pg_host, pg_user, pg_password);
    let (postgres_client, connection) = tokio_postgres::connect(&config, tokio_postgres::NoTls)
        .await
        .expect("failed to connect to postgres server");

    tokio::spawn(async move {
        connection.await.expect("connection error");
    });

    init_tables(&postgres_client)
        .await
        .expect("failed to initialize database");

    // Snapshot the per-channel catch-up floors (last_message_id high-water) BEFORE the gateway
    // connects. This ordering is load-bearing: once client.start() runs, incoming message/reaction
    // events bump last_message_id past the downtime gap within milliseconds, so reading the floor
    // after connect would silently skip the gap forever. runs after init_tables so `channels` exists.
    let catch_up_floors: HashMap<i64, i64> = postgres_client
        .query("SELECT id, last_message_id FROM channels", &[])
        .await
        .expect("failed to snapshot catch-up floors")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();

    // connect to discord
    let token = env::var("DISCORD_TOKEN").expect("no environment variable DISCORD_TOKEN provided");
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_MESSAGE_REACTIONS;

    let handlers = Handlers {
        logger: DiscordLogger::new(postgres_client, catch_up_floors).await,
        link_rewriter: DiscordLinkRewriter::new(),
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handlers)
        .await
        .expect("discord auth failed");

    client.start().await.expect("Client error change");
}
