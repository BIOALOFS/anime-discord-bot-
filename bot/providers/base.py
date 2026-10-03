from __future__ import annotations

from abc import ABC
from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any


class Capability(StrEnum):
    CHARACTER_SEARCH = "character_search"
    WORK_SEARCH = "work_search"
    WORK_CHARACTERS = "work_characters"
    CHARACTER_DETAILS = "character_details"
    IMAGES = "images"
    CLASSIFICATIONS = "classifications"


class ProviderError(RuntimeError):
    pass


class UnsupportedCapability(ProviderError):
    pass


@dataclass(slots=True)
class NormalizedWork:
    provider: str
    external_id: str
    title: str
    title_en: str | None = None
    title_ar: str | None = None
    category: str = "other"
    version: str | None = None
    is_adult: bool | None = None
    rating: str | None = None
    source_url: str | None = None


@dataclass(slots=True)
class NormalizedImage:
    url: str
    source_url: str | None = None
    license: str | None = None
    verified_character_image: bool = False


@dataclass(slots=True)
class NormalizedCharacter:
    provider: str
    external_id: str
    name: str
    name_en: str | None = None
    name_ar: str | None = None
    aliases: list[str] = field(default_factory=list)
    works: list[NormalizedWork] = field(default_factory=list)
    images: list[NormalizedImage] = field(default_factory=list)
    description: str | None = None
    category: str = "other"
    version: str | None = None
    actor_name: str | None = None
    content_approved: bool = False
    source_url: str | None = None
    metadata: dict[str, Any] = field(default_factory=dict)


class CharacterProvider(ABC):
    name: str
    capabilities: frozenset[Capability]

    def require(self, capability: Capability) -> None:
        if capability not in self.capabilities:
            raise UnsupportedCapability(f"{self.name} does not support {capability.value}")

    async def search_characters(self, query: str, page: int = 1) -> list[NormalizedCharacter]:
        self.require(Capability.CHARACTER_SEARCH)
        raise NotImplementedError

    async def search_works(self, query: str, page: int = 1) -> list[NormalizedWork]:
        self.require(Capability.WORK_SEARCH)
        raise NotImplementedError

    async def characters_for_work(self, work: NormalizedWork, page: int = 1) -> list[NormalizedCharacter]:
        self.require(Capability.WORK_CHARACTERS)
        raise NotImplementedError

