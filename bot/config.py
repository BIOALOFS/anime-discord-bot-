from __future__ import annotations

from dataclasses import dataclass
import os
from dotenv import load_dotenv

load_dotenv()


@dataclass(frozen=True, slots=True)
class Settings:
    discord_token: str = os.getenv("DISCORD_TOKEN", "")
    database_url: str = os.getenv("DATABASE_URL", "sqlite+aiosqlite:///anpot.db")
    tmdb_token: str = os.getenv("TMDB_API_TOKEN", "")
    default_language: str = os.getenv("DEFAULT_LANGUAGE", "ar")
    prefix: str = os.getenv("COMMAND_PREFIX", "!")
    user_agent: str = os.getenv("HTTP_USER_AGENT", "AnpotBot/1.0")
    cache_ttl: int = int(os.getenv("PROVIDER_CACHE_TTL", "900"))
    max_image_bytes: int = int(os.getenv("MAX_IMAGE_BYTES", str(8 * 1024 * 1024)))
    trusted_users_file: str = os.getenv("TRUSTED_USERS_FILE", "trusted_users.json")
    test_guild_id: int | None = int(os.environ["DISCORD_TEST_GUILD_ID"]) if os.getenv("DISCORD_TEST_GUILD_ID") else None

    def validate(self) -> None:
        if not self.discord_token:
            raise RuntimeError("DISCORD_TOKEN is required")
        if self.default_language not in {"ar", "en"}:
            raise RuntimeError("DEFAULT_LANGUAGE must be ar or en")
