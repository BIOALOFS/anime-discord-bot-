# Cleanup log (round 5 audit)

What was removed and what was merged. Nothing here is reachable from a command any more.

## Removed commands, UI and code
- **Profile display styles** (collage, podium, tier list, names, compact, gallery, random) and the style dropdown. The profile is a
  single message of three embeds with three component rows. Image engine pieces for them (grid/podium/tier renderers, themes,
  header/cell specs, output cache) are deleted; `render` now only draws banners and the guessing-game crops.
- **Profile management commands**: groups, tiers, notes, lists, reordering, bulk add, clear, compare, share, templates, quote,
  per-character report buttons. Editing lives in `/edit` only; `/profile` only views and adds.
- **Per-work notification lists** (add / pin / mute / member requests), the bell button and their strings.
- **Digest mode, the daily cap, excluded genres and the daily birthday post** of the airing feed (nothing could set them any more).
  `/random type:birthdays` stays.
- **Old admin commands**: `/setup`, the old `/settings section/option/value` panel, extra allowed channels and the admin-bypass
  switch (admins now always bypass the bot-channel lock).
- Dead helpers found by the compiler and by hand: unused DB functions, strings, test helpers and the tests that only covered them.

## Removed database objects (migrations 0011 and 0012, applied after the automatic backup)
- Tables: `alert_list`, `alert_requests`, `digest_queue`, `guild_extra_channels`.
- Columns: `profiles.template`, `profiles.default_style`, `guild_feed.{excluded_genres, digest_hours, daily_cap, last_digest_at, mode}`,
  `guild_settings.{admin_bypass, birthday_channel, birthday_channel_enabled}`, `feed_state.last_birthday_day`.
- Kept on purpose (user data, still in exports): profile groups, tiers, notes and lists. Existing image bans are still honoured.

## Merged / shared
- One character view (`commands/character.rs`) now serves `/character`, the cast dropdown of work cards, `/top`, `/random`, rosters,
  versus and the guess-the-character result, so the action buttons (add/remove, works, link, back) exist once.
- One upload path (`take_upload` / `upload_gate`) for character pictures and banners (PNG/JPG/WebP/GIF, 3 MB, re-encoded, same
  per-server rules); one select-option builder (`ui::select::normalize_choices`) for 25-option / 100-character / unique-value limits.
- One `Providers::work_by_key` for provider-prefixed ids, used by `/suggest`.
- `guild::can_manage` is the single permission rule for `/admin` and the channel commands.

## New in this round
`/suggest`, reminders, ready-made banners, GIF banners, the three-embed profile, character action buttons everywhere.

## Round 6
- **Profile**: the three-embed layout is replaced by one Components V2 container (embed fallback of at most two embeds kept in
  the same view). `info_embed`/`item_embed` and the author-icon helper are gone; `ui::styles::item_card` and `Style` leftovers deleted.
- **Games**: the owner-only `guess-character` / `guess-anime` quizzes and the per-user channel guard are removed, superseded by the
  collective games in `commands/arena.rs` (one shared session per channel).
- **Recommend**: the dropdown only "selected" an item for the add/hide buttons; it now opens the work card.
- New: `ui::v2` (Components V2 builders), shared sessions and timers in the session engine, `show work` global handler, chapter sources
  (`chapters/`), weekly leaderboard, suggestion pictures.

