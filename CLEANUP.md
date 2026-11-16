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
