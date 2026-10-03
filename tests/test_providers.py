import pytest
from bot.providers.base import Capability, CharacterProvider, NormalizedCharacter, UnsupportedCapability
from bot.providers.registry import ProviderRegistry

class Good(CharacterProvider):
    name="good"; capabilities=frozenset({Capability.CHARACTER_SEARCH})
    async def search_characters(self,query,page=1): return [NormalizedCharacter("good","1","Hero",content_approved=True)]
class Bad(CharacterProvider):
    name="bad"; capabilities=frozenset({Capability.CHARACTER_SEARCH})
    async def search_characters(self,query,page=1): raise RuntimeError("offline")
class Unsupported(CharacterProvider): name="tmdb"; capabilities=frozenset()

@pytest.mark.asyncio
async def test_partial_provider_failure_preserves_results():
    result=await ProviderRegistry([Good(),Bad(),Unsupported()]).search_characters("hero")
    assert [x.name for x in result.results]==["Hero"]
    assert any("unavailable" in x for x in result.warnings)
    assert any("unsupported" in x for x in result.warnings)

