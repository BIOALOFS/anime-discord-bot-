# anpot — Arabic Discord bot for anime, manga, manhwa, manhua and novels

A lightweight Rust Discord bot (poise + serenity, SQLite, AniList GraphQL). All user-facing text is Arabic;
code, logs and docs are English. Adult media and characters are filtered out everywhere by default.

## Commands

Nine commands, with chained options (category → action → value) instead of dozens of subcommands:

| Command | What it does |
| --- | --- |
| `/profile` | Show a profile (yours or `user`'s). Options `category` → `action` → `value` (+ `image`, `text`): add characters/works, customize (colour, bio, template, custom image/title/description per character), organize (reorder, groups, tiers, notes), share (1200×630 card), privacy (visibility, JSON export/import, delete all my data), compare, lists (watched/planning) |
| `/search` | Works only: `type` (anime, manga, manhwa, manhua, novel, plus game / visual novel when those providers are enabled) → `name` with type-aware autocomplete |
| `/character` | `name` (autocomplete ranked by popularity, main work in the label) + optional `work` filter + optional `source` (all, anime/manga, games). Several matches → a select menu |
| `/airing` | Next episode of an anime or this week's schedule |
| `/random` | Random work/character, today's spotlight, today's character birthdays |
| `/top` | Most favourited characters or works (server or global) |
| `/recommend` | Suggestions from your favourites (with `user`: for both of you) |
| `/game` | Guess the character, guess the anime, versus, leaderboard |
| `/settings` | (Manage Guild) command channel lock, airing feed + filters, notification list, custom images + moderation, games, birthdays |
| `/help` | Commands by category with examples |

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
* Missing credentials disable the provider automatically. Per server: `/settings section:games option:toggle`
  switches game sources on/off (default on) and `option:mature-games` (default on) hides M / PEGI 18 / CERO Z games when off.
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

الأوامر العشرة: `/profile` `/search` `/character` `/airing` `/random` `/top` `/recommend` `/game` `/settings` `/help`.
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