## Round 7 (audit)
- **Removed commands (round 5; `/search` came back in round 9 as the general search):** `/search`, `/character` and `/random` (their jobs live in the seven category commands `/anime /manga /novel /drama /series /movie /game`);
  `src/commands/search.rs` and `src/commands/random.rs` are deleted. The old minigame `/game` became `/event` (`commands/game.rs` → `commands/event.rs`).
  Their autocompletes (`ac_source`, `ac_work`, the `source` choices), the daily-spotlight and birthday pickers in the command surface, and every mention
  in `/help`, README and the strings are gone. `/top` lost its `kind/scope/type` options and the global scope (it is the server's ranking now).
- **Removed code:** the cross-server top queries (`top_characters`, `top_works` and their counters), `media_cache_get`, `cache_get` (kept only inside
  its test), the `profiles.created_at` field and the join date (the column stays in the table), `run_media` (now `run_media_with`), the chapter
  announcement `components()` builder (replaced by digests), the old `shownotif:` button builders (old messages in channels still work through
  `parse_show_work`), and `ItemKind::Neither`.
- **Merged:** one `components` module builds every universal button ("أظهر العمل" / "أظهر الشخصيات" / ذكّرني / أضف لبروفايلي / status menu / item menus)
  and one global handler (`global.rs`) answers them; one `texts` module (alias `i18n`) holds every string; one `ui::rating` formatter shows rating and
  popularity for every source; one `CategorySearch` handler (`commands/category.rs`) serves the seven commands.
- **Guards that keep it clean:** a test fails when a `#[poise::command]` function exists but is not registered (and the reverse); another fails when a
  `View` is not declared in the registry or lacks the central builders; one fails when `/help` does not explain exactly the registered commands; one
  fails when a string loses its Arabic or goes back to the stiff wording. Startup overwrites the whole command list (global, or the dev guild) in
  one call, so a removed command disappears from Discord at the next start.
- **Migration 0016 (additive only):** `item_status`, `notification_settings`, `notification_prefs`, `notification_exclusions`, `drama_country_overrides`,
  `notified_events`, new columns on `reminders` (`work_key`, `category`, `notify_days`), `guild_settings` (`timezone`, `spoiler_guard`, `tmdb_region`)
  and `chapter_settings` (`digest_hours`, `digest_at`). Nothing is dropped or rebuilt, and the automatic backup runs first.

## Round 8
- **Removed:** the old computed profile badges (`ui/styles.rs`, `BADGE_*` strings, replaced by stored badges), `no_popularity` (character cards omit the line), the
  old per-call provider failure flags (`games_unavailable`), the separate game-search helpers, and every use of the invalid VNDB field `releases.minage`.
- **Guards:** a test checks every field of every VNDB request against the documented field lists; tests check the exact IGDB and VNDB bodies; another covers
  partial provider results and the random-pick offsets. `.env.example` is now the tracked `.env` template (as on `main`); `OWNER_ID` was added to it.
- **Migration 0017:** `mature_games` renamed to `mature_themes`. **Migration 0018 (additive):** `bot_state`, `guild_features`, `daily_posts`, `polls`, `poll_votes`,
  `badges`, `role_permissions`, `watch_nights`, `watch_night_rsvps`, `usage_stats`.

## Round 9
- **Removed (banners):** the 2:1–4:1 ratio rule, the 600 px minimum, the 1200×400 recommendation, the centre crop (`imaging::crop_banner`, `banner_ratio_ok`,
  `BannerError`), the preview / confirm step (`banner_picker::Confirm`, `confirm`), the `BANNER_TOO_NARROW`, `BANNER_BAD_RATIO` and `BANNER_CONFIRM_*` /
  `BANNER_CANCELLED` strings, their tests, and every mention in `/help` and the README. A banner is accepted as it is (8 MB, pixel guard, re-encoded when still).
- **Added:** `src/people/` (people parsers per source, fetch, messages), `src/search.rs` and `commands/search.rs` (the general search), `anilist/safe/people.rs`
  (staff requests), cached raw documents for VNDB staff and IGDB companies in `Providers`, the `AUTOCOMPLETE` task-local budget of the AniList client.
- **Replaced:** the autocomplete helper (`ui/autocomplete.rs`): one deadline of 2.3 s, a background lookup that warms a cache, prefix fallback, one sanitizer.
- **Data safety:** a migration runs only after a verified backup; row counts of every user-data table are compared afterwards and the backup is restored
  automatically if any count drops; a test rejects `DROP` / `DELETE` / `TRUNCATE` in migrations 0013 and later. No migration was added in this round.
- **Guards:** a navigation-matrix test for the profile (every control after a picture edit, missing / empty / oversized pictures), tests for the shared search
  cache between autocomplete and submit, the person / work / character button rules and the overflow menu.

## Round 10
- **Removed:** the 180-second collector timeout (`session::TIMEOUT`), the "send a NEW message" navigation of the universal buttons (`global::handle`, `followup`),
  `card_is_ephemeral`, and `image_of_character` outside tests (pictures are read by id through the cache).
- **Added:** `src/nav/` (lifetimes, the Back trail, the session store, the sweeper, the live-message registry, edit-in-place overlays, private views),
  `commands/profile/imgcache.rs` (image LRU), `people/profile.rs` (people as characters), attachment reuse for Components V2 edits.
- **Migration 0019 (additive):** `message_sessions`. No existing table or column was touched; people use ids above 10^15 in the existing character tables, so no
  column was added.
- **Not changed on purpose:** the five-minute cache of the `/top` rankings (it is data, not button state).

## Round 11
- **Added:** `src/wiki/` (parsers, client over a swappable transport, store, service, safety filter, fill, cards, the owner panel), migration 0020 (additive:
  `wiki_sources`, `wiki_cache`, `wiki_fts`), the `/game` `source` menu (IGDB, VNDB, all wikis, each wiki) replacing the fixed two-value choice (`GameSource`).
- **Removed:** the `GameSource` choice enum. No Giant Bomb code, variable or text exists anywhere.
- **Not done on purpose:** wiki pages cannot be added to a profile (they have no stable integer id), and the cards of the interactive game / character views
  (`FallbackView`, `CharacterView`) are not filled in place, only the cards that the navigation buttons and `/search` open.


## Round 11.1
- **Added:** `src/wiki/clean.rs` (jargon list, series matching), `wiki/profile.rs`, `wiki/resolve.rs`, `wiki/fill.rs` live fill, migration 0021 (additive
  `wiki_snapshots`), the `source` option on every search command, the add/remove toggle on every card.
- **Removed:** the "wiki pages cannot be added to a profile" limit and the "fallback only" wiki results; the `ac_name` source-less path.
- **Still true:** the notification digests do not list wiki items separately; a wiki work is not merged with a same-title work of another source (they are only linked for button navigation).

## Round 11.2 and 11.3
- **Added:** `src/netfetch.rs`, `anilist/index.rs`, `Kind` mediums, migrations 0022 (wiki `work_category`, `kind_hint`) and 0023 (`media_index`, a cache excluded from the row-count check).
- **Removed:** nothing was deleted: the audit found no unused dependency, no Giant Bomb code, no dead module. Kept as uncertain: the `#[allow(dead_code)]` items (`session.rs`, `main.rs`, `db/profile`, `db/works`, `commands/profile/items.rs`), and `.env`, which is tracked as the variables template (not read; make sure it holds no secret).
- **Not done:** restoring a navigable profile session after a restart (sessions after a restart are only disabled by the sweeper); the "من رابط" choice is a `/edit` action with the link in "النص", not a modal; `kind_hint` has no owner-panel field yet (database column only).
