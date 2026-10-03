import asyncio
from dataclasses import dataclass
from .base import CharacterProvider, NormalizedCharacter, ProviderError, UnsupportedCapability


@dataclass(slots=True)
class SearchOutcome:
    results: list[NormalizedCharacter]
    warnings: list[str]


class ProviderRegistry:
    def __init__(self, providers: list[CharacterProvider]): self.providers = providers

    async def search_characters(self, query: str, page: int = 1) -> SearchOutcome:
        calls = [p.search_characters(query, page) for p in self.providers]
        values = await asyncio.gather(*calls, return_exceptions=True)
        results, warnings = [], []
        for provider, value in zip(self.providers, values):
            if isinstance(value, UnsupportedCapability):
                warnings.append(f"{provider.name}: global character search unsupported")
            elif isinstance(value, Exception):
                warnings.append(f"{provider.name}: unavailable ({value})")
            else: results.extend(value)
        return SearchOutcome(results, warnings)
