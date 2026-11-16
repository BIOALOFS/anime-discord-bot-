# anpot — Arabic Discord bot for anime, manga, manhwa, manhua and novels

A lightweight Rust Discord bot (poise + serenity, SQLite, AniList GraphQL). All user-facing text is Arabic;
code, logs and docs are English. Adult media and characters are filtered out everywhere by default.

## Commands

Twelve user commands and three admin commands, with chained options instead of dozens of subcommands:

| Command | What it does |
| --- | --- |
| `/profile` | **One message, one visual block** (Discord Components V2: a single container; if Discord refuses V2 the same page is sent as two embeds). In order: the banner (PNG/JPG/WebP or an animated GIF), a compact info text (name/title, bio, `الشخصيات 4 · الأعمال 1 · عام · انضم 2026-11-15`), the current item's picture and its text with the counter `1/4` shown once. The container's side-bar is the colour you chose in `/edit` (presets or `#hex`, reset to default). Components are exactly: row 1 `⏮ ◀ mode-toggle ▶ ⏭` (the toggle names the *other* mode: "أعمال" while viewing characters, "شخصيات" while viewing works; it resets to item 1), row 2 a dropdown of the items on this page (25), row 3 a dropdown to switch pages (grouped ranges beyond 25 pages). `action:add-character` / `add-work` + `value` adds favourites. Only the person who ran the command can press the controls (others get an ephemeral hint). |
| `/edit` | Every modification. `target` = `profile` (banner, description ≤300, title ≤40, colour, privacy, reset, export/import/delete data), `character` (image incl. GIF for characters in your profile, title ≤40, description ≤300, reset, remove, move) or `work` (remove, move). The banner action without a value opens a menu of **16 ready-made banners** (gradients and patterns drawn in code, no bundled pictures) with a preview; you can also upload a picture/GIF (3 MB, needs custom images enabled in the server), give a colour like `#ff8800`, or reset. Confirmations are ephemeral with an undo button. |
| `/search` | Works only: `type` (anime, manga, manhwa, manhua, novel, plus game / visual novel when those providers are enabled) → `name` with type-aware autocomplete |
| `/character` | `name` (autocomplete ranked by popularity, main work in the label) + optional `work` filter + optional `source` (all, anime/manga, games). Several matches → a select menu |
| `/airing` | Next episode of an anime or this week's schedule |
| `/random` | Random work/character, today's spotlight, today's character birthdays |
| `/top` | Most favourited characters or works (server or global); characters come with a dropdown that opens their card |
| `/recommend` | Automatic AniList recommendations from your favourites (with `user`: for both of you) |
| `/suggest` | Member suggestions: `add` links two works of any provider/type (anime, manga, novel, game, visual novel) with a reason ≤300 chars; `random` shows one from this server. Layout: **both covers first** (side by side in a media gallery), then the reason, the author mention (no ping) and the added date, then buttons (another, open first/second work, delete for the author or Manage Guild) and a menu that opens either work's card as a new ephemeral message; `mine` lists yours with a menu to delete one. The same pair (either order) can only be added once per server; links and mentions are stripped from reasons; daily limit per member (default 10, set in `/admin` → Limits). |
| `/game` | **Collective games**: any member of the channel can answer, the first correct answer wins the round's points (speed bonus), wrong answerers are locked out of that round, presses are rate-limited, one game per channel; only the starter or an admin can skip or end; the final scoreboard and a weekly per-server leaderboard are kept. Modes (with a `difficulty` option): `character-image` (a picture that sharpens as hints arrive), `work-cover`, `character-hints`, `true-false`, `higher-lower`, `order-years`, `race` (10 rounds), plus `versus` and `leaderboard`. |
| `/settings` | Personal preferences only: language, privacy and your **reminders** (list and cancel; max 50) |
| `/help` | Commands by category with examples |
| `/admin` | Admin panel (Manage Guild, hidden from others): ephemeral buttons for Safety (NSFW, mature games, games), Notifications (on/off, filter mode), Custom images (on/off, who may upload), Roles, Channels (read-only summary, unset), Limits (cooldown) and Reset |
| `/set-bot-channel` | Admin-only, one `channel` option: the single channel where commands work (admins may bypass) |
| `/set-notifications-channel` | Admin-only, one `channel` option: where new episode notifications are posted |

