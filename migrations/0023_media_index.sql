-- Round 11.3 (additive, a cache): a local index of the works AniList has answered with, for autocomplete that cannot
-- fail: prefix, acronym ("orv") and small-typo matching over every title and synonym. Rebuilt by use; safe to empty.
CREATE TABLE IF NOT EXISTS media_index (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    norm TEXT NOT NULL,
    acro TEXT NOT NULL,
    popularity INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS media_index_kind ON media_index (kind, popularity DESC);
