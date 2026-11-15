#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod anilist;
mod commands;
mod config;
mod db;
mod error;
mod i18n;
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

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands_with_cooldown(config.command_cooldown_secs),
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
                let mut names = Vec::new();
                qualified_names(cmds, "", &mut names);
                let scope = match dev_guild {
                    Some(g) => format!("guild {g} (instant)"),
                    None => "global (can take up to an hour to appear)".to_string(),
                };
                tracing::info!(bot = %ready.user.name, %scope, count = names.len(), commands = %names.join(", "), "commands registered");
                Ok(Data {
                    db: pool,
                    config,
                    anilist,
                    auto: Default::default(),
                    http: reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(15))
                        .user_agent(concat!("anpot-discord-bot/", env!("CARGO_PKG_VERSION")))
                        .build()?,
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
    let mut cmds = vec![
        commands::ping(),
        commands::media::anime(),
        commands::media::manga(),
        commands::character::character(),
        commands::airing::airing(),
        commands::seasonal::seasonal(),
        commands::random::random(),
        commands::profile::profile(),
        commands::profile::top(),
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
        for want in [
            "ping",
            "anime",
            "manga",
            "character",
            "character search",
            "airing",
            "seasonal",
            "random",
            "random anime",
            "random manga",
            "random character",
            "profile",
            "profile create",
            "profile add",
            "profile bulk-add",
            "profile remove",
            "profile clear",
            "profile reorder",
            "profile move",
            "profile rank",
            "profile group",
            "profile group create",
            "profile group rename",
            "profile group delete",
            "profile group assign",
            "profile note",
            "profile set-bio",
            "profile set-color",
            "profile set-banner",
            "profile view",
            "profile style",
            "profile privacy",
            "profile export",
            "profile import",
            "profile compare",
            "top",
            "top characters",
        ] {
            assert!(
                names.iter().any(|n| n == want),
                "{want} is not registered: {names:?}"
            );
        }
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
