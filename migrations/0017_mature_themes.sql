-- The age-rating based `mature_games` switch becomes the content based `mature_themes` (on = horror and war
-- themes allowed, the default). Sexual content is hidden whatever the value; the stored values are kept.
ALTER TABLE guild_settings RENAME COLUMN mature_games TO mature_themes;
