use serenity::model::prelude::*;
use serenity::prelude::*;
use std::env;

mod logger;
mod tables;

use logger::DiscordLogger;
use tables::init_tables;

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

    // connect to discord
    let token = env::var("DISCORD_TOKEN").expect("no environment variable DISCORD_TOKEN provided");
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(DiscordLogger::new(postgres_client).await)
        .await
        .expect("discord auth failed");

    client.start().await.expect("Client error change");
}
