-- Round 5 audit: columns and tables nothing reads any more.
-- The profile has a single display mode (cards) and no templates.
ALTER TABLE profiles DROP COLUMN template;
ALTER TABLE profiles DROP COLUMN default_style;
-- The bot channel is the only channel lock (no extra channels, admins always bypass).
DROP TABLE IF EXISTS guild_extra_channels;
ALTER TABLE guild_settings DROP COLUMN admin_bypass;

-- Digest mode, the daily cap, excluded genres and the daily birthday post were removed with the old /settings.
DROP TABLE IF EXISTS digest_queue;
ALTER TABLE guild_feed DROP COLUMN excluded_genres;
ALTER TABLE guild_feed DROP COLUMN digest_hours;
ALTER TABLE guild_feed DROP COLUMN daily_cap;
ALTER TABLE guild_feed DROP COLUMN last_digest_at;
ALTER TABLE guild_feed DROP COLUMN mode;
ALTER TABLE guild_settings DROP COLUMN birthday_channel;
ALTER TABLE guild_settings DROP COLUMN birthday_channel_enabled;
ALTER TABLE feed_state DROP COLUMN last_birthday_day;
