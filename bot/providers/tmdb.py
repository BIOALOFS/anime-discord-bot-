from __future__ import annotations
from .base import Capability, CharacterProvider, NormalizedCharacter, NormalizedWork, ProviderError
from .http import CachedHTTP


class TMDBProvider(CharacterProvider):
    name = "tmdb"
    capabilities = frozenset({Capability.WORK_SEARCH, Capability.WORK_CHARACTERS, Capability.CLASSIFICATIONS})
    base = "https://api.themoviedb.org/3"

    def __init__(self, http: CachedHTTP, token: str): self.http, self.token = http, token
    def _headers(self):
        if not self.token: raise ProviderError("TMDB_API_TOKEN is not configured")
        return {"Authorization": f"Bearer {self.token}"}

    async def search_works(self, query: str, page: int = 1):
        data = await self.http.json("GET", f"{self.base}/search/multi", cache_key=f"tmdb:search:{query}:{page}", headers=self._headers(), params={"query": query, "page": page, "include_adult": "false"})
        out = []
        for x in data.get("results", []):
            if x.get("media_type") not in {"movie", "tv"} or x.get("adult") is not False: continue
            title = x.get("title") or x.get("name") or "Unknown"
            out.append(NormalizedWork("tmdb", f"{x['media_type']}:{x['id']}", x.get("original_title") or x.get("original_name") or title, title, category=x["media_type"], is_adult=False, source_url=f"https://www.themoviedb.org/{x['media_type']}/{x['id']}"))
        return out

    async def characters_for_work(self, work: NormalizedWork, page: int = 1):
        media_type, ident = work.external_id.split(":", 1)
        data = await self.http.json("GET", f"{self.base}/{media_type}/{ident}/credits", cache_key=f"tmdb:credits:{work.external_id}", headers=self._headers())
        results = []
        for index, cast in enumerate(data.get("cast", [])):
            role = (cast.get("character") or "").strip()
            if not role: continue
            # Role plus production identifies this internal candidate; actor IDs never identify characters.
            key = f"{work.external_id}:role:{role.casefold()}:{index}"
            results.append(NormalizedCharacter("tmdb", key, role, role, works=[work], category=work.category, actor_name=cast.get("name"), content_approved=False, metadata={"tmdb_actor_id": cast.get("id"), "image_policy": "placeholder_only"}))
        return results

