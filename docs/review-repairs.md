# Review repairs and verification boundaries

## Repaired implementation findings

* Ordered mutations now lock the stable user row on PostgreSQL, begin an immediate SQLite write transaction when possible, move every affected row to a unique negative position, flush, then assign contiguous positive positions. Removal, undo, and duplicate merge follow the same collision-free approach; uniqueness remains enforced.
* Provider candidates now have a selected-candidate import path that records character/work identities, relationships, images, licenses, and provider references. Re-import is idempotent and equal names do not merge without a common provider reference. Unapproved provider data remains pending.
* Required `/image` operations and `/settings avatar|banner` exist while flat legacy slash commands remain compatibility aliases. Slow slash search/download/custom paths defer before network work.
* Profiles use persisted appearance settings, Discord/default fallbacks, associated works, approved deterministic galleries, character/image counters, separate image navigation, and timeout edits.
* Prefix profile/find parsing supports the documented mention, numeric, and unquoted multiword forms.
* Custom images are stored transactionally; custom edits cover name, work, category, description, and image and reset approval.
* Display queries enforce owner-only pending custom visibility and exclude rejected/hidden content. Personal/appearance uploads are pending until separately reviewed. Approved characters with unavailable images use a safe placeholder.
* Collection search runs in the database across names, aliases, nicknames, and work titles with pagination.
* Export schema 2 includes descriptive names, works, providers, and selected image references and explicitly disclaims restore completeness.
* Image downloads use a connector resolver that validates the exact addresses supplied to the connection, revalidate redirect hosts, stream to completion with a cumulative limit, and separately retain uploads as pending moderation.
* SQLite foreign keys and busy timeout are enabled on every connection. Undo rejects re-added or newly unavailable characters, selected image deletion clears the foreign key first, provider capabilities no longer claim unimplemented methods, and TMDB role references no longer depend on cast-list position.

## External verification boundaries

The automated suite is designed for SQLite and mocked providers. Real PostgreSQL concurrency must be run against a disposable PostgreSQL database in CI. Discord registration and UI behavior can be inspected without logging in, but live interaction delivery requires a Discord application token. AniList, TMDB, Wikidata, redirects, rate limits, and operator-selected moderation services require network access and credentials where applicable; no live-provider result is fabricated. A JSON export is not an import workflow or database backup.
