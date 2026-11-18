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
    /// MangaDex / MangaUpdates fallback for works AniList lacks. Off unless explicitly enabled.
    #[serde(default, deserialize_with = "opt_bool")]
    pub enable_fallback_providers: bool,
    /// IGDB (games). Both values are needed; either missing disables the provider.
    #[serde(default)]
    pub igdb_client_id: Option<String>,
    #[serde(default)]
    pub igdb_client_secret: Option<Secret>,
    /// VNDB (visual novels) needs no key; set `ENABLE_VNDB=false` to switch it off.
    #[serde(default = "d_true", deserialize_with = "opt_bool_true")]
    pub enable_vndb: bool,
    /// TMDB (movies, TV, dramas). Needs `TMDB_API_KEY` or `TMDB_READ_TOKEN`; without either it is switched off.
    #[serde(default)]
    pub tmdb_api_key: Option<Secret>,
    #[serde(default)]
    pub tmdb_read_token: Option<Secret>,
    #[serde(default = "d_region")]
    pub tmdb_region: String,
    #[serde(default = "d_true", deserialize_with = "opt_bool_true")]
    pub enable_tmdb: bool,
    #[serde(default = "d_tmdb_rate")]
    pub tmdb_rate_limit_per_10s: u32,
    #[serde(default, deserialize_with = "opt_u64")]
    pub dev_guild_id: Option<u64>,
}

fn d_region() -> String {
    "OM".into()
}
fn d_tmdb_rate() -> u32 {
    30
}
fn d_true() -> bool {
    true
}
fn d_db() -> String {
    "sqlite://bot.db?mode=rwc".into()
}
fn d_log() -> String {
    "info".into()
}
fn d_rate() -> u32 {
    // AniList is currently degraded to ~30/min (baseline 90): stay below it.
    25
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

fn opt_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let s: Option<String> = Option::deserialize(d)?;
    Ok(matches!(
        s.as_deref().map(|v| v.trim().to_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    ))
}

/// Default-on switch: only an explicit "false/0/no/off" turns it off.
fn opt_bool_true<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let s: Option<String> = Option::deserialize(d)?;
    Ok(!matches!(
        s.as_deref().map(|v| v.trim().to_lowercase()).as_deref(),
        Some("0" | "false" | "no" | "off")
    ))
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
    /// Providers switched on by this configuration (blank credentials count as missing).
    pub fn provider_config(&self) -> crate::providers::ProviderConfig {
        let id = self.igdb_client_id.as_deref().map(str::trim).unwrap_or("");
        let secret = self
            .igdb_client_secret
            .as_ref()
            .map(|s| s.expose().trim())
            .unwrap_or("");
        crate::providers::ProviderConfig {
            fallback: self.enable_fallback_providers,
            igdb: (!id.is_empty() && !secret.is_empty())
                .then(|| (id.to_string(), secret.to_string())),
            vndb: self.enable_vndb,
        }
    }

    /// TMDB settings: blank keys count as missing, `ENABLE_TMDB=false` switches it off.
    pub fn tmdb_config(&self) -> crate::tmdb::Config {
        let clean = |s: &Option<Secret>| {
            s.as_ref()
                .map(|s| s.expose().trim().to_string())
                .filter(|s| !s.is_empty())
        };
        let on = self.enable_tmdb;
        crate::tmdb::Config {
            api_key: clean(&self.tmdb_api_key).filter(|_| on),
            read_token: clean(&self.tmdb_read_token).filter(|_| on),
            region: self
                .tmdb_region
                .trim()
                .to_uppercase()
                .chars()
                .take(2)
                .collect(),
            rate_per_10s: self.tmdb_rate_limit_per_10s,
        }
    }

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
            enable_fallback_providers: false,
            igdb_client_id: None,
            igdb_client_secret: None,
            enable_vndb: false,
            tmdb_api_key: None,
            tmdb_read_token: None,
            tmdb_region: "OM".into(),
            enable_tmdb: false,
            tmdb_rate_limit_per_10s: 30,
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
