-- Round 5: member suggestions, episode reminders, and removal of the per-work notification lists.
CREATE TABLE suggestions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    author_id INTEGER NOT NULL,
    first_id TEXT NOT NULL,
    second_id TEXT NOT NULL,
    first_title TEXT NOT NULL,
    second_title TEXT NOT NULL,
    first_url TEXT,
    second_url TEXT,
    -- both ids sorted and joined, so (a, b) and (b, a) are the same pair
    pair_key TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE UNIQUE INDEX idx_suggestions_pair ON suggestions (guild_id, pair_key);
CREATE INDEX idx_suggestions_author ON suggestions (guild_id, author_id, created_at);

CREATE TABLE reminders (
    user_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    -- the newest episode already announced to this user (dedupe)
    last_episode INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, work_id)
);
CREATE INDEX idx_reminders_work ON reminders (work_id);

ALTER TABLE guild_settings ADD COLUMN suggest_daily_limit INTEGER NOT NULL DEFAULT 10;
-- may a reminder fall back to a ping in the notifications channel when the member's DMs are closed?
ALTER TABLE guild_settings ADD COLUMN reminder_ping INTEGER NOT NULL DEFAULT 0;

-- The per-work notification list and member requests were removed in round 4.
DROP TABLE IF EXISTS alert_list;
DROP TABLE IF EXISTS alert_requests;
