# Anpot — Discord character collections

Anpot is an asynchronous, bilingual (Arabic-default/English) Discord bot for one personal fictional-character collection per Discord account. It has no draws, currency, rarity, ownership scarcity, NSFW mode, folders, or user-created sublists. The catalog is deliberately **not** represented as exhaustive.

## Install and run

Requires Python 3.11+.

```bash
python -m venv .venv
. .venv/bin/activate
pip install -r requirements.txt
cp .env.example .env                 # fill DISCORD_TOKEN and optional TMDB token
# edit trusted_users.json manually (never through Discord commands)
alembic upgrade head
python -m bot
```

Enable **Message Content Intent** in both the Discord Developer Portal and the bot code (already enabled in `bot/main.py`) for prefix commands. Invite with `bot` and `applications.commands` scopes; normal View Channel, Send Messages, Embed Links, Attach Files, Read Message History, and Send Messages in Threads permissions are required where used. Discord server permissions never grant shared-catalog authority.

Slash commands are synchronized during startup. Set `DISCORD_TEST_GUILD_ID` for immediate guild-only development copies; without it, global propagation can take time. Never publish `.env`.

## Trust and server channel setup

`trusted_users.json` is the sole source of bot trust. IDs must remain quoted strings to avoid numeric precision loss:

```json
{"global_admins": ["123..."], "servers": {"456...": ["789..."]}}
```

Edit the file directly; there is intentionally no Discord command or component that changes trust. `global_admins` can manage the shared catalog and administer every server. Per-server entries apply only to that server. Server owners and members with Manage Server can configure that server, but cannot manage the global catalog. The bot checks the file modification signature on every protected operation and confirmation. Missing, malformed, incorrectly typed, or unreadable files fail closed, log an error, and grant no file-based access. Keep this file readable only by the bot operator and do not publish its contents.

An authorized server administrator must first run `!setup #channel` or `/setup channel:`. Until then ordinary commands are blocked. Use `!channel add`, `remove`, `list`, or `all` (and the matching `/channel` subcommands). Replacing setup, enabling all channels, or leaving all-channel mode requires an initiator-only confirmation with a fresh permission check. Prefix commands outside configured channels are silent; slash commands explain the allowed channels ephemerally. A thread works when its parent is allowed and Discord permissions permit it. Deleted configured channels are pruned; losing the final restricted channel returns the server to setup-required rather than enabling every channel.

## Providers, credentials, attribution, and limitations

* **AniList** uses the public GraphQL endpoint for anime/manga characters, names, images, descriptions, and media. Queries require an explicitly non-adult associated work. AniList's `isAdult` is only one signal; unknown entries remain pending rather than approved. Review the [AniList API documentation and terms](https://docs.anilist.co/) before deployment.
* **TMDB** requires a v4 read token. It searches films/TV and reads credits. A cast credit's `character` string is stored as the role and the `name` as supplementary actor metadata. Actor IDs never become character identity and actor portraits are never presented as verified character images. TMDB does not offer equivalent global fictional-character search. This product uses the TMDB API but is not endorsed or certified by TMDB; operators must display the approved TMDB logo/notice required by the current [TMDB attribution rules](https://developer.themoviedb.org/docs/attribution) and check its terms before commercial use.
* **Wikidata** performs bounded entity searches and claim lookups (not unrestricted SPARQL). Only direct fictional-character instances are returned; candidates and Wikimedia Commons images remain pending. The Commons file page is retained as the source/license lookup—the software never assumes public access means unrestricted use. See the [Wikibase API](https://www.mediawiki.org/wiki/Wikibase/API) and [Wikidata query-service guidance](https://www.wikidata.org/wiki/Wikidata:SPARQL_query_service/Wikidata_Query_Help).

Provider responses are cached in-process for `PROVIDER_CACHE_TTL`. Each provider advertises capabilities explicitly. Partial failures are shown to users and successful sources remain usable; no failed search is described as exhaustive. Saved collection data never depends on a live provider call.

## Safety and review

Adult-only, pornographic, sexually explicit, and graphic-gore material is prohibited everywhere, without override. Provider flags, bounded keyword checks, HTTPS/type/size/dimension checks, and human moderation are layered controls—not guarantees. Unknown classifications, custom entries, and unverified images are pending by default. Pending personal customs are visible only to their owner; shared visibility requires maintainer approval. Reports support metadata, duplicates, broken images, and excluded content.

Remote downloads reject non-HTTPS URLs, credentials in URLs, non-global DNS results, unsafe redirect targets, unsupported MIME types, oversized payloads, and decompression-bomb-scale dimensions. Production operators should additionally configure a dedicated image moderation service and durable object storage; Discord CDN and external URLs can expire and are not backups.

Review workflow: inspect source, identity, work/version, rating, description, every image and license; approve only when confidently safe. Reject or hide otherwise. Catalog actions are stored in `audit_logs`; channel changes store actor, server, prior/new state, and timestamp in `server_audit_logs`. Trust never permits editing somebody else's private profile or bypassing excluded-content rules.

User-supplied character images, avatars, banners, custom entries, and provider candidates without independent approval are persisted as **pending**, not displayed publicly. File-format and network validation do not constitute content moderation. Configure human reviewers and, if desired, an external moderation service before production. Catalog images, personal images, and appearance overrides have separate approval operations.

## Persistence, migration, and backup

SQLite is appropriate for one bot process. Back it up while writes are stopped:

```bash
sqlite3 anpot.db '.backup backups/anpot-$(date +%F).db'
sqlite3 backups/anpot-2026-01-01.db 'PRAGMA integrity_check;'
```

For PostgreSQL set `DATABASE_URL=postgresql+asyncpg://user:pass@host/db`, create an empty database, and run `alembic upgrade head`; use `pg_dump --format=custom` / `pg_restore`. SQLite and PostgreSQL differ in concurrency, locking, JSON behavior, collations, and constraint timing. Do not copy the SQLite file: migrate with an ETL/export process, preserve UUID/provider IDs, validate row counts and unique positions, then test against a real PostgreSQL service. Run migrations before each release and take a provider-independent database backup first.

## Operations

Run one SQLite instance under systemd/Docker with restart policy and graceful SIGTERM. PostgreSQL supports multiple bot workers, but interaction routing and cache invalidation still require coordination. Monitor Discord/API error rates, migration status, disk, DB latency, review queues, and broken-image reports. Pin updates are intentional; review changelogs and run tests before upgrading.

See [COMMANDS.md](COMMANDS.md) for command syntax and [docs/architecture.md](docs/architecture.md) for identity, moderation, and deployment notes.

## Tests

```bash
pytest -q
```

The suite covers ordered transactional collection operations, duplicate prevention, ownership isolation, content filtering, and provider partial failure. CI should also run it with `TEST_DATABASE_URL` pointing to disposable PostgreSQL and apply Alembic to both databases.

See [`docs/review-repairs.md`](docs/review-repairs.md) for repaired findings, automated coverage, and live tests that still require Discord/provider credentials or a PostgreSQL service.
