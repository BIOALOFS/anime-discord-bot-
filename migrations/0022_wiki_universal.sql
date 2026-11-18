-- Round 11.2 (additive): wikis are not only about games. `work_category` is the generic category of works (games, anime,
-- manga, manhwa, novels, series, films...). `game_category` stays for backward compatibility and is filled from
-- `work_category` as well. `kind_hint` is an optional manual override of the medium of a wiki's works
-- (game, anime, manga, novel, series, movie). Nothing is dropped or deleted.
ALTER TABLE wiki_sources ADD COLUMN work_category TEXT;
ALTER TABLE wiki_sources ADD COLUMN kind_hint TEXT;
UPDATE wiki_sources SET work_category = game_category WHERE work_category IS NULL AND game_category IS NOT NULL;
