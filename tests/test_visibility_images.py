import pytest
from bot.database.models import Character,PersonalImage,ReviewStatus
from bot.services.collections import CollectionService
from bot.services.images import ImageService

@pytest.mark.asyncio
async def test_pending_custom_is_owner_only_and_hidden_is_not_displayed(db):
    service=CollectionService();await service.user(db,1);await service.user(db,2)
    char=Character(name_original="Private",category="original",review_status=ReviewStatus.pending,is_custom=True,owner_id=1);db.add(char);await db.flush();await service.add(db,1,char.id)
    assert (await service.page(db,1,1,viewer_id=1))[1]==1
    assert (await service.page(db,1,1,viewer_id=2))[1]==0
    char.review_status=ReviewStatus.hidden;await db.flush();assert (await service.page(db,1,1,viewer_id=1))[1]==0

@pytest.mark.asyncio
async def test_selected_personal_image_can_be_deleted_with_foreign_keys(db):
    service=CollectionService();await service.user(db,1);char=Character(name_original="A",category="game",review_status=ReviewStatus.approved);db.add(char);await db.flush();entry=await service.add(db,1,char.id)
    image=PersonalImage(entry_id=entry.id,url="https://example/image.png",review_status=ReviewStatus.approved);db.add(image);await db.flush();entry.selected_personal_image_id=image.id;await db.flush()
    images=ImageService.__new__(ImageService);await images.remove(db,1,"1",1);await db.flush();assert entry.selected_personal_image_id is None