Under every character image (from `/character`, a work's cast, `/top`, `/random`, a game round or a roster) there are the same buttons: add to / remove from my profile, works, source link and back. Inside `/profile` there are none. Every anime card has a 🔔 **remind me** button: when a new episode is released the bot DMs you (if your DMs are closed and the server allows it in `/admin` → Notifications, it pings you in the notifications channel). Reminders reuse the batched airing fetch, so they add no AniList calls of their own.

Both channel commands check that the bot can view, send, embed links and attach files there. Unsetting a channel is done
in `/admin` → Channels. Wherever a message lists character names it carries a dropdown (max 25 options, provider-prefixed
values) that opens the character in the same message with a Back button: `/character` results, work rosters, the cast on a
work card, `/top`, `/random` birthdays and profile cards.

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

`ENABLE_FALLBACK_PROVIDERS=true` lets `/search` fall back to MangaDex and MangaUpdates when AniList has no match for a
written work. Only `safe`/`suggestive` ratings are accepted (MangaUpdates works need known, non-adult genres), the
source is credited in the card footer, and covers are downloaded by the bot and attached (never hotlinked). These APIs
were not reachable from the development sandbox, so this path is covered by fixture tests only.

### Games and visual novels (optional)

AniList has no video games, so two optional providers sit behind the `CatalogProvider` trait. Ids are provider-prefixed
(`igdb:55`, `vndb:v17`, characters `igdb:c55` / `vndb:c12`). They feed `/search type:game|vn` and `/character`, and
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
* Missing credentials disable the provider automatically. Per server: `/admin` → Safety
  switches game sources on/off (default on) and the mature-games toggle (default on) hides M / PEGI 18 / CERO Z games when off.
* **Safety:** only sexual content is blocked. IGDB: theme "Erotic" and ESRB AO are hidden. VNDB: any sexual-content ("ero")
  tag or a suggestive/explicit image flag is hidden. Violent or horror games (for example Silent Hill, M / PEGI 18) stay
  visible unless `mature-games` is off. An item without a reliable verdict (no age rating on IGDB, no tags on VNDB) is hidden.
  Game characters are shown only if their games pass.
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
3. شغّل: `cargo run` ثم جرّب `/search` و`/profile`.

أوامر المستخدمين: `/profile` (عرض بطاقات مع غلاف وإضافة) `/edit` (كل التعديلات) `/search` `/character` `/airing` `/random` `/top` `/recommend` `/game` `/settings` (اللغة والخصوصية) `/help`.
أوامر الإدارة (للمشرفين): `/admin` لوحة أزرار، `/set-bot-channel` قناة الأوامر، `/set-notifications-channel` قناة إشعارات الحلقات. إشعارات الحلقات تغطي كل الأنمي تلقائيًا، والتصفية من لوحة الإدارة.

### دليل المستخدم
- **`/profile`**: رسالة واحدة: الغلاف (يدعم GIF متحرك)، بطاقة المعلومات، بطاقة العنصر. الصف الأول `⏮ ◀ زر التبديل ▶ ⏭` (يعرض اسم الوضع الآخر: «أعمال» أو «شخصيات»)، والثاني قائمة للقفز إلى أي عنصر في الصفحة، والثالث قائمة لتغيير الصفحة.
- **`/edit`**: كل التعديلات. للغلاف: بدون قيمة تفتح قائمة 16 غلافًا جاهزًا مع معاينة، أو أرفق صورة/GIF (حتى 3MB)، أو لونًا مثل `#ff8800`، أو `reset`.
- **أزرار الشخصية** تحت كل صورة شخصية (خارج البروفايل): إضافة/إزالة من بروفايلي، الأعمال، رابط المصدر، رجوع.
- **زر «ذكّرني»** على بطاقة الأنمي: رسالة خاصة عند صدور حلقة جديدة؛ قائمة تذكيراتك من `/settings setting:reminders`.
- **`/suggest`**: `add` لربط عملين من أي نوع مع السبب، `random` اقتراح عشوائي (صورتا العملين أولًا ثم السبب والمؤلف والتاريخ، وقائمة لفتح بطاقة أي عمل)، `mine` اقتراحاتك مع الحذف.
- **لون البروفايل**: `/edit` ← البروفايل ← اللون (ألوان جاهزة أو `#ff8800` أو `reset`). البروفايل رسالة واحدة وكتلة واحدة.
- **`/game`**: ألعاب جماعية يجيب فيها أي عضو في القناة؛ أسرع إجابة صحيحة تكسب، والتخطي والإنهاء لمن بدأ اللعبة أو للمشرفين، مع ترتيب أسبوعي لكل سيرفر.
- **زر «أظهر العمل»** في كل إشعار يرسل بطاقة العمل في رسالة جديدة ولا يعدّل الإشعار.

### دليل المشرف
- **`/set-bot-channel`** القناة الوحيدة لأوامر البوت (المشرفون مستثنون)، و**`/set-notifications-channel`** قناة إشعارات الحلقات. يتحقق البوت من صلاحياته في القناة.
- **`/admin`** (لوحة أزرار خاصة بك): الأمان، الإشعارات (التشغيل، نوع التصفية، تنبيه التذكير بالقناة)، **مصادر الفصول** (MangaDex وMangaUpdates وروابط RSS مع لغة عربية أو إنجليزية؛ تُنشر فصول الأعمال المتابَعة فقط)، الصور المخصصة (التفعيل ومن يرفع)، الرتب، القنوات (عرض وإلغاء التحديد)، الحدود (مهلة الأوامر وحد الاقتراحات اليومي)، إعادة ضبط بيانات السيرفر بتأكيد.
- ما حُذف في الجولة الخامسة موثّق في `CLEANUP.md`.
`/search` للأعمال فقط و`/character` للشخصيات (أنمي/مانجا وألعاب). الألعاب اختيارية: ضع `IGDB_CLIENT_ID` و`IGDB_CLIENT_SECRET` من <https://dev.twitch.tv/console> (تطبيق من نوع Confidential، رابط OAuth: `http://localhost`، وحساب Twitch بتحقق ثنائي) ويعمل VNDB بدون مفتاح. اكتب `/help` لرؤية الأمثلة. أوامر الإدارة تحتاج صلاحية إدارة السيرفر.

> **تنبيه أمني:** نُشر توكن حقيقي سابقًا داخل `.env` في تاريخ المستودع. أعد ضبط التوكن من لوحة المطورين ولا ترفع `.env` أبدًا.

## التشغيل بدون بناء على الاستضافة

الاستضافات الصغيرة توقف أمر `cargo run --release` أثناء البناء، لذلك نبني الملف في GitHub Actions:

1. من تبويب **Actions** افتح آخر تشغيل ناجح لـ **Build static binary** ونزّل الملف **anpot-linux** (`anpot-linux.zip`).
2. فك الضغط وارفع الملف `anpot` إلى مدير الملفات في الاستضافة، وأنشئ بجانبه ملف `.env` فيه `DISCORD_TOKEN` و`DATABASE_URL=sqlite://bot.db?mode=rwc`.
3. اجعل أمر التشغيل `./anpot` وتأكد أن الملف قابل للتنفيذ (`chmod +x anpot` من الكونسول).
4. احذف `Cargo.toml` والمجلد `src` من الاستضافة إن كنت رفعتهما سابقًا حتى لا تحاول اللوحة البناء.
5. خذ نسخة احتياطية من `bot.db` بانتظام. قبل تطبيق أي ترحيل جديد ينسخ البوت القاعدة تلقائيًا إلى مجلد `backups/` (يحتفظ بآخر 7 نسخ).

الملف ثابت بالكامل (`musl`)، والترحيلات والخط مدمجان فيه، فلا حاجة لأي ملفات أخرى.
