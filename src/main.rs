mod tables;

use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::prelude::*;
use serenity::prelude::*;
use std::env;
use tables::init_tables;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    // log every message and its reactions, etc
    // WIP
    async fn message(&self, ctx: Context, new_message: Message) {
        println!(
            "[{}] {} in #{}: {}",
            new_message.timestamp,
            new_message.author.name,
            new_message.channel_id,
            new_message.content
        );

        // randomly react to incoming messages 0.5% of the time
        if rand::random::<f64>() < 0.005 {
            new_message
                .react(ctx, ReactionType::Unicode(String::from("👀")))
                .await
                .expect("failed to react creepily");
        }
    }

    async fn ready(&self, _ctx: Context, _data_about_bot: Ready) {
        println!("client is now ready");
    }
}

#[tokio::main]
async fn main() {
    // connect to locally running postgres
    let (client, connection) =
        tokio_postgres::connect("host=localhost user=postgres", tokio_postgres::NoTls)
            .await
            .expect("failed to connect to postgres server");

    tokio::spawn(async move {
        connection.await.expect("connection error");
    });

    let rows = client
        .query("SELECT $1::TEXT", &[&"hello world"])
        .await
        .expect("client not found");
    let value: &str = rows[0].get(0);
    println!("database replied with: {}", value);

    init_tables(&client)
        .await
        .expect("failed to initialize database");

    // connect to discord
    let token = env::var("DISCORD_TOKEN").expect("no environment variable DISCORD_TOKEN provided");
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .await
        .expect("discord auth failed");

    client.start().await.expect("Client error");
}
