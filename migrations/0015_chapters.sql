-- Chapter notifications for tracked manga / manhwa / manhua (works in a member's profile or reminders).
CREATE TABLE chapter_settings (
    guild_id INTEGER PRIMARY KEY,
    mangadex INTEGER NOT NULL DEFAULT 0,
    mangaupdates INTEGER NOT NULL DEFAULT 0,
    rss INTEGER NOT NULL DEFAULT 0,
    -- preferred MangaDex translation language: 'ar' or 'en'
    lang TEXT NOT NULL DEFAULT 'ar'
);

-- Feeds an admin added. A feed belongs to the tracked work whose title matches `title_match`.
CREATE TABLE rss_feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    url TEXT NOT NULL,
    title_match TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (guild_id, url)
);

-- AniList work -> the id the source uses for it (found once by title, then remembered).
CREATE TABLE chapter_map (
    work_id INTEGER NOT NULL,
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    PRIMARY KEY (work_id, source)
);

-- Dedupe: a chapter is announced once per server, whichever source reported it first.
CREATE TABLE chapter_posted (
    guild_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    chapter_key TEXT NOT NULL,
    posted_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, work_id, chapter_key)
);
