import pytest
from types import SimpleNamespace
from sqlalchemy import select
from bot.database.models import Character, CollectionEntry, ReviewStatus, ServerAuditLog
from bot.services.collections import CollectionService
from bot.services.servers import ServerConfigurationService


@pytest.mark.asyncio
async def test_configuration_persists_audits_and_does_not_touch_collection(db):
    collections=CollectionService();servers=ServerConfigurationService()
    await collections.user(db,1);character=Character(name_original="Hero",category="game",review_status=ReviewStatus.approved);db.add(character);await db.flush();entry=await collections.add(db,1,character.id)
    await servers.replace(db,100,200,9);await servers.add(db,100,201,9);await servers.remove(db,100,200,9)
    config=await servers.get(db,100)
    assert config.setup_complete and not config.all_channels and [x.channel_id for x in config.channels]==[201]
    assert (await db.get(CollectionEntry,entry.id)).character_id==character.id
    assert len(list((await db.scalars(select(ServerAuditLog))).all()))==3


@pytest.mark.asyncio
async def test_final_channel_cannot_be_removed_and_all_mode_is_explicit(db):
    service=ServerConfigurationService();await service.replace(db,100,200,9)
    with pytest.raises(ValueError,match="final allowed"): await service.remove(db,100,200,9)
    await service.enable_all(db,100,9);config=await service.get(db,100)
    assert config.setup_complete and config.all_channels
    with pytest.raises(ValueError,match="confirmation"): await service.add(db,100,201,9)
    await service.switch_to_restricted(db,100,201,9);assert not (await service.get(db,100)).all_channels


@pytest.mark.asyncio
async def test_deleted_final_channel_requires_setup_and_never_enables_all(db):
    service=ServerConfigurationService();await service.replace(db,100,200,9)
    guild=SimpleNamespace(id=100,get_channel=lambda channel_id: None)
    channel=SimpleNamespace(id=200,parent=None)
    access=await service.access(db,guild,channel)
    assert not access.allowed and not access.setup_complete and not access.all_channels
    config=await service.get(db,100);assert not config.setup_complete and not config.all_channels
