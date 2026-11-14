CREATE TABLE users (
    discord_id INTEGER PRIMARY KEY,
    anilist_username TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE profiles (
    user_id INTEGER PRIMARY KEY,
    bio TEXT,
    accent_color INTEGER,
    quote TEXT,
    banner TEXT,
    default_style TEXT NOT NULL DEFAULT 'collage',
    privacy TEXT NOT NULL DEFAULT 'public' CHECK (privacy IN ('public', 'servers', 'private')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE profile_groups (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES profiles(user_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    UNIQUE (user_id, name)
);

CREATE TABLE profile_characters (
    user_id INTEGER NOT NULL REFERENCES profiles(user_id) ON DELETE CASCADE,
    character_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    group_id INTEGER REFERENCES profile_groups(id) ON DELETE SET NULL,
    tier TEXT,
    note TEXT,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, character_id)
);
CREATE INDEX idx_profile_characters_pos ON profile_characters (user_id, position);
CREATE INDEX idx_profile_characters_char ON profile_characters (character_id);

CREATE TABLE character_cache (
    character_id INTEGER PRIMARY KEY,
    name_romaji TEXT NOT NULL,
    name_native TEXT,
    image_url TEXT,
    is_safe INTEGER NOT NULL,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE guild_settings (
    guild_id INTEGER PRIMARY KEY,
    safe_mode INTEGER NOT NULL DEFAULT 1,
    nsfw_mode INTEGER NOT NULL DEFAULT 0,
    allowed_channels TEXT,
    language TEXT NOT NULL DEFAULT 'ar'
);
