use serde::Deserialize;

/// A secret that never shows up in logs or `Debug` output.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub discord_token: Secret,
    #[serde(default = "d_db")]
    pub database_url: String,
    #[serde(default = "d_log")]
    pub log_level: String,
    #[serde(default = "d_rate")]
    pub anilist_rate_limit_per_min: u32,
    #[serde(default = "d_search")]
    pub cache_ttl_search_secs: u64,
    #[serde(default = "d_details")]
    pub cache_ttl_details_secs: u64,
    #[serde(default = "d_profile")]
    pub cache_ttl_profile_secs: u64,
    #[serde(default = "d_airing")]
    pub cache_ttl_airing_secs: u64,
    #[serde(default = "d_neg")]
    pub cache_ttl_negative_secs: u64,
    #[serde(default = "d_cooldown")]
    pub command_cooldown_secs: u64,
    #[serde(default, deserialize_with = "opt_u64")]
    pub dev_guild_id: Option<u64>,
}

fn d_db() -> String {
    "sqlite://bot.db?mode=rwc".into()
}
fn d_log() -> String {
    "info".into()
}
fn d_rate() -> u32 {
    80
}
fn d_search() -> u64 {
    600
}
fn d_details() -> u64 {
    21_600
}
fn d_profile() -> u64 {
    900
}
fn d_airing() -> u64 {
    300
}
fn d_neg() -> u64 {
    300
}
fn d_cooldown() -> u64 {
    3
}

/// `DEV_GUILD_ID=` (empty) in .env.example must mean "unset".
fn opt_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let s: Option<String> = Option::deserialize(d)?;
    match s.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => v.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // Only the working directory's .env; absent is fine.
        let _ = dotenvy::from_filename(".env");
        Ok(envy::from_env()?)
    }
}

#[cfg(test)]
impl Config {
    pub fn for_tests() -> Self {
        Self {
            discord_token: Secret("test".into()),
            database_url: "sqlite::memory:".into(),
            log_level: "info".into(),
            anilist_rate_limit_per_min: 6000,
            cache_ttl_search_secs: 60,
            cache_ttl_details_secs: 60,
            cache_ttl_profile_secs: 60,
            cache_ttl_airing_secs: 60,
            cache_ttl_negative_secs: 60,
            command_cooldown_secs: 0,
            dev_guild_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_never_printed() {
        let cfg = Config::for_tests();
        let shown = format!("{cfg:?}");
        assert!(!shown.contains("test\"") && shown.contains("<redacted>"));
        assert_eq!(cfg.discord_token.expose(), "test");
    }
}
