-- Round 10 (additive): the sessions of interactive messages, so that their buttons can be disabled on time even after a
-- restart. `state_json` is the navigation stack (descriptors only), `components_json` the components as last rendered
-- (what the sweeper greys out). Interaction tokens are never stored.
CREATE TABLE IF NOT EXISTS message_sessions (
    message_id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    guild_id INTEGER,
    owner_id INTEGER,
    kind TEXT NOT NULL DEFAULT 'view',
    timeout_class TEXT NOT NULL DEFAULT 'default',
    started_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    hard_expires_at INTEGER NOT NULL,
    state_json TEXT NOT NULL DEFAULT '[]',
    components_json TEXT NOT NULL DEFAULT '[]',
    v2 INTEGER NOT NULL DEFAULT 0,
    ephemeral INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_message_sessions_expiry ON message_sessions (expires_at);
