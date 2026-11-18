-- Round 8 (additive only): owner tools, daily posts, polls, badges, role permissions, watch nights, usage stats.

-- Small key/value state of the bot itself (maintenance mode).
CREATE TABLE IF NOT EXISTS bot_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Per server feature switches. Features that post messages are OFF until an admin turns them on.
CREATE TABLE IF NOT EXISTS guild_features (
    guild_id INTEGER NOT NULL,
    feature  TEXT    NOT NULL,
    enabled  INTEGER NOT NULL DEFAULT 0,
    hour     INTEGER NOT NULL DEFAULT 9,
    -- feature specific small settings (poll day / duration, ...)
    extra    TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (guild_id, feature)
);

-- What was posted for a server on a day (birthdays, featured picks): dedupe, and "not repeated for 30 days".
CREATE TABLE IF NOT EXISTS daily_posts (
    guild_id INTEGER NOT NULL,
    kind     TEXT    NOT NULL,
    item_key TEXT    NOT NULL,
    day      TEXT    NOT NULL,
    PRIMARY KEY (guild_id, kind, item_key, day)
);
CREATE INDEX IF NOT EXISTS idx_daily_posts_kind ON daily_posts (guild_id, kind, day);

CREATE TABLE IF NOT EXISTS polls (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id   INTEGER NOT NULL,
    channel_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL DEFAULT 0,
    -- JSON list of the options: [{"key": "...", "label": "..."}]
    options    TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    closes_at  INTEGER NOT NULL,
    closed     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_polls_open ON polls (closed, closes_at);

CREATE TABLE IF NOT EXISTS poll_votes (
    poll_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    option  INTEGER NOT NULL,
    PRIMARY KEY (poll_id, user_id)
);

CREATE TABLE IF NOT EXISTS badges (
    user_id    INTEGER NOT NULL,
    badge_key  TEXT    NOT NULL,
    earned_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, badge_key)
);

-- Which roles may do an action; no rows for an action means everyone may.
CREATE TABLE IF NOT EXISTS role_permissions (
    guild_id INTEGER NOT NULL,
    action   TEXT    NOT NULL,
    role_id  INTEGER NOT NULL,
    PRIMARY KEY (guild_id, action, role_id)
);

CREATE TABLE IF NOT EXISTS watch_nights (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id    INTEGER NOT NULL,
    channel_id  INTEGER NOT NULL,
    message_id  INTEGER NOT NULL DEFAULT 0,
    work_key    TEXT    NOT NULL,
    title       TEXT    NOT NULL,
    description TEXT    NOT NULL DEFAULT '',
    starts_at   INTEGER NOT NULL,
    created_by  INTEGER NOT NULL,
    reminded    INTEGER NOT NULL DEFAULT 0,
    event_id    TEXT
);
CREATE INDEX IF NOT EXISTS idx_watch_nights_due ON watch_nights (reminded, starts_at);

CREATE TABLE IF NOT EXISTS watch_night_rsvps (
    night_id INTEGER NOT NULL,
    user_id  INTEGER NOT NULL,
    going    INTEGER NOT NULL,
    PRIMARY KEY (night_id, user_id)
);

-- Daily aggregated counters: no message content and no personal data. kind: 'command', 'error', 'cache', 'api'.
CREATE TABLE IF NOT EXISTS usage_stats (
    day   TEXT    NOT NULL,
    kind  TEXT    NOT NULL,
    name  TEXT    NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, kind, name)
);
