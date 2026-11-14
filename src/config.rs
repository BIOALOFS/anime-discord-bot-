use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub discord_token: String,
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
    "sqlite://data/anpot.db".into()
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
        let _ = dotenvy::dotenv();
        Ok(envy::from_env()?)
    }
}

#[cfg(test)]
impl Config {
    pub fn for_tests() -> Self {
        Self {
            discord_token: "test".into(),
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
