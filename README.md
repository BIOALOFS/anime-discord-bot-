# anpot — Arabic Discord bot for anime, manga, manhwa, manhua and novels

A lightweight Rust Discord bot (poise + serenity, SQLite, AniList GraphQL). All user-facing text is Arabic;
code, logs and docs are English. Adult media and characters are filtered out everywhere by default.

## Commands

Seventeen user commands and three admin commands. The seven category commands share one shape, so learning one teaches all.

| Command | What it does |
| --- | --- |
| `/anime` `/manga` `/novel` `/drama` `/series` `/movie` `/game` | Same options everywhere: **type** (work, character, random work, random character), **name** (autocomplete; optional for random picks) and the filters of the category: anime — format, year, status; manga — format (manga, manhwa, manhua, one-shot), status; novel — origin, status; drama — country, year, status; series — country, year, status, genre; movie — year, genre, country; game — platform, source (IGDB games or VNDB visual novels). Filters apply to searches and to random picks. A random pick avoids your last five and the **"عشوائي آخر"** button re-rolls in the same message. `/drama` lists Asian productions, `/series` the rest of the world (an admin can move a country from `/admin` → المسلسلات والجدول). |
| `/event` | **Collective games** with a `category` option (anime, manga, novel, drama, series, movie): any member of the channel can answer, the first correct answer wins the round's points (speed bonus), one game per channel, only the starter or an admin can skip or end. Modes: `character-image`, `work-cover`, `character-hints`, `true-false`, `higher-lower`, `order-years`, `race`, plus `versus` and `leaderboard`. The character modes need character pictures, so they are not offered for dramas, series and films. The answer's "أظهر العمل / أظهر الشخصيات" button appears only after the reveal. |
| `/top` | The **server's** rankings (never across servers): anime, manga, novels, drama, series, films, games, characters, members, event points; a time range (week, month, all time); a hero card for first place, places 2–10 as a list, pages, and a menu that opens any item as a new message. Cached for five minutes. |
| `/schedule` | This week's calendar: anime from AniList (exact times) and dramas/series from TMDB (the day), in your time zone (default `Asia/Muscat`, set per server in `/admin`, per member in `/settings`). Day and category selects, "أعمالي فقط", pages, and a menu that opens any work. |
| `/profile` | **One message, one visual block** (Components V2; embeds as a fallback): banner (PNG/JPG/WebP or animated GIF), a compact info line (`الشخصيات 4 · الأعمال 1 · عام`), the current item's picture and text, the counter `1/4`. It holds characters and works of **every** source: AniList, TMDB shows and films, TMDB cast entries (with the actor's photo, labelled), games, visual novels. A work shows your watch status as a text line. |
| `/edit` | Every modification. `target` = `profile` (banner, description, title, colour, privacy, reset, export/import/delete data), `character` or `work`. **Banner:** recommended **1200×400**; ratio between 2:1 and 4:1, at least 600 px wide, 3 MB. A still picture is center-cropped to 3:1 and you see a preview with *confirm / cancel*; a GIF must already have a fitting ratio (it is never cropped). 16 ready-made banners at exactly 1200×400. |
| `/suggest` | Member suggestions: `add` links two works with a reason ≤300 chars, `random` shows one from this server, `mine` lists yours. |
| `/recommend` | Automatic AniList recommendations from your favourites (with `user`: for both of you). |
| `/airing` | Next episode of an anime. |
| `/settings` | Personal preferences: language, privacy, reminders (max 50) and **notifications** (a private panel: a switch, a filter mode and a digest interval per category, time zone, the hour of the weekly summary, the spoiler guard and what you never want to hear about). |
| `/help` | One ephemeral message built from the real command list: a section menu, a command menu, three quick guides (add a character to my profile, turn on a reminder, customise my profile) and the credits page. |
| `/admin` | Admin panel (Manage Guild): Safety, Notifications, Custom images, Roles, Channels, Limits, Chapter sources (incl. digest interval), **المسلسلات والجدول** (schedule time zone, spoiler guard, moving countries between drama and series) and Reset. |
| `/set-bot-channel` | Admin-only: the single channel where commands work (admins may bypass). |
| `/set-notifications-channel` | Admin-only: where new episode, chapter and weekly-summary messages are posted. |

**Every message about a character carries "أظهر العمل" and every message about a work carries "أظهر الشخصيات"** (a work card also has add-to-profile, a 🔔 reminder button and a watch-status select: watching or reading now, completed, later, paused, dropped). Both buttons send a **new** message, never edit the old one, and any member can press them; lists use select menus that open items as new messages. The profile is the one exception. Removed commands: `/search`, `/character` and `/random` (their jobs are in the category commands); the old minigame `/game` is now `/event`, and `/game` is the games category.

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
"أظهر العمل" button (or menu) that sends the work card as a NEW message** (ephemeral where the server restricts the bot
channel); the notification itself is never edited.

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
| `ANILIST_RATE_LIMIT_PER_MIN` | `25` | no | AniList budget (AniList is currently degraded to ~30/min); adapts to `X-RateLimit-*` / `Retry-After` |
| `COMMAND_COOLDOWN_SECS` | `3` | no | per-user cooldown |
| `DEV_GUILD_ID` | empty | no | register commands instantly in one server (otherwise global, up to ~1 hour); global commands are cleared in this mode |
| `ENABLE_FALLBACK_PROVIDERS` | `false` | no | MangaDex/MangaUpdates fallback for works AniList lacks (see below) |
| `IGDB_CLIENT_ID`, `IGDB_CLIENT_SECRET` | empty | no | enable IGDB games and game characters (see below); both are needed |
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

أوامر المستخدمين: `/anime` `/manga` `/novel` `/drama` `/series` `/movie` `/game` `/event` `/top` `/schedule` `/profile` `/edit` `/suggest` `/recommend` `/airing` `/settings` `/help`.
أوامر الإدارة (للمشرفين): `/admin` لوحة أزرار، `/set-bot-channel` قناة الأوامر، `/set-notifications-channel` قناة الإشعارات.

### دليل المستخدم
- **أوامر الأعمال السبعة** تعمل بالطريقة نفسها: اختر **النوع** (عمل، شخصية، عمل عشوائي، شخصية عشوائية) ثم **الاسم** من الاقتراحات، ولكل أمر فلاتره (الصيغة، السنة، الحالة، البلد، التصنيف، المنصة…). زر «عشوائي آخر» يغيّر النتيجة في الرسالة نفسها.
- **`/drama`** للدراما الآسيوية و**`/series`** لمسلسلات بقية العالم و**`/movie`** للأفلام (من TMDB)، و**`/game`** للألعاب والروايات البصرية.
- تحت كل رسالة عن شخصية زر **«أظهر العمل»**، وتحت كل رسالة عن عمل زر **«أظهر الشخصيات»**؛ كلاهما يرسل رسالة جديدة ويعمل لأي عضو. وعلى بطاقة العمل أيضًا: أضف لبروفايلي، ذكّرني، وقائمة حالتك (أشاهده الآن، أكملته، سأشاهده لاحقًا، متوقف، أسقطته).
- **`/profile`**: رسالة واحدة تضم شخصياتك وأعمالك من كل المصادر، و**`/edit`** لكل التعديلات. الغلاف الموصى به **1200×400** (النسبة بين 2:1 و4:1 وأقل عرض 600 بكسل)؛ الصورة الثابتة تُقصّ من المنتصف مع معاينة وزرّي «اعتمد» و«إلغاء»، والـGIF يُقبل فقط إن كانت نسبته مناسبة.
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
