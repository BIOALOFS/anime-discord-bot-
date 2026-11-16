-- Weekly points of the collective games, per server (week = ISO year and week, e.g. 2026-W46).
CREATE TABLE weekly_scores (
    guild_id INTEGER NOT NULL,
    week TEXT NOT NULL,
    user_id INTEGER NOT NULL,
    points INTEGER NOT NULL DEFAULT 0,
    correct INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, week, user_id)
);
