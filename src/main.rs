#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod anilist;
mod chapters;
mod commands;
mod components;
mod config;
mod db;
mod error;
mod events;
mod feed;
mod global;
mod guild;
pub mod texts;
/// The one module of user-facing text; `i18n` is kept as its short alias across the code.
pub use texts as i18n;
mod providers;
mod render;
mod tmdb;
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
    pub tmdb: std::sync::Arc<tmdb::Tmdb>,
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
    let provider_cfg = config.provider_config();
    let tmdb_cfg = config.tmdb_config();
    let (feed_pool, feed_api) = (pool.clone(), anilist.clone());
    let feed_web = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("anpot-discord-bot/", env!("CARGO_PKG_VERSION")))
        .build()?;

    // One TMDB client for commands and for the background reminders (shared cache and rate limit).
    let tmdb_shared = std::sync::Arc::new(tmdb::Tmdb::new(feed_web.clone(), tmdb_cfg.clone()));
    let feed_tmdb = tmdb_shared.clone();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands_with_cooldown(config.command_cooldown_secs),
            on_error: |e| Box::pin(error::on_error(e)),
            event_handler: |ctx, event, _framework, data| Box::pin(events::handle(ctx, event, data)),
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
                    providers: std::sync::Arc::new(providers::Providers::new(http.clone(), provider_cfg.clone())),
                    tmdb: tmdb_shared.clone(),
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
            chapters: std::sync::Arc::new(chapters::poll::Sources::new(feed_web)),
            last_chapter_poll: std::sync::atomic::AtomicI64::new(0),
            tmdb: feed_tmdb,
            last_tmdb_poll: std::sync::atomic::AtomicI64::new(0),
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
    // The seventeen user commands plus the three admin commands; nothing else is ever registered.
    let mut cmds = vec![
        commands::category::anime(),
        commands::category::manga(),
        commands::category::novel(),
        commands::category::drama(),
        commands::category::series(),
        commands::category::movie(),
        commands::category::game(),
        commands::event::event(),
        commands::top::top(),
        commands::schedule::schedule(),
        commands::profile::profile(),
        commands::profile::edit(),
        commands::suggest::suggest(),
        commands::recommend::recommend(),
        commands::airing::airing(),
        commands::settings::settings(),
        commands::help::help(),
        commands::admin::admin(),
        commands::channels::set_notifications_channel(),
        commands::channels::set_bot_channel(),
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
            "anime",
            "manga",
            "novel",
            "drama",
            "series",
            "movie",
            "game",
            "event",
            "top",
            "schedule",
            "profile",
            "edit",
            "suggest",
            "suggest add",
            "suggest random",
            "suggest mine",
            "recommend",
            "airing",
            "settings",
            "help",
            "admin",
            "set-notifications-channel",
            "set-bot-channel",
        ];
        assert_eq!(
            names, want,
            "the seventeen user commands plus /admin and the two channel commands, no aliases or leftovers, no /search /character /random"
        );
        // Must serialise for Discord without panicking.
        assert!(!poise::builtins::create_application_commands(&cmds).is_empty());
    }

    /// Names of the top-level registered commands.
    fn top_level_names() -> Vec<String> {
        commands_with_cooldown(3)
            .iter()
            .map(|c| c.name.to_string())
            .collect()
    }

    #[test]
    fn help_explains_exactly_the_registered_commands() {
        let cmds = commands_with_cooldown(3);
        let registered = top_level_names();
        let documented: Vec<&str> = commands::help::SECTIONS
            .iter()
            .flat_map(|s| s.commands.iter().copied())
            .collect();
        for name in &registered {
            assert!(
                documented.contains(&name.as_str()),
                "/{name} is registered but not in any /help section"
            );
        }
        for name in &documented {
            assert!(
                registered.iter().any(|r| r == name),
                "/help documents /{name}, which is not registered"
            );
        }
        // The command menu is built from the registry: one entry per command, each with an Arabic description.
        let infos = commands::help::cmd_infos(&cmds);
        assert_eq!(infos.len(), registered.len());
        assert!(infos
            .iter()
            .all(|i| !i.description.is_empty() && !commands::help::example(&i.name).is_empty()));
        assert!(
            infos.len() <= 25,
            "the command select menu holds 25 options"
        );
        // No removed command is mentioned anywhere in the help texts.
        let texts: String = commands::help::SECTIONS
            .iter()
            .map(|s| s.intro)
            .chain(commands::help::GUIDES.iter().map(|g| g.2))
            .collect();
        for gone in ["/search", "/character", "/random"] {
            assert!(!texts.contains(gone), "{gone} no longer exists");
        }
    }

    /// Every `#[poise::command]` function in the sources must be in the registered list, so a command can never be
    /// written, forgotten in `commands_with_cooldown` and silently never shipped (or shipped by an old registration).
    #[test]
    fn every_command_defined_in_code_is_registered_and_vice_versa() {
        fn defined(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for e in std::fs::read_dir(dir).expect("dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    defined(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).expect("read");
                    let prod = text.split("#[cfg(test)]").next().unwrap_or(&text);
                    let mut pending = false;
                    for line in prod.lines() {
                        let t = line.trim();
                        if t.starts_with("#[poise::command(") {
                            pending = true;
                        }
                        if pending {
                            if let Some(rest) = t
                                .strip_prefix("pub async fn ")
                                .or_else(|| t.strip_prefix("async fn "))
                            {
                                let name: String = rest
                                    .chars()
                                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                                    .collect();
                                out.push((name, p.display().to_string()));
                                pending = false;
                            }
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        defined(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands"),
            &mut found,
        );
        let mut registered = Vec::new();
        walk(&commands_with_cooldown(3), "", &mut registered);
        let registered: Vec<String> = registered
            .iter()
            .map(|n| n.rsplit(' ').next().unwrap_or(n).replace('-', "_"))
            .collect();
        assert!(
            found.len() >= 20,
            "the scan found only {} commands",
            found.len()
        );
        for (name, file) in &found {
            let n = name.trim_end_matches("_cmd").to_string();
            assert!(
                registered.contains(name) || registered.contains(&n),
                "{name} ({file}) is defined as a command but is not registered: delete it or add it to commands_with_cooldown"
            );
        }
        for r in &registered {
            assert!(
                found
                    .iter()
                    .any(|(n, _)| n == r || n.trim_end_matches("_cmd") == r),
                "{r} is registered but no #[poise::command] function with that name exists"
            );
        }
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
