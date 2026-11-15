-- Airing feed, notification list and episode dedupe.
CREATE TABLE guild_feed (
    guild_id INTEGER PRIMARY KEY,
    channel_id INTEGER,
    enabled INTEGER NOT NULL DEFAULT 0,
    min_popularity INTEGER NOT NULL DEFAULT 1500,
    formats TEXT NOT NULL DEFAULT 'TV,TV_SHORT,ONA,MOVIE',
    excluded_genres TEXT NOT NULL DEFAULT '',
    digest_hours INTEGER NOT NULL DEFAULT 0,
    daily_cap INTEGER NOT NULL DEFAULT 0,
    role_id INTEGER,
    -- 'all' (every anime plus pins) or 'listed' (only the notification list)
    mode TEXT NOT NULL DEFAULT 'all',
    last_digest_at INTEGER NOT NULL DEFAULT 0
);

-- mode: 'listed' (post), 'pinned' (post even below the popularity threshold), 'muted' (never post)
CREATE TABLE alert_list (
    guild_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL DEFAULT 'listed',
    PRIMARY KEY (guild_id, work_id)
);

-- Member requests from the card's bell button, awaiting admin approval.
CREATE TABLE alert_requests (
    guild_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    user_id INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (guild_id, work_id, user_id)
);

CREATE TABLE posted_episodes (
    guild_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    episode INTEGER NOT NULL,
    posted_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, work_id, episode)
);
CREATE INDEX idx_posted_episodes_time ON posted_episodes (guild_id, posted_at);

-- Episodes waiting for the next digest.
CREATE TABLE digest_queue (
    guild_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    episode INTEGER NOT NULL,
    title TEXT NOT NULL,
    airing_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, work_id, episode)
);

CREATE TABLE feed_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_checked INTEGER NOT NULL DEFAULT 0
);
INSERT INTO feed_state (id, last_checked) VALUES (1, 0);

ALTER TABLE guild_settings ADD COLUMN birthday_channel_enabled INTEGER NOT NULL DEFAULT 0;
