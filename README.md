# anpot — Arabic Discord bot for AniList

A lightweight Rust Discord bot (poise + serenity, SQLite, AniList GraphQL). All user-facing text is Arabic;
code, logs and docs are English. Adult media and characters are filtered out everywhere by default.

## Features

- `/anime`, `/manga`, `/character search`, `/airing`, `/seasonal`, `/random anime|manga|character`
- `/profile ...` global favourite-character profiles: create, add (autocomplete + bulk), remove, clear, reorder,
  move, rank, groups, notes, bio/colour/banner, view styles, privacy, export/import, compare; `/top characters`
- Safe by default: characters and media are checked against AniList adult flags (fail-closed).

## Configuration

Environment variables (a `.env` file in the working directory is read if present; see `.env.example`):

| Variable | Default | Meaning |
| --- | --- | --- |
| `DISCORD_TOKEN` | — | Bot token (required, never logged) |
| `DATABASE_URL` | `sqlite://bot.db?mode=rwc` | SQLite file, relative to the working directory |
| `LOG_LEVEL` | `info` | tracing filter |
| `ANILIST_RATE_LIMIT_PER_MIN` | `80` | AniList request budget |
| `COMMAND_COOLDOWN_SECS` | `3` | per-user cooldown |
| `DEV_GUILD_ID` | empty | register commands instantly in one server (otherwise global, up to ~1 hour) |

Create the Discord application at <https://discord.com/developers/applications>, add a bot, copy its token and invite
it with the `bot` and `applications.commands` scopes. No privileged intents are needed.

> **Security:** an earlier commit of this repository contained a real `DISCORD_TOKEN` in `.env`. Reset the token in the
> Developer Portal and never commit `.env` (it is now git-ignored).

## Deploy without compiling on the host

Small hosts (bot-hosting.net and similar) kill `cargo run --release`. Build in GitHub Actions instead:

1. Open the repository's **Actions** tab, pick the latest successful **Build static binary** run and download the
   **anpot-linux** artifact (`anpot-linux.zip`).
2. Unzip it, upload `anpot` to the host's file manager and create `.env` next to it with `DISCORD_TOKEN` and
   `DATABASE_URL=sqlite://bot.db?mode=rwc`.
3. Set the startup command to `./anpot` and make sure the file is executable (`chmod +x anpot` in the console).
4. Delete `Cargo.toml` and `src/` from the host if they were uploaded earlier, so the panel does not try to compile.
5. Back up `bot.db` regularly.

The binary is a fully static `x86_64-unknown-linux-musl` build (rustls, bundled SQLite); migrations and the font are
embedded, so no other files are required.

## Deploy on Fly.io

One machine, one volume, no HTTP port (the bot is a worker):

```bash
fly volume create data --size 1
fly secrets set DISCORD_TOKEN=...
fly deploy --ha=false
fly scale count 1
```

`fly.toml` mounts the volume at `/data` and sets `DATABASE_URL=sqlite:///data/bot.db?mode=rwc`.

## Development

```bash
cp .env.example .env   # set DISCORD_TOKEN and DEV_GUILD_ID
cargo run
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

---

# anpot — بوت ديسكورد عربي لـ AniList

## البدء السريع

1. أنشئ تطبيقًا وبوتًا من <https://discord.com/developers/applications> وانسخ التوكن، ثم ادعُ البوت بنطاقَي `bot` و`applications.commands`.
2. أنشئ ملف `.env` (انسخ `.env.example`) وضع `DISCORD_TOKEN`، وللتجربة السريعة ضع `DEV_GUILD_ID` ليظهر الأمر فورًا في سيرفرك.
3. شغّل: `cargo run` ثم جرّب `/anime` و`/profile create`.

> **تنبيه أمني:** نُشر توكن حقيقي سابقًا داخل `.env` في تاريخ المستودع. أعد ضبط التوكن من لوحة المطورين ولا ترفع `.env` أبدًا.

## التشغيل بدون بناء على الاستضافة

الاستضافات الصغيرة توقف أمر `cargo run --release` أثناء البناء، لذلك نبني الملف في GitHub Actions:

1. من تبويب **Actions** افتح آخر تشغيل ناجح لـ **Build static binary** ونزّل الملف **anpot-linux** (`anpot-linux.zip`).
2. فك الضغط وارفع الملف `anpot` إلى مدير الملفات في الاستضافة، وأنشئ بجانبه ملف `.env` فيه `DISCORD_TOKEN` و`DATABASE_URL=sqlite://bot.db?mode=rwc`.
3. اجعل أمر التشغيل `./anpot` وتأكد أن الملف قابل للتنفيذ (`chmod +x anpot` من الكونسول).
4. احذف `Cargo.toml` والمجلد `src` من الاستضافة إن كنت رفعتهما سابقًا حتى لا تحاول اللوحة البناء.
5. خذ نسخة احتياطية من `bot.db` بانتظام.

الملف ثابت بالكامل (`musl`)، والترحيلات والخط مدمجان فيه، فلا حاجة لأي ملفات أخرى.
