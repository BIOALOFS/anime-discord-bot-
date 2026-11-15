-- Round 4: profile title/language, notification filter modes, admin panel settings.
ALTER TABLE profiles ADD COLUMN display_title TEXT;
ALTER TABLE profiles ADD COLUMN language TEXT NOT NULL DEFAULT 'ar';

-- 'all' | 'popular' | 'profiles' | 'formats'
ALTER TABLE guild_feed ADD COLUMN filter_mode TEXT NOT NULL DEFAULT 'all';
ALTER TABLE guild_feed ADD COLUMN kinds TEXT NOT NULL DEFAULT 'anime,manga,manhwa,manhua,novel';

-- who may upload custom images: 'everyone' | 'admins'
ALTER TABLE guild_settings ADD COLUMN upload_policy TEXT NOT NULL DEFAULT 'everyone';
-- members with this role may use the admin commands too
ALTER TABLE guild_settings ADD COLUMN manager_role_id INTEGER;
-- per-user command cooldown override in seconds (0 = the bot default)
ALTER TABLE guild_settings ADD COLUMN cooldown_secs INTEGER NOT NULL DEFAULT 0;
