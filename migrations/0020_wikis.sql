-- Round 11 (additive): global MediaWiki sources (Fandom, wiki.gg…) and their cache. Nothing existing is touched.
-- A wiki is defined once by the bot owner and applies to every server; there is no per-server row.
CREATE TABLE IF NOT EXISTS wiki_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    host TEXT NOT NULL UNIQUE,
    api_url TEXT NOT NULL,
    lang TEXT NOT NULL DEFAULT 'en',
    license_name TEXT,
    license_url TEXT,
    character_category TEXT,
    game_category TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    added_by INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_checked_at INTEGER,
    last_error TEXT
);

-- Pages fetched from the wikis. Rows are a cache: they expire by age and removing a wiki leaves them to expire.
CREATE TABLE IF NOT EXISTS wiki_cache (
    wiki_id INTEGER NOT NULL,
    page_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    normalized_key TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'other',
    extract TEXT,
    image_url TEXT,
    aliases TEXT NOT NULL DEFAULT '[]',
    parent_title TEXT,
    categories TEXT NOT NULL DEFAULT '[]',
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (wiki_id, page_id)
);
CREATE INDEX IF NOT EXISTS idx_wiki_cache_key ON wiki_cache (normalized_key);
CREATE INDEX IF NOT EXISTS idx_wiki_cache_fetched ON wiki_cache (fetched_at);

-- Autocomplete reads only this index (never a wiki, because of Discord's three seconds).
CREATE VIRTUAL TABLE IF NOT EXISTS wiki_fts USING fts5(
    title, aliases, content = 'wiki_cache', content_rowid = 'rowid', tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER IF NOT EXISTS wiki_cache_ai AFTER INSERT ON wiki_cache BEGIN
    INSERT INTO wiki_fts (rowid, title, aliases) VALUES (new.rowid, new.title, new.aliases);
END;
CREATE TRIGGER IF NOT EXISTS wiki_cache_ad AFTER DELETE ON wiki_cache BEGIN
    INSERT INTO wiki_fts (wiki_fts, rowid, title, aliases) VALUES ('delete', old.rowid, old.title, old.aliases);
END;
CREATE TRIGGER IF NOT EXISTS wiki_cache_au AFTER UPDATE ON wiki_cache BEGIN
    INSERT INTO wiki_fts (wiki_fts, rowid, title, aliases) VALUES ('delete', old.rowid, old.title, old.aliases);
    INSERT INTO wiki_fts (rowid, title, aliases) VALUES (new.rowid, new.title, new.aliases);
END;
