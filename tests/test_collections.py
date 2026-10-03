import pytest
import asyncio
from sqlalchemy import select
from bot.database.models import Base,Character,CollectionEntry,ReviewStatus
from bot.database.session import Database
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

@pytest.mark.asyncio
async def test_all_reordering_directions_repeated_removals_and_failed_move(db):
    service=CollectionService();await service.user(db,7)
    chars=[Character(name_original=str(i),category="game",review_status=ReviewStatus.approved) for i in range(6)];db.add_all(chars);await db.flush()
    for char in chars:await service.add(db,7,char.id)
    for value,target in [("6",1),("1",6),("3",5),("5",2),("2",4)]:await service.move(db,7,value,target)
    assert list((await db.scalars(select(CollectionEntry.position).where(CollectionEntry.user_id==7).order_by(CollectionEntry.position))).all())==list(range(1,7))
    with pytest.raises(ValueError):await service.move(db,7,"1",99)
    assert list((await db.scalars(select(CollectionEntry.position).where(CollectionEntry.user_id==7).order_by(CollectionEntry.position))).all())==list(range(1,7))
    await service.remove(db,7,"1");await service.remove(db,7,"3");await service.remove(db,7,"4")
    assert list((await db.scalars(select(CollectionEntry.position).where(CollectionEntry.user_id==7).order_by(CollectionEntry.position))).all())==[1,2,3]

@pytest.mark.asyncio
async def test_undo_rejects_readded_or_hidden_character(db):
    service=CollectionService();await service.user(db,8);char=Character(name_original="A",category="game",review_status=ReviewStatus.approved);db.add(char);await db.flush();await service.add(db,8,char.id);await service.remove(db,8,"1");await service.add(db,8,char.id)
    with pytest.raises(ValueError,match="re-added"):await service.undo(db,8)
    await service.remove(db,8,"1");char.review_status=ReviewStatus.hidden;await db.flush()
    with pytest.raises(ValueError,match="no longer available"):await service.undo(db,8)

@pytest.mark.asyncio
async def test_concurrent_sqlite_mutations_serialize_per_owner(tmp_path):
    database=Database(f"sqlite+aiosqlite:///{tmp_path/'concurrent.db'}");service=CollectionService()
    async with database.engine.begin() as connection:await connection.run_sync(Base.metadata.create_all)
    async with database.session() as session:
        await service.user(session,99);chars=[Character(name_original=x,category="game",review_status=ReviewStatus.approved) for x in ("A","B")];session.add_all(chars);await session.flush();ids=[x.id for x in chars]
    async def add(character_id):
        async with database.session() as session:await service.add(session,99,character_id)
    await asyncio.gather(*(add(character_id) for character_id in ids))
    async with database.session() as session:
        assert list((await session.scalars(select(CollectionEntry.position).where(CollectionEntry.user_id==99).order_by(CollectionEntry.position))).all())==[1,2]
    await database.close()

@pytest.mark.asyncio
async def test_failed_resequence_rolls_back_without_corrupting_positions(tmp_path):
    database=Database(f"sqlite+aiosqlite:///{tmp_path/'rollback.db'}");service=CollectionService()
    async with database.engine.begin() as connection:await connection.run_sync(Base.metadata.create_all)
    async with database.session() as session:
        await service.user(session,12);chars=[Character(name_original=x,category="game",review_status=ReviewStatus.approved) for x in ("A","B","C")];session.add_all(chars);await session.flush()
        for char in chars:await service.add(session,12,char.id)
    original=service._resequence
    async def fail_after_temporary(session,entries):
        for entry in entries:entry.position=-(entry.id+1)
        await session.flush();raise RuntimeError("injected failure")
    service._resequence=fail_after_temporary
    with pytest.raises(RuntimeError):
        async with database.session() as session:await service.move(session,12,"3",1)
    service._resequence=original
    async with database.session() as session:assert list((await session.scalars(select(CollectionEntry.position).where(CollectionEntry.user_id==12).order_by(CollectionEntry.position))).all())==[1,2,3]
    await database.close()
