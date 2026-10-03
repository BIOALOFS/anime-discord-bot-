# Architecture and production checklist

`bot/commands` adapts Discord input only. `bot/services` owns validation and transactions. `bot/providers` normalizes external data without persistence assumptions. `bot/database` uses SQLAlchemy asyncio and provider references remain distinct from internal UUIDs. Equal names never cause cross-source merging: work, version/adaptation and verified identifiers require maintainer review.

Every Discord user ID maps to one `users` row and one position-indexed collection. There is no list/folder entity. A uniqueness constraint prevents duplicate internal characters; versions remain separate internal IDs. Personal images, nicknames and notes belong to collection entries, never shared characters. Removal closes position gaps and stores a JSON undo snapshot.

Profile fallbacks use the current Discord display avatar and display name; fetch a full `discord.User` when an account banner is needed. A tasteful generated placeholder is used when neither personal nor approved catalog image exists. Profile component checks bind navigation to the opening user, including while browsing somebody else.

Maintainer commands expose review/approve/reject, metadata correction, duplicate merge, image replacement, reports, hiding, and provider health only after a fresh `TrustedUsers.is_global` check, in one DB transaction, with an `AuditLog`. Duplicate merges repoint collection rows, preserve personal fields and galleries, deduplicate conflicts, and normalize positions before hiding the source. Global authority is never inferred from guild roles.

`TrustedUsers` is the fail-closed, hot-reloaded authorization source. Global trust gates every shared-catalog operation; server ownership, Manage Server, or trust scoped to the current guild gates setup only. `ServerConfigurationService` separately persists setup state and channels. Ordinary command and component gates share that service, resolve threads through their parent, and prune deleted channels without falling back to all-channel mode. Administrative confirmations bind to their initiator and rerun the live trust/Discord permission check.

Before production: supply Discord/TMDB credentials; choose durable reviewed-image storage; configure human moderators and optionally an image-safety vendor; replace the generic placeholder with an operator-owned asset; satisfy TMDB attribution branding; define retention/privacy policy; exercise restore; and run integration tests against PostgreSQL. External moderation and provider credentials are operator-supplied and are not bundled.
