# anpot — Arabic Discord bot for anime, manga, manhwa, manhua and novels

A lightweight Rust Discord bot (poise + serenity, SQLite, AniList GraphQL). All user-facing text is Arabic;
code, logs and docs are English. Adult media and characters are filtered out everywhere by default.

## Commands

Seventeen user commands and three admin commands. The seven category commands share one shape, so learning one teaches all.

| Command | What it does |
| --- | --- |
| `/anime` `/manga` `/novel` `/drama` `/series` `/movie` `/game` | Same options everywhere: **type** (work, character, person — actors, voice actors, staff and companies —, random work, random character), **name** (autocomplete; optional for random picks) and the filters of the category: anime — format, year, status; manga — format (manga, manhwa, manhua, one-shot), status; novel — origin, status; drama — country, year, status; series — country, year, status, genre; movie — year, genre, country; game — platform, source (IGDB games or VNDB visual novels). Filters apply to searches and to random picks. A random pick avoids your last five and the **"عشوائي آخر"** button re-rolls in the same message. `/drama` lists Asian productions, `/series` the rest of the world (an admin can move a country from `/admin` → المسلسلات والجدول). |
| `/search` | **General search** across every category. `name` (autocomplete) shows a mixed, balanced list of at most 25: 🎬 works, 👤 characters, 🎙️ people, each labelled with its kind (for example "🎬 Frieren · عمل · أنمي · 2023"); the value is a provider-prefixed key, so picking one opens the right card directly. `category` (all, anime, manga, novels, drama, series, movies, games) and `type` (all, work, character, person) narrow it. A typed name searches all enabled sources in parallel through the same function and caches as the autocomplete, ranks exact names first, then popularity, and answers one message with a grouped list, a select menu (pages of 25) that opens each item as a new message, and an Arabic note when a source did not answer. |
| `/event` | **Collective games** with a `category` option (anime, manga, novel, drama, series, movie): any member of the channel can answer, the first correct answer wins the round's points (speed bonus), one game per channel, only the starter or an admin can skip or end. Modes: `character-image`, `work-cover`, `character-hints`, `true-false`, `higher-lower`, `order-years`, `race`, plus `versus` and `leaderboard`. The character modes need character pictures, so they are not offered for dramas, series and films. The answer's "أظهر العمل / أظهر الشخصيات" button appears only after the reveal. |
| `/top` | The **server's** rankings (never across servers): anime, manga, novels, drama, series, films, games, characters, members, event points; a time range (week, month, all time); a hero card for first place, places 2–10 as a list, pages, and a menu that opens any item in place (with Back). The rankings are cached for five minutes (that is data, not button state). |
| `/schedule` | This week's calendar: anime from AniList (exact times) and dramas/series from TMDB (the day), in your time zone (default `Asia/Muscat`, set per server in `/admin`, per member in `/settings`). Day and category selects, "أعمالي فقط", pages, and a menu that opens any work. |
| `/profile` | **One message, one visual block** (Components V2; embeds as a fallback): banner (PNG/JPG/WebP or animated GIF), a compact info line (`الشخصيات 4 · الأعمال 1 · عام`), the current item's picture and text, the counter `1/4`. It holds characters and works of **every** source: AniList, TMDB shows and films, TMDB cast entries (with the actor's photo, labelled), games, visual novels. A work shows your watch status as a text line. **`compare`** (a member): one compact message with the taste match in percent (Jaccard similarity per category, weighted by size), the number of shared items and menus that open each shared item as a new message; private profiles cannot be compared. **People in the profile:** actors, voice actors, directors, writers and other staff (TMDB persons, AniList staff, VNDB staff) can be added with the person card's "أضف لبروفايلي" and live in the same **الشخصيات** list as characters (there is no third mode; the toggle stays أعمال ⇄ شخصيات). They are shown with a "شخص حقيقي · ممثل" line, their popularity and up to three best known works, can be edited with `/edit character` like any character (nickname, description, picture or GIF, move, remove), and are counted in the characters count, `/top` and `/profile compare`. IGDB companies cannot be added. A short badges line (10/50/100 items, event wins, suggestions, first character, first custom banner) is earned once and announced privately. |
| `/edit` | Every modification. `target` = `profile` (banner, description, title, colour, privacy, reset, export/import/delete data), `character` or `work`. **Banner:** any shape and size is accepted as it is (PNG/JPG/WebP/GIF up to 8 MB; an animated GIF keeps animating, Discord scales it on display). Only technical limits remain: it must really decode as one of those types, stay under 8 MB and a sane pixel count, and still pictures are re-encoded to strip metadata. 16 ready-made banners at 1200×400. |
| `/suggest` | Member suggestions: `add` links two works with a reason ≤300 chars, `random` shows one from this server, `mine` lists yours. |
| `/recommend` | Recommendations with a `source` option: **من بروفايلي** (AniList recommendations of your anime/manga plus TMDB recommendations of your dramas, series and films; deduplicated, without what you already have) or **عامة** (popular, safe picks). The default is your profile when it holds at least three items. With `user`: what both of you would like. Results open as new messages with the universal buttons. |
| `/airing` | Next episode of an anime. |
| `/settings` | Personal preferences: language, privacy, reminders (max 50) and **notifications** (a private panel: a switch, a filter mode and a digest interval per category, time zone, the hour of the weekly summary, the spoiler guard and what you never want to hear about). |
| `/help` | One ephemeral message built from the real command list: a section menu, a command menu, three quick guides (add a character to my profile, turn on a reminder, customise my profile) and the credits page. |
| `/admin` | Admin panel (Manage Guild): Safety, Notifications, Custom images, Roles, Channels, Limits, Chapter sources (incl. digest interval), **المسلسلات والجدول** (schedule time zone, spoiler guard, moving countries between drama and series), **حالة المصادر** (enabled, last success, last sanitized error and rate budget of AniList, TMDB, IGDB, VNDB, MangaDex, MangaUpdates and each RSS feed, plus a re-check button), **منشورات يومية** (character birthdays for characters that live in a member's profile there, character of the day, work of the day — never repeated for 30 days; all off by default, one hour), **تصويت أسبوعي** («ماذا نشاهد؟» from the server's `/suggest` entries: day, hour, duration, one changeable vote per member, live counts, a result message that opens the winning pair), **صلاحيات الرتب** (which roles may upload images, add suggestions, start events, edit the profile; empty = everyone, Manage Server always), **ليلة مشاهدة** (a work, a date and time in the server's zone, an announcement with attend / not attend buttons, a reminder at the start, optionally a Discord scheduled event) and Reset. The application owner also sees **للمالك**: a backup now (sent privately when it fits), usage statistics and **maintenance mode**. |
| `/set-bot-channel` | Admin-only: the single channel where commands work (admins may bypass). |
| `/set-notifications-channel` | Admin-only: where new episode, chapter and weekly-summary messages are posted. |

**Every message about a character carries "أظهر العمل" and every message about a work carries "أظهر الشخصيات"** (a work card also has add-to-profile, a 🔔 reminder button and a watch-status select: watching or reading now, completed, later, paused, dropped). **Navigation edits the message in place** (Round 10): pressing one of these buttons, choosing an item in a menu, or paging a list replaces the view on the same message and adds a **"رجوع"** (Back) button that returns to the previous view (a stack of up to 10 views per message; the first view has no Back). That is for the member who ran the command; **any other member** who presses the same button gets a **private (ephemeral) view** of their own, with its own Back button and lifetime, and the public message does not change. **Notifications** are shared by the whole server, so pressing "أظهر العمل" / "أظهر الشخصيات" or choosing an item opens a private view for the presser and the notification is never edited. Reminder, add-to-profile and status presses act for the presser and answer privately. The profile is the one exception: it keeps its own controls. Removed commands: `/character` and `/random` (their jobs are in the category commands); `/search` is the general search over every category; the old minigame `/game` is now `/event`, and `/game` is the games category. **People:** a person card (image, name and native name, roles, birth date and age, popularity, a short biography, notable works) has "أظهر الأعمال" (a paged list; each choice opens the work or the character as a new message); a work card has "أظهر الطاقم" (cast, voice actors, staff, companies) and a character card has "أظهر المؤدي". Sources: AniList staff (anime, manga, novels), TMDB persons (drama, series, movies), IGDB companies and VNDB staff (games). People flagged adult, and people whose only works are blocked by the safety rules, are never shown. A crowded row first drops link buttons into the text, then moves the crew / performer buttons into a "المزيد" menu; "أظهر العمل", "أظهر الشخصيات" and add-to-profile always stay.

Both channel commands check that the bot can view, send, embed links and attach files there. Unsetting a channel is done in `/admin` → Channels.

### Credits and data sources

* **AniList** — anime, manga, novels and their characters.
* **TMDB** — dramas, series, films, cast and posters. *This product uses the TMDB API but is not endorsed or certified by TMDB.* The same statement (in Arabic and English) is in `/help` → عن البوت والمصادر, and TMDB cards carry the footer "المصدر: TMDB".
* **JustWatch** — where-to-watch data comes through TMDB; cards that show it say "Streaming data by JustWatch" (up to 6 providers for the server's region).
* **IGDB** (games, non-commercial) and **VNDB** (visual novels); **MangaDex** and **MangaUpdates** for chapters.

### TMDB setup

Create a free account at <https://www.themoviedb.org/settings/api>, request an API key and put it in `.env` as `TMDB_API_KEY` (or the "API Read Access Token" as `TMDB_READ_TOKEN`; the token wins if both are set). Without either, `/drama`, `/series`, `/movie` and the TMDB parts of `/schedule` and `/top` say the source is not enabled. The bot never asks TMDB for adult titles (`include_adult=false`, the adult flag, a keyword blacklist and NC-17 / explicit certifications are all checked, and a title without a reliable verdict is hidden). Requests are rate-limited (`TMDB_RATE_LIMIT_PER_10S`, default 30) and cached: searches 10 min, details 6 h, lists 1 h, misses 5 min.

### Notifications

Once `/set-notifications-channel` is set, every newly aired anime episode is announced (default filter **all**). The admin
panel offers: all, only popular (threshold), only works that members have in their profiles, or only selected formats/kinds.
Many releases in one tick are grouped into a few compact messages with a menu of the works. **Every notification has a
"أظهر العمل" button (or menu) that opens the work card as a PRIVATE message for whoever pressed it**; the notification
itself is never edited. Its buttons stay active for **one hour** from sending (not extended by presses), then they are greyed out.

### Chapter notifications (manga, manhwa, manhua)

AniList has no chapter schedule, so chapters come from a separate `ChapterSource` layer (not the catalog providers), switched on
per server in `/admin` → **مصادر الفصول**. Only **tracked** works are checked: manga/manhwa/manhua that a member of the server has in
their profile or a reminder on. A new chapter is posted to the notifications channel with the "أظهر العمل" button and a "اقرأ" link,
and "remind me" DMs the chapter too (the work card of manga has the reminder button).

| Source | Notes |
| --- | --- |
| MangaDex (priority 1) | official API, no key; ~3 requests/second (limit ~5); only `safe` and `suggestive` ratings; the server's preferred translation language `ar` or `en` (feed `translatedLanguage` filter); credit "MangaDex" and a link to the chapter on MangaDex; no pages are fetched or hosted. |
| MangaUpdates (priority 2) | keyless release information with a polite rate (one request at a time, one second apart); the link points to MangaUpdates. |
| RSS / Atom (priority 3) | feeds an admin adds in the panel as `https://site/feed.xml | Work title`: https only, no credentials or odd ports, no private/loopback/link-local addresses (the host is resolved and the connection pinned to the checked address, redirects refused), 1 MB and 8 s limits. A feed belongs to the tracked work whose title matches; entries are deduplicated by id and chapter number. At most 10 feeds per server. |

The poller runs every 15 minutes with caches (10 min), per-source back-off (1 min doubling to 30 min) and a rotation of at most 30
works per server per round. A chapter is announced once per server (the first source in priority order wins); the first check of a
work only records old chapters, so switching a source on never floods the channel. **No website scraping and no Mihon-style
extensions.** The MangaDex, MangaUpdates and feed responses were not reachable from the development sandbox, so parsing is covered by
fixtures only.

Notes: AniList has airing data for **anime only** (not manga/manhwa chapters). "All works" is approximate: rare titles
may be missing. Webtoon-origin series come through AniList as manhwa; Webtoon/Naver are never scraped.

## Configuration

Environment variables (a `.env` file in the working directory is read if present; see `.env.example`):

| Variable | Default | Required | Meaning |
| --- | --- | --- | --- |
| `DISCORD_TOKEN` | — | **yes** | Bot token (never logged) |
| `DATABASE_URL` | `sqlite://bot.db?mode=rwc` | no | SQLite file, relative to the working directory |
| `LOG_LEVEL` | `info` | no | tracing filter |
| `VIEW_TIMEOUT_DEFAULT_SECS` | `60` | no | How long the buttons and menus of an ordinary message (search, results, cards, rosters, `/top`, `/schedule`, `/suggest`, `/recommend`, `/airing`, `/help`, `/admin`, private views) stay active; every press extends it |
| `VIEW_TIMEOUT_MAX_SECS` | `600` | no | Hard cap for those messages in total, however often they are pressed |
| `VIEW_TIMEOUT_PROFILE_SECS` | `300` | no | The same for the profile message |
| `VIEW_TIMEOUT_PROFILE_MAX_SECS` | `900` | no | Hard cap for the profile message |
| `VIEW_TIMEOUT_NOTIFICATION_SECS` | `3600` | no | Notifications and digests: fixed from the moment of sending, never extended |
| `IMAGE_CACHE_MB` | `64` | no | In-memory cache of processed custom pictures, GIFs and banners of the profile |
| `ANILIST_RATE_LIMIT_PER_MIN` | `25` | no | AniList budget (AniList is currently degraded to ~30/min); adapts to `X-RateLimit-*` / `Retry-After` |
| `COMMAND_COOLDOWN_SECS` | `3` | no | per-user cooldown |
| `DEV_GUILD_ID` | empty | no | register commands instantly in one server (otherwise global, up to ~1 hour); global commands are cleared in this mode |
| `ENABLE_FALLBACK_PROVIDERS` | `false` | no | MangaDex/MangaUpdates fallback for works AniList lacks (see below) |
| `IGDB_CLIENT_ID`, `IGDB_CLIENT_SECRET` | empty | no | enable IGDB games and game characters (see below); both are needed |
| `OWNER_ID` | empty | no | the owner's Discord user id; by default the application owner (Discord application info) is used |
| `ENABLE_VNDB` | `true` | no | VNDB visual novels and characters, no key needed; `false` switches it off |
| `TMDB_API_KEY` or `TMDB_READ_TOKEN` | empty | no | enable dramas, series and films (see *TMDB setup*) |
| `TMDB_REGION` | `OM` | no | region of the "where to watch" data (ISO 3166-1) |
| `ENABLE_TMDB` | `true` | no | `false` switches TMDB off even with a key |
| `TMDB_RATE_LIMIT_PER_10S` | `30` | no | request budget per 10 seconds (TMDB allows about 50 per second) |
| `CACHE_TTL_*` | see `.env.example` | no | cache lifetimes in seconds |

Create the Discord application at <https://discord.com/developers/applications>, add a bot, copy its token and invite
it with the `bot` and `applications.commands` scopes. No privileged intents are needed.

> **Security:** an earlier commit of this repository contained a real `DISCORD_TOKEN` in `.env`. Reset the token in the
> Developer Portal and never commit `.env` (it is git-ignored).

### Safety

Every query asks AniList for non-adult media; characters are judged by up to 50 of their appearances and hidden if any
is adult or has no known appearance; stored favourites are re-checked when shown. The `nsfw-mode` setting is stored but
content stays safe. Custom images are **off** per server until an admin enables them in `/settings` (section `images`);
they can only be set for characters already in your own profile and only appear in your profile.

### Fallback providers (optional, off by default)

`ENABLE_FALLBACK_PROVIDERS=true` lets the written-work categories fall back to MangaDex and MangaUpdates when AniList has no match for a
written work. Only `safe`/`suggestive` ratings are accepted (MangaUpdates works need known, non-adult genres), the
source is credited in the card footer, and covers are downloaded by the bot and attached (never hotlinked). These APIs
were not reachable from the development sandbox, so this path is covered by fixture tests only.

### Games and visual novels (optional)

AniList has no video games, so two optional providers sit behind the `CatalogProvider` trait. Ids are provider-prefixed
(`igdb:55`, `vndb:v17`, characters `igdb:c55` / `vndb:c12`). They feed `/game` (work and character searches, random picks), and
their works and characters can be added to profiles like any other. Pictures are downloaded and cached by the bot
(never hotlinked) and every card credits the source.

* **IGDB** (games, game characters) is free for **non-commercial use only**. To get credentials:
  1. Sign in at <https://dev.twitch.tv/console> with a Twitch account that has two-factor authentication enabled.
  2. Register an application: any name, **OAuth Redirect URL** `http://localhost`, category *Application Integration*,
     client type **Confidential**.
  3. Open *Manage* on the application, copy the **Client ID** and generate a **Client Secret**.
  4. Put them in `.env` as `IGDB_CLIENT_ID` and `IGDB_CLIENT_SECRET`. The bot fetches and refreshes the Twitch
     client-credentials token itself. Limits respected: 4 requests/second, at most 8 open requests. Footer: "Data from IGDB.com".
* **VNDB** (visual novels and characters) uses the public HTTP API v2 ("Kana", `https://api.vndb.org/kana`), no key.
  The bot paces requests below ~200 per 5 minutes and caches results; footer: "Data from VNDB". The data is under the
  VNDB data license (<https://vndb.org/d17>).
* Missing credentials disable the provider automatically. Per server: `/admin` → Safety switches game sources on/off
  (default on) and **`mature-themes`** (default on).
* **Safety is judged by content, never by age rating** (IGDB age ratings are unreliable, so they are not even requested):
  * IGDB: every query asks for `themes`, `genres` and `keywords`. A game is hidden only on a positive sexual signal: theme
    *Erotic* (42), a keyword from the blacklist in `providers/safety.rs` (`EXPLICIT_KEYWORD_PARTS` / `EXPLICIT_KEYWORDS_EXACT`,
    easy to extend) or an adult genre. A game with no age rating and clean content is shown; violent and horror games (Silent
    Hill) are shown. With `mature-themes` off, themes Horror (19) and Warfare (39) and survival-horror keywords are hidden too.
  * VNDB: hidden on a sexual-content tag (category `ero`, ids in `SEXUAL_TAG_IDS`, or a name in `SEXUAL_TAG_NAMES`), on a cover
    or screenshot flagged above safe, or on an 18+ release together with sexual tags. Characters are hidden on explicitly
    sexual traits or a flagged picture. Violence and dark themes stay visible unless `mature-themes` is off (then brutal
    images are hidden).
  * An empty theme/keyword/tag list is a real answer (nothing sexual is tagged). If a request fails, or IGDB answers with an
    error instead of games, nothing is judged or shown and the bot says the source is unavailable.
* Not verifiable offline: these APIs were unreachable from the development sandbox, so parsing and safety are covered by
  fixture tests only.

### Operations (round 8)

* **Logs:** every non-2xx answer of IGDB, VNDB, TMDB, MangaDex, MangaUpdates and RSS feeds is logged at warn level with provider, endpoint, status and a scrubbed excerpt (keys and tokens removed, 300 characters). Startup logs one line per provider: enabled or disabled and why.
* **Backups:** a consistent copy (`VACUUM INTO`) once a day into `backups/` next to the database (`/data/backups` on a volume), the newest seven kept, skipped with a warning when free space is low (three times the database plus 50 MB). A backup is also made before any migration, and the migration only runs after that backup passed `PRAGMA integrity_check` and its key tables were read back; if the backup fails the bot stops with a clear error and the database is untouched. After the migration the row counts of every user-data table are compared with the counts before: if any count dropped, the backup is restored automatically and the bot refuses to start (the log line `migration verified` reports the totals otherwise). Migrations are additive only (a test rejects `DROP`, `DELETE` or `TRUNCATE` in any new migration file). **Moving to another host:** copy the whole `/data` volume (`bot.db`, `backups/`), start the bot there with the same `DATABASE_URL`; **restoring:** stop the bot, copy a file from `/data/backups` over `bot.db` and start it again.
* **Maintenance mode** (owner): every command answers "البوت في صيانة قصيرة" except for the owner; the flag survives restarts.
* **Statistics** (owner): commands per day and per command, provider errors, cache hit rate (TMDB), API calls, active members per day. Only names and counts are stored (no message text, no ids), kept 30 days.
* **Games:** a failing provider never blocks the other; the "source not responding" message appears only when every queried provider failed. VNDB bodies are built by tested functions and checked against the documented field lists (`releases.minage` is not a `/vn` field: asking for it caused HTTP 400).

## Deploy without compiling on the host

Small hosts (bot-hosting.net and similar) kill `cargo run --release`. Build in GitHub Actions instead:

1. Open the repository's **Actions** tab, pick the latest successful **Build static binary** run and download the
   **anpot-linux** artifact (`anpot-linux.zip`).
2. Unzip it, upload `anpot` to the host's file manager and create `.env` next to it with `DISCORD_TOKEN` and
   `DATABASE_URL=sqlite://bot.db?mode=rwc`.
3. Set the startup command to `./anpot` and make sure the file is executable (`chmod +x anpot` in the console).
4. Delete `Cargo.toml` and `src/` from the host if they were uploaded earlier, so the panel does not try to compile.
5. Back up `bot.db` regularly. Before applying new migrations to an existing database the bot copies it to
   `backups/` next to it (the last 7 copies are kept).

The binary is a fully static `x86_64-unknown-linux-musl` build (rustls, bundled SQLite, mimalloc); migrations and the
font are embedded, so no other files are required. A generic `Dockerfile` is included for container hosts.

## Development

```bash
cp .env.example .env   # set DISCORD_TOKEN and DEV_GUILD_ID
cargo run
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

---

# anpot — بوت ديسكورد عربي للأنمي والمانجا والمانهوا والروايات

## البدء السريع

1. أنشئ تطبيقًا وبوتًا من <https://discord.com/developers/applications> وانسخ التوكن، ثم ادعُ البوت بنطاقَي `bot` و`applications.commands`.
2. أنشئ ملف `.env` (انسخ `.env.example`) وضع `DISCORD_TOKEN`، وللتجربة السريعة ضع `DEV_GUILD_ID` ليظهر الأمر فورًا في سيرفرك.
3. شغّل: `cargo run` ثم جرّب `/anime` و`/profile` و`/help`.

أوامر المستخدمين: `/anime` `/manga` `/novel` `/drama` `/series` `/movie` `/game` `/search` `/event` `/top` `/schedule` `/profile` `/edit` `/suggest` `/recommend` `/airing` `/settings` `/help`.
أوامر الإدارة (للمشرفين): `/admin` لوحة أزرار، `/set-bot-channel` قناة الأوامر، `/set-notifications-channel` قناة الإشعارات.

### دليل المستخدم
- **أوامر الأعمال السبعة** تعمل بالطريقة نفسها: اختر **النوع** (عمل، شخصية، عمل عشوائي، شخصية عشوائية) ثم **الاسم** من الاقتراحات، ولكل أمر فلاتره (الصيغة، السنة، الحالة، البلد، التصنيف، المنصة…). زر «عشوائي آخر» يغيّر النتيجة في الرسالة نفسها.
- **`/drama`** للدراما الآسيوية و**`/series`** لمسلسلات بقية العالم و**`/movie`** للأفلام (من TMDB)، و**`/game`** للألعاب والروايات البصرية.
- تحت كل رسالة عن شخصية زر **«أظهر العمل»**، وتحت كل رسالة عن عمل زر **«أظهر الشخصيات»**؛ كلاهما يرسل رسالة جديدة ويعمل لأي عضو. وعلى بطاقة العمل أيضًا: أضف لبروفايلي، ذكّرني، وقائمة حالتك (أشاهده الآن، أكملته، سأشاهده لاحقًا، متوقف، أسقطته).
- **`/profile`**: رسالة واحدة تضم شخصياتك وأعمالك من كل المصادر، و**`/edit`** لكل التعديلات. الغلاف يُقبل بأي مقاس كما هو (PNG وJPG وWebP وGIF حتى 8 ميغابايت) والـGIF يبقى متحركًا؛ لا قصّ ولا معاينة، والحماية تقنية فقط (نوع حقيقي وحجم وعدد بكسلات معقول).
- **`/top`** ترتيب السيرفر (أنمي، مانجا، روايات، دراما، مسلسلات، أفلام، ألعاب، شخصيات، الأعضاء، نقاط الفعاليات) بمدة أسبوع أو شهر أو الكل.
- **`/schedule`** جدول الأسبوع للأنمي والدراما والمسلسلات بتوقيتك، و**`/event`** الفعاليات الجماعية مع خيار الفئة.
- **التذكير**: زر «ذكّرني» على أي بطاقة يرسل لك رسالة خاصة عند صدور حلقة أو فصل أو فيلم، وقبلها بيوم تنبيه «قريبًا». لوحة الإشعارات والاستثناءات من `/settings` ← الإشعارات.
- **`/help`** يشرح كل أمر مسجّل فعلًا، مع أدلة سريعة وصفحة المصادر.

### دليل المشرف
- **`/set-bot-channel`** القناة الوحيدة لأوامر البوت (المشرفون مستثنون)، و**`/set-notifications-channel`** قناة الإشعارات. يتحقق البوت من صلاحياته في القناة.
- **`/admin`**: الأمان، الإشعارات، **مصادر الفصول** (مع تكرار الملخص: كل ساعة، ساعتين وهو الافتراضي، 4، 8، يوميًا)، **المسلسلات والجدول** (منطقة الجدول الزمنية، حماية الحرق، نقل بلد بين الدراما والمسلسلات)، الصور المخصصة، الرتب، القنوات، الحدود، إعادة الضبط.
- كل أحد صباح اثنين يصل ملخص أسبوعي في قناة الإشعارات.
- ما حُذف في الجولات السابقة موثّق في `CLEANUP.md`.
- **TMDB**: ضع `TMDB_API_KEY` (أو `TMDB_READ_TOKEN`) في `.env` من <https://www.themoviedb.org/settings/api>. هذا المنتج يستخدم TMDB API لكنه غير معتمد أو مصدّق من TMDB. بيانات أماكن المشاهدة من JustWatch عبر TMDB.
- الألعاب اختيارية: ضع `IGDB_CLIENT_ID` و`IGDB_CLIENT_SECRET` من <https://dev.twitch.tv/console> (تطبيق من نوع Confidential، رابط OAuth: `http://localhost`، وحساب Twitch بتحقق ثنائي) ويعمل VNDB بدون مفتاح.

> **تنبيه أمني:** نُشر توكن حقيقي سابقًا داخل `.env` في تاريخ المستودع. أعد ضبط التوكن من لوحة المطورين ولا ترفع `.env` أبدًا.

## التشغيل بدون بناء على الاستضافة

الاستضافات الصغيرة توقف أمر `cargo run --release` أثناء البناء، لذلك نبني الملف في GitHub Actions:

1. من تبويب **Actions** افتح آخر تشغيل ناجح لـ **Build static binary** ونزّل الملف **anpot-linux** (`anpot-linux.zip`).
2. فك الضغط وارفع الملف `anpot` إلى مدير الملفات في الاستضافة، وأنشئ بجانبه ملف `.env` فيه `DISCORD_TOKEN` و`DATABASE_URL=sqlite://bot.db?mode=rwc`.
3. اجعل أمر التشغيل `./anpot` وتأكد أن الملف قابل للتنفيذ (`chmod +x anpot` من الكونسول).
4. احذف `Cargo.toml` والمجلد `src` من الاستضافة إن كنت رفعتهما سابقًا حتى لا تحاول اللوحة البناء.
5. خذ نسخة احتياطية من `bot.db` بانتظام. قبل تطبيق أي ترحيل جديد ينسخ البوت القاعدة تلقائيًا إلى مجلد `backups/` (يحتفظ بآخر 7 نسخ).

الملف ثابت بالكامل (`musl`)، والترحيلات والخط مدمجان فيه، فلا حاجة لأي ملفات أخرى.

## Button lifetimes (Round 10)
| Message | Components stay active |
| --- | --- |
| Profile | 5 minutes, extended by every press, 15 minutes at most |
| Notifications and digests | 1 hour from sending, fixed |
| `/event` rounds, the weekly poll, watch-night announcements | their own timers (unchanged) |
| Every other message (search, results, rosters, `/top`, `/schedule`, `/suggest`, `/recommend`, `/airing`, `/help`, `/admin`, cards, private views) | 1 minute, extended by every press, 10 minutes at most |

At the end of the time the bot edits the message and greys out every button and menu. The content stays exactly as it was; no new message and no "expired" note are sent. A press on a control of a message that is no longer tracked (for example after a restart) is acknowledged and ignored. The sessions are stored in the `message_sessions` table (message, owner, class, expiry, the Back stack and the components as last shown, never an interaction token), so messages are greyed out on time even after a restart; ephemeral messages are tracked in memory only (their token lives 15 minutes). A sweeper runs every 30 seconds with four edits at a time and backs off on rate limits.

## Wikis (Round 11)
The bot owner can add MediaWiki sites (Fandom, wiki.gg, Wikipedia-style) as extra sources from `/admin` → **للمالك** → **إدارة الويكيات**. Only the official `api.php` is used, never HTML pages.

- **Global.** A wiki is defined once and applies to every server; there is no per-server switch. A new wiki is **enabled on creation**, with no restart (the list is read from the database with a one-minute cache that every add, edit, toggle and delete drops).
- **Owner only.** Add, edit, enable/disable, delete and "اختبار الاتصال" are refused for everyone else (the same rule as the other owner tools). Members only benefit. The panel also has an **emergency switch** for all wikis (`wiki_global_enabled`, on by default), "مسح الكاش" (the cached pages only) and a read-only line: wikis, cached pages, requests today and the last errors.
- **Adding.** Paste a page URL, a bare host or an `api.php` URL; it is normalised to the host and `/api.php` (then `/w/api.php`). The address must be HTTPS on a public host name (no localhost, no IP addresses, no `.local`/`.internal`, no other port). The bot reads `siteinfo` (name, language, license), detects the **character category** (it tries `Characters`, `Playable characters`, `Main characters`, `Non-playable characters`, `Heroes`, `Villains` and categories that start with `Char`, ignoring image/template/hub categories, and keeps the one with the most pages) and the **game category** (`Games`, `Video games`), and shows "وجدت N شخصية، مثال: …" with confirm / edit category / cancel. Nothing is saved before the owner confirms; with no category found a wiki can still be saved search-only.
- **Automatic.** For `/game`, the game-character search and `/search` the main providers and every enabled wiki are asked together; a failing wiki never fails the search (its cached pages stand in and the owner sees the last error). If the main providers have nothing, wiki results are shown directly. In `/game`, **المصدر** offers IGDB, VNDB, **كل الويكيات** (wikis only) and each wiki by name (within Discord's 25 options). Autocomplete reads only the local index (SQLite FTS5), never a wiki, because of Discord's three seconds; the index fills from searches, category fetches and a low-priority background warmer.
- **Filling gaps.** A card from IGDB or VNDB (a game or a character) that lacks a picture or a text gets them from a matching wiki page: only empty fields, never over what the provider gave, only for a strong match (the same name AND the same game for characters), cached for 7 days, the picture checked as a real image of at most 8 MB (the API thumbnails are at most 600 px), within a short budget (the lookup finishes in the background for the next view).
- **No duplicates.** Names are compared lower-cased, without accents or punctuation ("Pyramid-Head" = "pyramid head"); two characters are the same only with the same name and the same game. The same name in different games, or with an unknown game, stays separate.
- **Safety.** Every page (title, aliases, text, categories, picture address) passes the content filter (explicit words, adult categories); a doubtful page is dropped silently. There are no age ratings. Wiki data has no rating or popularity: none is shown, none is invented.
- **Attribution.** A wiki card names the wiki and its license in the footer and links the page from the title and a button (CC BY-SA).
- **Data.** Migration 0020 is additive (`wiki_sources`, `wiki_cache`, an FTS5 index). Deleting a wiki removes only its definition; its cached pages expire (a week) and are never shown meanwhile. Member data is never touched.
- **Optional setting:** `WIKI_REQUESTS_PER_SECOND` (default `2`, per wiki).

## Wikis are first-class sources (Round 11.1)
A wiki entry behaves like an entry from AniList, IGDB, VNDB or TMDB. Only real people (staff, actors, voice actors) stay out of profiles.

- **`source` everywhere.** `/anime`, `/manga`, `/novel`, `/drama`, `/series`, `/movie`, `/search` and `/game` take an optional, autocomplete-backed **المصدر**: the command's own providers, **كل الويكيات** (wikis only) or one wiki by name (25-option cap). The choice is read from the cached `wiki_sources` table, so a wiki added or removed in `/admin` shows up in every server at once with no command re-registration. Empty = the old behavior.
- **Profiles.** Every character and work card (wiki ones too) has "أضف إلى البروفايل", which becomes "إزالة من البروفايل" once the item is there. A wiki item has the key `wiki:{wiki_id}:{page_id}` (characters live in the integer id range from 4e15, works as `wiki` works) and a **snapshot** (migration 0021, additive: name, picture, summary, series, source, page URL, license). Disabling or deleting a wiki never removes or breaks an item; it keeps showing its snapshot with the source in the footer. No item limit; the profile stays paginated with its 30 s cache.
- **Editing.** Picture, nickname and every other per-item field work on wiki items and are stored apart from the source data, so cache refreshes and wiki changes never overwrite them.
- **No second class.** Wiki characters join the same ranked, de-duplicated list as the other providers; "أظهر العمل" is always on a wiki character and resolves the work on click (series field, then the same wiki, then other sources by flexible title); "أظهر الشخصيات" works on wiki works and on other works with strong wiki matches. The interactive views (`/game`, character cards) now receive the fill-missing-fields result **in place** while the view is alive (no lifetime is extended), and AniList/TMDB cards are filled too.
- **Cleaning (Mudae-style wikis).** Bot jargon is removed by one constant, `JARGON` and `IGNORED_FIELDS` in `src/wiki/clean.rs` (`rollable`, `waifu`, `husbando`, `roulette`, `kakera`, `claim`, `wishlist`, `$` commands; fields ROULETTE / OFFICIAL SITE). An empty summary stays empty. The infobox **Series** is the parent work, matched flexibly (case, accents, punctuation, "Sun/Moon" = "Sun and Moon" = "Sun & Moon"; edition words are stripped only when the rest still matches strongly). The first infobox picture is preferred. The kind is never assumed to be "game".

### Owner checklist
1. `/admin` → للمالك → إدارة الويكيات → إضافة ويكي → `silenthill.fandom.com` → check the preview → تأكيد.
2. `/game` type شخصية, name "Pyramid Head", no source: a single card with the wiki footer.
3. `/game` type عمل with a title that IGDB and VNDB do not have: the wiki results are shown.
4. Pick the source "كل الويكيات", then a single wiki.
5. Add a second wiki: it works in another server at once.
6. Disable it: gone everywhere; delete it: member data untouched.
7. Typing in the name field answers in under three seconds.

