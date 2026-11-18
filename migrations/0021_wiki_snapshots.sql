-- Round 11.1 (additive): the snapshot of a wiki page that a member added to a profile (name, picture, summary, series,
-- source, page address, license), so the item survives cache expiry and a wiki being disabled or deleted. Never
-- deleted by wiki changes.
CREATE TABLE IF NOT EXISTS wiki_snapshots (
    wiki_id INTEGER NOT NULL,
    page_id INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'other',
    name TEXT NOT NULL,
    image_url TEXT,
    summary TEXT,
    parent TEXT,
    source_name TEXT NOT NULL,
    page_url TEXT NOT NULL,
    license_name TEXT,
    license_url TEXT,
    saved_at INTEGER NOT NULL,
    PRIMARY KEY (wiki_id, page_id)
);
