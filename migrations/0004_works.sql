-- Favourite works (anime, manga, manhwa, manhua, novels) next to favourite characters.
-- Work ids are provider-prefixed text ("anilist:123", later "mu:456", "mdex:<uuid>").
CREATE TABLE media_cache (
    work_id TEXT PRIMARY KEY,
    title_romaji TEXT NOT NULL,
    title_english TEXT,
    cover_url TEXT,
    kind TEXT NOT NULL,
    format TEXT,
    country TEXT,
    year INTEGER,
    score INTEGER,
    status TEXT,
    is_safe INTEGER NOT NULL,
    source TEXT NOT NULL DEFAULT 'anilist',
    fetched_at INTEGER NOT NULL
);

CREATE TABLE profile_works (
    user_id INTEGER NOT NULL REFERENCES profiles(user_id) ON DELETE CASCADE,
    work_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    position INTEGER NOT NULL,
    group_id INTEGER REFERENCES profile_groups(id) ON DELETE SET NULL,
    tier TEXT,
    note TEXT,
    -- NULL, 'watched' or 'planning'
    list TEXT,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, work_id)
);
CREATE INDEX idx_profile_works_pos ON profile_works (user_id, position);
CREATE INDEX idx_profile_works_work ON profile_works (work_id);
