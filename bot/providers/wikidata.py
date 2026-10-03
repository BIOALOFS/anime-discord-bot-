from __future__ import annotations
from .base import Capability, CharacterProvider, NormalizedCharacter, NormalizedImage, ProviderError
from .http import CachedHTTP


class WikidataProvider(CharacterProvider):
    name = "wikidata"
    capabilities = frozenset({Capability.CHARACTER_SEARCH})
    api = "https://www.wikidata.org/w/api.php"

    def __init__(self, http: CachedHTTP): self.http = http

    async def search_characters(self, query: str, page: int = 1):
        # Entity search is bounded; candidates remain pending until P31 claims are checked.
        data = await self.http.json("GET", self.api, cache_key=f"wd:search:{query}:{page}", params={"action":"wbsearchentities","search":query,"language":"en","uselang":"en","type":"item","limit":20,"continue":max(0,(page-1)*20),"format":"json","origin":"*"})
        ids = [x["id"] for x in data.get("search", [])]
        if not ids: return []
        entities = await self.http.json("GET", self.api, cache_key=f"wd:entities:{','.join(ids)}", params={"action":"wbgetentities","ids":"|".join(ids),"props":"labels|aliases|descriptions|claims|sitelinks","languages":"en|ar","format":"json","origin":"*"})
        out = []
        for qid, entity in entities.get("entities", {}).items():
            claims = entity.get("claims", {})
            instance_ids = {c.get("mainsnak",{}).get("datavalue",{}).get("value",{}).get("id") for c in claims.get("P31", [])}
            # Q95074 fictional character; subclasses require human review rather than unsafe broad SPARQL.
            if "Q95074" not in instance_ids: continue
            labels = entity.get("labels", {}); aliases = entity.get("aliases", {})
            en = labels.get("en",{}).get("value"); ar = labels.get("ar",{}).get("value")
            commons = claims.get("P18", []); images=[]
            for claim in commons[:5]:
                filename = claim.get("mainsnak",{}).get("datavalue",{}).get("value")
                if filename:
                    encoded = filename.replace(" ", "_")
                    images.append(NormalizedImage(f"https://commons.wikimedia.org/wiki/Special:Redirect/file/{encoded}", f"https://commons.wikimedia.org/wiki/File:{encoded}", license="See Wikimedia Commons file page", verified_character_image=False))
            out.append(NormalizedCharacter("wikidata", qid, ar or en or qid, en, ar, [a["value"] for values in aliases.values() for a in values][:30], images=images, description=(entity.get("descriptions",{}).get("en",{}).get("value")), content_approved=False, source_url=f"https://www.wikidata.org/wiki/{qid}"))
        return out
