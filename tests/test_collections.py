import pytest
from sqlalchemy import select
from bot.database.models import Character, CollectionEntry, ReviewStatus
from bot.services.collections import CollectionService

@pytest.mark.asyncio
async def test_add_move_remove_undo_is_transactional_and_contiguous(db):
    service=CollectionService(); await service.user(db,42,"en")
    chars=[Character(name_original=x,category="game",review_status=ReviewStatus.approved) for x in ("A","B","C")]
    db.add_all(chars); await db.flush()
    for char in chars: await service.add(db,42,char.id)
    await service.move(db,42,"3",1)
    rows=list((await db.scalars(select(CollectionEntry).where(CollectionEntry.user_id==42).order_by(CollectionEntry.position))).all())
    assert [r.character_id for r in rows]==[chars[2].id,chars[0].id,chars[1].id]
    removed=await service.remove(db,42,"2")
    assert removed.character_id==chars[0].id
    assert list((await db.scalars(select(CollectionEntry.position).order_by(CollectionEntry.position))).all())==[1,2]
    restored=await service.undo(db,42)
    assert restored.position==2
    assert list((await db.scalars(select(CollectionEntry.position).order_by(CollectionEntry.position))).all())==[1,2,3]

@pytest.mark.asyncio
async def test_duplicate_add_is_rejected(db):
    service=CollectionService(); await service.user(db,1)
    char=Character(name_original="A",category="novel",review_status=ReviewStatus.approved); db.add(char); await db.flush()
    await service.add(db,1,char.id)
    with pytest.raises(ValueError,match="already collected"): await service.add(db,1,char.id)

@pytest.mark.asyncio
async def test_personal_metadata_isolated_by_owner(db):
    service=CollectionService(); char=Character(name_original="A",category="comic",review_status=ReviewStatus.approved); db.add(char); await db.flush()
    for uid in (1,2): await service.user(db,uid); await service.add(db,uid,char.id)
    await service.set_metadata(db,1,"1",nickname="Mine",note="private")
    other=await service.resolve(db,2,"1")
    assert other.nickname is None and other.note is None and char.name_original=="A"

