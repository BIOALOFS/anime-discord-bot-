from __future__ import annotations

import asyncio
import json
import logging
from dataclasses import dataclass
from pathlib import Path
from typing import Any

log = logging.getLogger(__name__)


@dataclass(frozen=True, slots=True)
class TrustSnapshot:
    global_admins: frozenset[str]
    servers: dict[str, frozenset[str]]


class TrustedUsers:
    """Fail-closed, hot-reloaded file-backed trust store."""

    def __init__(self, path: str | Path):
        self.path = Path(path)
        self._snapshot = TrustSnapshot(frozenset(), {})
        self._signature: tuple[int, int] | None = None
        self._lock = asyncio.Lock()

    @staticmethod
    def _parse(data: Any) -> TrustSnapshot:
        if not isinstance(data, dict) or set(data) != {"global_admins", "servers"}:
            raise ValueError("expected exactly global_admins and servers")
        admins, servers = data["global_admins"], data["servers"]
        if not isinstance(admins, list) or not isinstance(servers, dict):
            raise ValueError("global_admins must be a list and servers must be an object")
        if any(not isinstance(value, str) or not value.isdigit() for value in admins):
            raise ValueError("global administrator IDs must be digit strings")
        parsed: dict[str, frozenset[str]] = {}
        for guild_id, users in servers.items():
            # The distributed empty example contains this explanatory key only.
            if guild_id == "DISCORD_SERVER_ID" and users == []:
                continue
            if not isinstance(guild_id, str) or not guild_id.isdigit():
                raise ValueError("server IDs must be digit strings")
            if not isinstance(users, list) or any(not isinstance(value, str) or not value.isdigit() for value in users):
                raise ValueError("server trusted-user IDs must be lists of digit strings")
            parsed[guild_id] = frozenset(users)
        return TrustSnapshot(frozenset(admins), parsed)

    async def reload_if_changed(self, *, force: bool = False) -> TrustSnapshot:
        async with self._lock:
            try:
                stat = await asyncio.to_thread(self.path.stat)
                signature = (stat.st_mtime_ns, stat.st_size)
                if not force and signature == self._signature:
                    return self._snapshot
                raw = await asyncio.to_thread(self.path.read_text, encoding="utf-8")
                snapshot = self._parse(json.loads(raw))
            except (OSError, ValueError, json.JSONDecodeError) as exc:
                # Never retain old privileges after deletion or an invalid edit.
                self._snapshot = TrustSnapshot(frozenset(), {})
                self._signature = None
                log.error("Trusted-user file %s is unavailable or invalid; all file trust denied: %s", self.path, exc)
                return self._snapshot
            self._snapshot, self._signature = snapshot, signature
            log.info("Trusted-user permissions loaded from %s", self.path)
            return snapshot

    async def is_global(self, user_id: int | str) -> bool:
        return str(user_id) in (await self.reload_if_changed()).global_admins

    async def is_server_trusted(self, guild_id: int | str, user_id: int | str) -> bool:
        snapshot = await self.reload_if_changed()
        return str(user_id) in snapshot.servers.get(str(guild_id), frozenset())

    async def can_manage_server(self, member: Any, guild: Any) -> bool:
        if guild is None or member is None:
            return False
        if await self.is_global(member.id) or await self.is_server_trusted(guild.id, member.id):
            return True
        return member.id == guild.owner_id or bool(getattr(getattr(member, "guild_permissions", None), "manage_guild", False))

