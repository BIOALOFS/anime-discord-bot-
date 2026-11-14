#[allow(dead_code)] // consumed by commands from milestone 3 on
mod anilist;
mod commands;
mod config;
mod db;
mod error;
mod i18n;

use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

pub struct Data {
    #[allow(dead_code)] // used from milestone 2 on
    pub db: SqlitePool,
    #[allow(dead_code)]
    pub config: config::Config,
    #[allow(dead_code)]
    pub anilist: std::sync::Arc<anilist::AniListClient>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level))
        .init();

    let pool = db::connect(&config.database_url).await?;
    let anilist = std::sync::Arc::new(anilist::AniListClient::new(&config)?);
    let token = config.discord_token.clone();
    let dev_guild = config.dev_guild_id;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![commands::ping()],
            on_error: |e| Box::pin(error::on_error(e)),
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let cmds = &framework.options().commands;
                match dev_guild {
                    Some(g) => {
                        poise::builtins::register_in_guild(ctx, cmds, serenity::GuildId::new(g))
                            .await?
                    }
                    None => poise::builtins::register_globally(ctx, cmds).await?,
                }
                tracing::info!(bot = %ready.user.name, "ready");
                Ok(Data {
                    db: pool,
                    config,
                    anilist,
                })
            })
        })
        .build();

    // Slash commands only: no privileged intents needed.
    let mut client =
        serenity::ClientBuilder::new(token, serenity::GatewayIntents::non_privileged())
            .framework(framework)
            .await?;

    let shards = client.shard_manager.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutting down");
        shards.shutdown_all().await;
    });

    client.start().await?;
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = term.recv() => {},
                _ = tokio::signal::ctrl_c() => {},
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}
