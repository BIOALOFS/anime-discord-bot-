from __future__ import annotations
import asyncio
import time
from typing import Any
import aiohttp
from .base import ProviderError


class CachedHTTP:
    def __init__(self, session: aiohttp.ClientSession, ttl: int = 900):
        self.session = session
        self.ttl = ttl
        self.cache: dict[str, tuple[float, Any]] = {}
        self.lock = asyncio.Lock()

    async def json(self, method: str, url: str, *, cache_key: str, **kwargs: Any) -> Any:
        cached = self.cache.get(cache_key)
        if cached and cached[0] > time.monotonic():
            return cached[1]
        try:
            async with self.session.request(method, url, timeout=aiohttp.ClientTimeout(total=15), **kwargs) as response:
                if response.status == 429:
                    raise ProviderError("provider rate limit reached; try later")
                if response.status >= 400:
                    raise ProviderError(f"provider returned HTTP {response.status}")
                value = await response.json(content_type=None)
        except (aiohttp.ClientError, asyncio.TimeoutError) as exc:
            raise ProviderError(f"provider unavailable: {type(exc).__name__}") from exc
        async with self.lock:
            self.cache[cache_key] = (time.monotonic() + self.ttl, value)
        return value

