import pytest
from sqlalchemy import func,select
from bot.database.models import CatalogImage,Character,CharacterWork,ProviderReference,ReviewStatus,Work
from bot.providers.base import NormalizedCharacter,NormalizedImage,NormalizedWork
from bot.services.catalog import CatalogImportService
from bot.services.collections import CollectionService

@pytest.mark.asyncio
async def test_selected_provider_import_is_idempotent_and_preserves_provenance(db):
    service=CatalogImportService();work=NormalizedWork("mock","w1","Work",category="anime",is_adult=False,source_url="https://source/work",content_approved=True)
    candidate=NormalizedCharacter("mock","c1","Hero",works=[work],images=[NormalizedImage("https://images/hero.png","https://source/image","CC-BY",True)],category="anime",content_approved=True,source_url="https://source/character")
    first=await service.import_candidate(db,candidate);second=await service.import_candidate(db,candidate)
    assert first.id==second.id and first.review_status==ReviewStatus.approved
    assert await db.scalar(select(func.count()).select_from(Character))==1
    assert await db.scalar(select(func.count()).select_from(Work))==1
    assert await db.scalar(select(func.count()).select_from(CharacterWork))==1
    assert await db.scalar(select(func.count()).select_from(ProviderReference))==2
    image=(await db.scalars(select(CatalogImage))).one();assert image.source_url=="https://source/image" and image.license=="CC-BY" and image.review_status==ReviewStatus.approved
    collections=CollectionService();entry=await collections.add(db,123,first.id);assert entry.position==1
    with pytest.raises(ValueError,match="already collected"):await collections.add(db,123,first.id)

@pytest.mark.asyncio
async def test_unapproved_import_stays_pending_and_equal_names_do_not_merge(db):
    service=CatalogImportService()
    one=NormalizedCharacter("mock","1","Same",works=[NormalizedWork("mock","a","A",is_adult=None)],content_approved=False)
    two=NormalizedCharacter("mock","2","Same",works=[NormalizedWork("mock","b","B",is_adult=False)],content_approved=True)
    a=await service.import_candidate(db,one);b=await service.import_candidate(db,two)
    assert a.id!=b.id and a.review_status==ReviewStatus.pending and b.review_status==ReviewStatus.approved
