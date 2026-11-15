-- Lightweight metadata so profiles render even when AniList is slow or down.
ALTER TABLE character_cache ADD COLUMN main_work TEXT;
ALTER TABLE character_cache ADD COLUMN main_work_url TEXT;
ALTER TABLE character_cache ADD COLUMN favourites INTEGER;

-- Which users used the bot in which guild (no privileged intents needed for server counters).
CREATE TABLE user_guilds (
    guild_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_id)
);
CREATE INDEX idx_user_guilds_user ON user_guilds (user_id);

CREATE TABLE guess_scores (
    user_id INTEGER PRIMARY KEY,
    correct INTEGER NOT NULL DEFAULT 0,
    played INTEGER NOT NULL DEFAULT 0,
    best_streak INTEGER NOT NULL DEFAULT 0,
    streak INTEGER NOT NULL DEFAULT 0
);

ALTER TABLE guild_settings ADD COLUMN birthday_channel INTEGER;
