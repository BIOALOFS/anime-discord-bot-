-- Servers can hide mature-rated games (ESRB M, PEGI 18, CERO Z); on by default, only sexual content is always hidden.
ALTER TABLE guild_settings ADD COLUMN mature_games INTEGER NOT NULL DEFAULT 1;
