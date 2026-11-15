#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod anilist;
mod commands;
mod config;
mod db;
mod error;
mod feed;
mod guild;
mod i18n;
mod providers;
mod render;
mod ui;

use poise::serenity_prelude as serenity;
use sqlx::SqlitePool;

pub struct Data {
    #[allow(dead_code)] // used from milestone 4 on
    pub db: SqlitePool,
    #[allow(dead_code)] // used from milestone 8 on
    pub config: config::Config,
    pub anilist: std::sync::Arc<anilist::SafeAniList>,
    pub auto: ui::autocomplete::AutoState,
    pub http: reqwest::Client,
    pub renderer: std::sync::Arc<render::Renderer>,
    pub guilds: std::sync::Arc<guild::GuildStore>,
    pub providers: std::sync::Arc<providers::Providers>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level))
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting anpot");
    let pool = db::connect(&config.database_url).await?;
    let anilist = std::sync::Arc::new(anilist::SafeAniList::new(&config)?);
    let token = config.discord_token.expose().to_string();
    let dev_guild = config.dev_guild_id;
    let fallback = config.enable_fallback_providers;
    let (feed_pool, feed_api) = (pool.clone(), anilist.clone());

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands_with_cooldown(config.command_cooldown_secs),
            on_error: |e| Box::pin(error::on_error(e)),
            command_check: Some(|ctx| Box::pin(guild::command_check(ctx))),
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                let cmds = &framework.options().commands;
                // Registration overwrites the whole list in its scope, so removed commands disappear.
                match dev_guild {
                    Some(g) => {
                        poise::builtins::register_in_guild(ctx, cmds, serenity::GuildId::new(g)).await?;
                        // Old global commands would show up next to the guild ones: clear them.
                        serenity::Command::set_global_commands(&ctx.http, vec![]).await?;
                    }
                    None => poise::builtins::register_globally(ctx, cmds).await?,
                }
                let mut names = Vec::new();
                qualified_names(cmds, "", &mut names);
                let scope = match dev_guild {
                    Some(g) => format!("guild {g} (instant)"),
                    None => "global (can take up to an hour to appear)".to_string(),
                };
                tracing::info!(bot = %ready.user.name, %scope, count = names.len(), commands = %names.join(", "), "commands registered");
                let http = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(15))
                    .user_agent(concat!("anpot-discord-bot/", env!("CARGO_PKG_VERSION")))
                    .build()?;
                Ok(Data {
                    guilds: std::sync::Arc::new(guild::GuildStore::new(pool.clone())),
                    providers: std::sync::Arc::new(providers::Providers::new(http.clone(), fallback)),
                    renderer: std::sync::Arc::new(render::Renderer::new(http.clone())),
                    http,
                    db: pool,
                    config,
                    anilist,
                    auto: Default::default(),
                })
            })
        })
        .build();

    // Slash commands only: no privileged intents needed.
    let mut client =
        serenity::ClientBuilder::new(token, serenity::GatewayIntents::non_privileged())
            .framework(framework)
            .await?;

    // Background airing feed: stops with the process and never takes it down.
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let feed_task = tokio::spawn(feed::run(
        feed::Feed {
            pool: feed_pool,
            api: feed_api,
            http: client.http.clone(),
        },
        stop_rx,
    ));

    let shards = client.shard_manager.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutting down");
        let _ = stop_tx.send(true);
        // A watchdog thread (independent of the async runtime) guarantees the process ends even if the
        // gateway never connected and a clean shutdown would wait forever.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(10));
            eprintln!("gateway did not stop in time; exiting");
            std::process::exit(0);
        });
        tokio::spawn(async move { shards.shutdown_all().await });
    });

    let result = client.start().await;
    feed_task.abort();
    result?;
    Ok(())
}

type Command = poise::Command<Data, error::Error>;

