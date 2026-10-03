from bot.providers.registry import ProviderRegistry
from .content import validate_safe_text


class SearchService:
    def __init__(self, providers: ProviderRegistry): self.providers = providers
    async def characters(self, query: str, page: int = 1):
        query = validate_safe_text(query, maximum=200, field="query")
        result = await self.providers.search_characters(query, page)
        # Unknown/pending provider results can be shown with warning but cannot be added until reviewed.
        result.results = [x for x in result.results if x.content_approved]
        return result
