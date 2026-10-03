from __future__ import annotations
from html import unescape
import re
from .base import Capability, CharacterProvider, NormalizedCharacter, NormalizedImage, NormalizedWork, ProviderError
from .http import CachedHTTP

API = "https://graphql.anilist.co"
CHAR_QUERY = """query($q:String!,$page:Int!){Page(page:$page,perPage:20){characters(search:$q,sort:SEARCH_MATCH){id name{full native alternative} image{large} description(asHtml:false) media(perPage:10){nodes{id type format isAdult title{romaji english native} siteUrl}}}}}"""
WORK_QUERY = """query($q:String!,$page:Int!){Page(page:$page,perPage:20){media(search:$q,sort:SEARCH_MATCH,isAdult:false){id type format isAdult title{romaji english native} siteUrl}}}"""
WORK_CHARS = """query($id:Int!,$page:Int!){Media(id:$id,isAdult:false){characters(page:$page,perPage:25){nodes{id name{full native alternative} image{large} description(asHtml:false)}}}}"""


class AniListProvider(CharacterProvider):
    name = "anilist"
    capabilities = frozenset(Capability)

    def __init__(self, http: CachedHTTP): self.http = http

    async def _query(self, query: str, variables: dict):
        data = await self.http.json("POST", API, cache_key=f"al:{query[:15]}:{variables}", json={"query": query, "variables": variables})
        if data.get("errors"): raise ProviderError(data["errors"][0].get("message", "AniList query failed"))
        return data["data"]

    @staticmethod
    def _work(node: dict) -> NormalizedWork:
        title = node.get("title") or {}
        return NormalizedWork("anilist", str(node["id"]), title.get("native") or title.get("romaji") or title.get("english") or "Unknown", title.get("english"), None, (node.get("format") or node.get("type") or "other").lower(), is_adult=node.get("isAdult"), source_url=node.get("siteUrl"))

    @classmethod
    def _character(cls, node: dict, works: list[NormalizedWork] | None = None) -> NormalizedCharacter:
        names = node.get("name") or {}; media = (node.get("media") or {}).get("nodes", [])
        safe_works = [cls._work(x) for x in media if x.get("isAdult") is False] if works is None else works
        # Unknown classification is deliberately pending, even when no associated adult work was returned.
        approved = bool(safe_works) and all(w.is_adult is False for w in safe_works)
        desc = re.sub(r"<[^>]+>", "", unescape(node.get("description") or ""))[:2000] or None
        image = (node.get("image") or {}).get("large")
        return NormalizedCharacter("anilist", str(node["id"]), names.get("native") or names.get("full") or "Unknown", names.get("full"), aliases=names.get("alternative") or [], works=safe_works, images=[NormalizedImage(image, verified_character_image=True)] if image else [], description=desc, category=(safe_works[0].category if safe_works else "anime_manga"), content_approved=approved, source_url=f"https://anilist.co/character/{node['id']}")

    async def search_characters(self, query: str, page: int = 1):
        nodes = (await self._query(CHAR_QUERY, {"q": query, "page": page}))["Page"]["characters"]
        return [self._character(n) for n in nodes if any(not x.get("isAdult", True) for x in (n.get("media") or {}).get("nodes", []))]

    async def search_works(self, query: str, page: int = 1):
        return [self._work(x) for x in (await self._query(WORK_QUERY, {"q": query, "page": page}))["Page"]["media"]]

    async def characters_for_work(self, work: NormalizedWork, page: int = 1):
        nodes = (await self._query(WORK_CHARS, {"id": int(work.external_id), "page": page}))["Media"]["characters"]["nodes"]
        return [self._character(n, [work]) for n in nodes]

