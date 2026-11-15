-- Per-server game scores, versus votes and the daily spotlight.
CREATE TABLE game_scores (
    guild_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    correct INTEGER NOT NULL DEFAULT 0,
    played INTEGER NOT NULL DEFAULT 0,
    streak INTEGER NOT NULL DEFAULT 0,
    best_streak INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (guild_id, user_id)
);

CREATE TABLE versus_votes (
    character_id INTEGER PRIMARY KEY,
    wins INTEGER NOT NULL DEFAULT 0,
    shown INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE hidden_recommendations (
    user_id INTEGER NOT NULL,
    work_id INTEGER NOT NULL,
    PRIMARY KEY (user_id, work_id)
);
