-- Round 7: watch status, per-category notifications, exclusions, drama/series overrides, TMDB reminders.
-- Everything is additive: no existing table is rebuilt or dropped.

-- "I'm watching it now / completed / later / paused / dropped" for any work of any source.
CREATE TABLE IF NOT EXISTS item_status (
    user_id    INTEGER NOT NULL,
    work_key   TEXT    NOT NULL,
    status     TEXT    NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, work_key)
);

-- Reminders keep their integer work_id (AniList ids as before, TMDB ids encoded above 3e12); the key and the
-- category make them readable for every source, and notify_days is the "قريبًا" lead time.
ALTER TABLE reminders ADD COLUMN work_key TEXT NOT NULL DEFAULT '';
ALTER TABLE reminders ADD COLUMN category TEXT NOT NULL DEFAULT 'anime';
ALTER TABLE reminders ADD COLUMN notify_days INTEGER NOT NULL DEFAULT 1;
UPDATE reminders SET work_key = 'anilist:' || work_id WHERE work_key = '';

-- Per member and category switches and delivery preferences ('*' is the member's default row).
CREATE TABLE IF NOT EXISTS notification_settings (
    user_id      INTEGER NOT NULL,
    category     TEXT    NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    -- 'all' | 'mine' (profile and reminders only) | 'popular'
    mode         TEXT    NOT NULL DEFAULT 'mine',
    -- chapter digests: hours between digests (1, 2, 4, 8 or 24)
    digest_hours INTEGER NOT NULL DEFAULT 2,
    PRIMARY KEY (user_id, category)
);

-- Per member: timezone and the hour of the daily/weekly summaries.
CREATE TABLE IF NOT EXISTS notification_prefs (
    user_id     INTEGER PRIMARY KEY,
    timezone    TEXT    NOT NULL DEFAULT 'Asia/Muscat',
    summary_hour INTEGER NOT NULL DEFAULT 9,
    spoiler_guard INTEGER NOT NULL DEFAULT 0
);

-- 'item' (a work key), 'category', 'format' or 'genre' the member does not want notifications about.
CREATE TABLE IF NOT EXISTS notification_exclusions (
    user_id INTEGER NOT NULL,
    kind    TEXT    NOT NULL,
    value   TEXT    NOT NULL,
    PRIMARY KEY (user_id, kind, value)
);

-- Servers may move a country between /drama and /series.
CREATE TABLE IF NOT EXISTS drama_country_overrides (
    guild_id INTEGER NOT NULL,
    country  TEXT    NOT NULL,
    target   TEXT    NOT NULL,
    PRIMARY KEY (guild_id, country)
);

-- What was already announced to whom (episodes, releases, "coming soon" notices), so nothing is sent twice.
CREATE TABLE IF NOT EXISTS notified_events (
    user_id  INTEGER NOT NULL,
    work_key TEXT    NOT NULL,
    event    TEXT    NOT NULL,
    sent_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, work_key, event)
);

-- Server-level settings added this round: timezone for schedules, spoiler guard, the region for "where to watch".
ALTER TABLE guild_settings ADD COLUMN timezone TEXT NOT NULL DEFAULT 'Asia/Muscat';
ALTER TABLE guild_settings ADD COLUMN spoiler_guard INTEGER NOT NULL DEFAULT 0;
ALTER TABLE guild_settings ADD COLUMN tmdb_region TEXT;

-- Chapter digests: how often a server gets one message with its new chapters, and when the last one was sent.
ALTER TABLE chapter_settings ADD COLUMN digest_hours INTEGER NOT NULL DEFAULT 2;
ALTER TABLE chapter_settings ADD COLUMN digest_at INTEGER NOT NULL DEFAULT 0;
