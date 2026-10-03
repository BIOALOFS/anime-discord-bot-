from types import SimpleNamespace
import pytest
from sqlalchemy import select
from bot.commands.maintainer import MaintainerCog
from bot.database.models import Base,CatalogImage,Character,CollectionEntry,PersonalImage,ReviewStatus
from bot.database.session import Database
from bot.services.collections import CollectionService

@pytest.mark.asyncio
async def test_duplicate_merge_preserves_collection_metadata_images_and_positions(tmp_path):
    database=Database(f"sqlite+aiosqlite:///{tmp_path/'merge.db'}");service=CollectionService()
    async with database.engine.begin() as connection:await connection.run_sync(Base.metadata.create_all)
    async with database.session() as db:
        await service.user(db,1);source=Character(name_original="Duplicate",category="game",review_status=ReviewStatus.approved);target=Character(name_original="Canonical",category="game",review_status=ReviewStatus.approved);other=Character(name_original="Other",category="game",review_status=ReviewStatus.approved);db.add_all([source,target,other]);await db.flush()
        first=await service.add(db,1,source.id);first.nickname="Nick";personal=PersonalImage(entry_id=first.id,url="https://image/personal",review_status=ReviewStatus.approved);catalog=CatalogImage(character_id=source.id,url="https://image/catalog",review_status=ReviewStatus.approved);db.add_all([personal,catalog]);await db.flush();first.selected_personal_image_id=personal.id
        await service.add(db,1,target.id);await service.add(db,1,other.id);ids=(source.id,target.id)
    sent=[];ctx=SimpleNamespace(author=SimpleNamespace(id=99),send=lambda message:sent.append(message))
    async def send(message):sent.append(message)
    ctx.send=send;cog=MaintainerCog(SimpleNamespace(db=database))
    await cog.mergecharacters.callback(cog,ctx,*ids)
    async with database.session() as db:
        entries=list((await db.scalars(select(CollectionEntry).where(CollectionEntry.user_id==1).order_by(CollectionEntry.position))).all())
        assert [x.position for x in entries]==[1,2] and {x.character_id for x in entries}=={ids[1],other.id}
        merged=next(x for x in entries if x.character_id==ids[1]);assert merged.nickname=="Nick" and merged.selected_personal_image_id is not None
        assert (await db.get(CatalogImage,catalog.id)).character_id==ids[1]
    await database.close()
