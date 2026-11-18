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
