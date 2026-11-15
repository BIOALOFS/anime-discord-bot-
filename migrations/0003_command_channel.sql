-- Command channel lock (/setup): the only channel where the bot answers, plus extras.
ALTER TABLE guild_settings ADD COLUMN command_channel_id INTEGER;
ALTER TABLE guild_settings ADD COLUMN admin_bypass INTEGER NOT NULL DEFAULT 1;

CREATE TABLE guild_extra_channels (
    guild_id INTEGER NOT NULL,
    channel_id INTEGER NOT NULL,
    PRIMARY KEY (guild_id, channel_id)
);
