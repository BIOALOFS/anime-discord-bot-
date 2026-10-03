from __future__ import annotations

from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload
from bot.database.models import CatalogImage, Character, CharacterWork, ProviderReference, ReviewStatus, Work
from bot.providers.base import NormalizedCharacter


class CatalogImportService:
    """Imports only a selected provider candidate and preserves its provenance."""

    async def import_candidate(self,db:AsyncSession,candidate:NormalizedCharacter) -> Character:
        existing=(await db.scalars(select(Character).join(ProviderReference).options(selectinload(Character.works),selectinload(Character.images)).where(ProviderReference.provider==candidate.provider,ProviderReference.external_id==candidate.external_id,ProviderReference.character_id.is_not(None)))).first()
        status=ReviewStatus.approved if candidate.content_approved else ReviewStatus.pending
        if existing:
            # Never let a later provider response silently promote or unhide content.
            existing.name_original=candidate.name or existing.name_original
            existing.name_en=candidate.name_en or existing.name_en;existing.name_ar=candidate.name_ar or existing.name_ar
            existing.aliases=list(dict.fromkeys([*existing.aliases,*candidate.aliases]))
            existing.description=candidate.description or existing.description
            character=existing
            linked={link.work_id for link in character.works};known={image.url for image in character.images}
        else:
            character=Character(name_original=candidate.name,name_en=candidate.name_en,name_ar=candidate.name_ar,aliases=candidate.aliases,category=candidate.category,version=candidate.version,description=candidate.description,review_status=status)
            db.add(character);await db.flush();db.add(ProviderReference(character_id=character.id,provider=candidate.provider,external_id=candidate.external_id,source_url=candidate.source_url))
            linked=set();known=set()
        for normalized in candidate.works:
            work=(await db.scalars(select(Work).join(ProviderReference).where(ProviderReference.provider==normalized.provider,ProviderReference.external_id==normalized.external_id,ProviderReference.work_id.is_not(None)))).first()
            if not work:
                work=Work(title_original=normalized.title,title_en=normalized.title_en,title_ar=normalized.title_ar,category=normalized.category,version=normalized.version,review_status=ReviewStatus.approved if candidate.content_approved and normalized.content_approved else ReviewStatus.pending)
                db.add(work);await db.flush();db.add(ProviderReference(work_id=work.id,provider=normalized.provider,external_id=normalized.external_id,source_url=normalized.source_url))
            if work.id not in linked:
                db.add(CharacterWork(character_id=character.id,work_id=work.id,actor_name=candidate.actor_name));linked.add(work.id)
        for normalized in candidate.images:
            if normalized.url not in known:
                db.add(CatalogImage(character_id=character.id,url=normalized.url,source_url=normalized.source_url,license=normalized.license,is_default=not known,review_status=ReviewStatus.approved if candidate.content_approved and normalized.verified_character_image else ReviewStatus.pending));known.add(normalized.url)
        await db.flush();return character
