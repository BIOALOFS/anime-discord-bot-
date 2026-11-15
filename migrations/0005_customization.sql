-- Per-character personal customization, custom images and moderation.
ALTER TABLE profile_characters ADD COLUMN custom_title TEXT;
ALTER TABLE profile_characters ADD COLUMN custom_description TEXT;
ALTER TABLE profile_characters ADD COLUMN custom_image_id INTEGER;
ALTER TABLE profiles ADD COLUMN template TEXT NOT NULL DEFAULT 'classic';

CREATE TABLE custom_images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    hash TEXT NOT NULL,
    mime TEXT NOT NULL,
    bytes BLOB NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    animated INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (user_id, hash)
);

CREATE TABLE image_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    image_id INTEGER NOT NULL,
    owner_id INTEGER NOT NULL,
    reporter_id INTEGER NOT NULL,
    guild_id INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    resolved INTEGER NOT NULL DEFAULT 0,
    UNIQUE (image_id, reporter_id)
);

CREATE TABLE upload_bans (
    guild_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    banned_by INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (guild_id, user_id)
);

CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER,
    actor_id INTEGER NOT NULL,
    action TEXT NOT NULL,
    target TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

ALTER TABLE guild_settings ADD COLUMN custom_images INTEGER NOT NULL DEFAULT 0;
ALTER TABLE guild_settings ADD COLUMN games INTEGER NOT NULL DEFAULT 1;