fn apply_cooldown(cmd: &mut Command, secs: u64) {
    if let Ok(mut cfg) = cmd.cooldown_config.write() {
        cfg.user = (secs > 0).then(|| std::time::Duration::from_secs(secs));
    }
    for sub in &mut cmd.subcommands {
        apply_cooldown(sub, secs);
    }
}

/// `profile group create`-style names of every (sub)command, for the startup log.
fn qualified_names(cmds: &[Command], prefix: &str, out: &mut Vec<String>) {
    for c in cmds {
        let path = format!("{prefix}{}", c.name);
        out.push(path.clone());
        qualified_names(&c.subcommands, &format!("{path} "), out);
    }
}

/// Every command gets the configured per-user cooldown.
fn commands_with_cooldown(secs: u64) -> Vec<Command> {
    // Exactly the nine public commands; nothing else is ever registered.
    let mut cmds = vec![
        commands::profile::profile(),
        commands::search::search(),
        commands::airing::airing(),
        commands::random::random(),
        commands::top::top(),
        commands::recommend::recommend(),
        commands::game::game(),
        commands::settings::settings(),
        commands::help::help(),
    ];
    for c in &mut cmds {
        apply_cooldown(c, secs);
    }
    cmds
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

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(cmds: &[Command], prefix: &str, out: &mut Vec<String>) {
        for c in cmds {
            let name_ok = c.name.chars().count() <= 32 && c.name == c.name.to_lowercase();
            assert!(name_ok, "bad command name {}", c.name);
            assert!(
                c.description
                    .as_deref()
                    .is_some_and(|d| !d.is_empty() && d.chars().count() <= 100),
                "bad description for {}",
                c.name
            );
            assert!(
                c.name_localizations.contains_key("ar"),
                "{} lacks an Arabic name",
                c.name
            );
            assert!(
                c.description_localizations.contains_key("ar"),
                "{} lacks an Arabic description",
                c.name
            );
            for loc in c.name_localizations.values() {
                assert!(
                    loc.chars().count() <= 32 && *loc == loc.to_lowercase(),
                    "bad localized name {loc}"
                );
            }
            for p in &c.parameters {
                assert!(
                    p.name
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-'),
                    "{}.{} is not a valid Discord option name",
                    c.name,
                    p.name
                );
                assert!(p.name.chars().count() <= 32, "bad parameter {}", p.name);
                assert!(
                    p.name_localizations.contains_key("ar"),
                    "{}.{} lacks an Arabic name",
                    c.name,
                    p.name
                );
                assert!(
                    p.description
                        .as_deref()
                        .is_some_and(|d| d.chars().count() <= 100),
                    "bad description for {}.{}",
                    c.name,
                    p.name
                );
            }
            let path = format!("{prefix}{}", c.name);
            out.push(path.clone());
            walk(&c.subcommands, &format!("{path} "), out);
        }
    }

    #[test]
    fn all_commands_are_registered_localized_and_valid() {
        let cmds = commands_with_cooldown(3);
        let mut names = Vec::new();
        walk(&cmds, "", &mut names);
        let want = [
            "profile",
            "search",
            "airing",
            "random",
            "top",
            "recommend",
            "game",
            "settings",
            "help",
        ];
        assert_eq!(
            names, want,
            "exactly the nine public commands, in this order, no aliases or leftovers"
        );
        // Must serialise for Discord without panicking.
        assert!(!poise::builtins::create_application_commands(&cmds).is_empty());
    }

    #[test]
    fn cooldown_is_applied_to_every_command() {
        fn check(cmds: &[Command], secs: u64) {
            for c in cmds {
                let user = c.cooldown_config.read().map(|g| g.user).unwrap_or_default();
                assert_eq!(
                    user,
                    Some(std::time::Duration::from_secs(secs)),
                    "{}",
                    c.name
                );
                check(&c.subcommands, secs);
            }
        }
        check(&commands_with_cooldown(3), 3);
        let none = commands_with_cooldown(0);
        assert!(none[0]
            .cooldown_config
            .read()
            .map(|g| g.user.is_none())
            .unwrap_or(false));
    }
}
