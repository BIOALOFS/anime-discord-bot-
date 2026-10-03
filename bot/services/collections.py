from __future__ import annotations

from sqlalchemy import String, and_, cast, func, or_, select, text
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload
from bot.database.models import Character, CharacterWork, CollectionEntry, PersonalImage, ReviewStatus, UndoRecord, User, Work
from .content import normalize, validate_safe_text


class NotFound(ValueError): pass
class Ambiguous(ValueError):
    def __init__(self,matches): self.matches=matches;super().__init__("multiple characters match; select one")


class CollectionService:
    async def user(self,db:AsyncSession,discord_id:int,language:str="ar") -> User:
        user=await db.get(User,discord_id)
        if not user: user=User(discord_id=discord_id,language=language);db.add(user);await db.flush()
        return user

    async def _lock_owner(self,db,user_id):
        if db.bind and db.bind.dialect.name=="sqlite" and not db.in_transaction():
            await db.execute(text("BEGIN IMMEDIATE"))
        if db.bind and db.bind.dialect.name=="postgresql":
            # Also covers the first mutation before a stable User row exists.
            await db.execute(text("SELECT pg_advisory_xact_lock(:owner)"),{"owner":int(user_id) & 0x7FFF_FFFF_FFFF_FFFF})
        await self.user(db,user_id)
        # PostgreSQL obtains a row lock. SQLite's write transaction serialization plus
        # busy_timeout protects the same critical section across processes.
        await db.scalar(select(User).where(User.discord_id==user_id).with_for_update())

    async def _ordered(self,db,user_id):
        return list((await db.scalars(select(CollectionEntry).where(CollectionEntry.user_id==user_id).order_by(CollectionEntry.position,CollectionEntry.id))).all())

    async def _resequence(self,db,entries):
        # ORM UPDATEs may be emitted in primary-key order. Every row is therefore first
        # moved to a unique negative position before any positive final position exists.
        for entry in entries: entry.position=-(entry.id+1)
        await db.flush()
        for position,entry in enumerate(entries,1): entry.position=position
        await db.flush()

    async def add(self,db,user_id,character_id):
        await self._lock_owner(db,user_id)
        character=await db.get(Character,character_id)
        if not character or character.review_status not in {ReviewStatus.approved,ReviewStatus.pending}: raise NotFound("character is unavailable")
        if character.review_status==ReviewStatus.pending and character.owner_id!=user_id: raise ValueError("character is pending review")
        if await db.scalar(select(CollectionEntry.id).where(CollectionEntry.user_id==user_id,CollectionEntry.character_id==character_id)): raise ValueError("character is already collected")
        position=(await db.scalar(select(func.max(CollectionEntry.position)).where(CollectionEntry.user_id==user_id)) or 0)+1
        entry=CollectionEntry(user_id=user_id,character_id=character_id,position=position);db.add(entry);await db.flush();return entry

    def _base(self,user_id):
        return (select(CollectionEntry).join(Character).options(selectinload(CollectionEntry.character).selectinload(Character.works).selectinload(CharacterWork.work),selectinload(CollectionEntry.personal_images)).where(CollectionEntry.user_id==user_id))

    async def search(self,db,user_id,query,page=1,per_page=25,viewer_id=None):
        term=f"%{normalize(query)}%"
        conditions=[or_(
            func.lower(Character.name_original).like(term),func.lower(func.coalesce(Character.name_en,"")).like(term),func.lower(func.coalesce(Character.name_ar,"")).like(term),
            func.lower(func.coalesce(CollectionEntry.nickname,"")).like(term),func.lower(cast(Character.aliases,String)).like(term),
            and_(Work.review_status==ReviewStatus.approved,or_(func.lower(func.coalesce(Work.title_original,"")).like(term),func.lower(func.coalesce(Work.title_en,"")).like(term),func.lower(func.coalesce(Work.title_ar,"")).like(term)))
        )]
        if viewer_id is not None:conditions.append(or_(Character.review_status==ReviewStatus.approved,(Character.review_status==ReviewStatus.pending)&(Character.owner_id==viewer_id)&(Character.owner_id==user_id)))
        stmt=(self._base(user_id).outerjoin(CharacterWork,CharacterWork.character_id==Character.id).outerjoin(Work,Work.id==CharacterWork.work_id).where(*conditions).distinct().order_by(CollectionEntry.position).offset((page-1)*per_page).limit(per_page))
        return list((await db.scalars(stmt)).unique().all())

    async def resolve(self,db,user_id,value):
        if value.isdigit():
            entry=(await db.scalars(self._base(user_id).where(CollectionEntry.position==int(value)))).first()
            if entry:return entry
        entry=(await db.scalars(self._base(user_id).where(CollectionEntry.character_id==value))).first()
        if entry:return entry
        matches=await self.search(db,user_id,value,1,26)
        exact=normalize(value);exact_matches=[e for e in matches if exact in {normalize(e.character.name_original),normalize(e.character.name_en or ""),normalize(e.character.name_ar or ""),normalize(e.nickname or "")}]
        candidates=exact_matches or matches
        if not candidates: raise NotFound("character not found in collection")
        if len(candidates)>1: raise Ambiguous(candidates[:25])
        return candidates[0]

    async def page(self,db,user_id,position,limit=1,viewer_id=None):
        viewer_id=user_id if viewer_id is None else viewer_id
        visible=or_(Character.review_status==ReviewStatus.approved,(Character.review_status==ReviewStatus.pending) & (Character.owner_id==viewer_id) & (Character.owner_id==user_id))
        total=await db.scalar(select(func.count()).select_from(CollectionEntry).join(Character).where(CollectionEntry.user_id==user_id,visible)) or 0
        rows=list((await db.scalars(self._base(user_id).where(visible).order_by(CollectionEntry.position).offset(max(0,position-1)).limit(limit))).unique().all())
        return rows,total

    async def remove(self,db,user_id,value):
        await self._lock_owner(db,user_id);entry=await self.resolve(db,user_id,value);await db.refresh(entry,["personal_images"])
        payload={"character_id":entry.character_id,"position":entry.position,"nickname":entry.nickname,"note":entry.note,"images":[{"url":i.url,"source":i.source,"review_status":i.review_status.value} for i in entry.personal_images],"selected_personal_url":next((i.url for i in entry.personal_images if i.id==entry.selected_personal_image_id),None),"selected_catalog_image_id":entry.selected_catalog_image_id}
        db.add(UndoRecord(user_id=user_id,payload=payload));entry.selected_personal_image_id=None;entry.selected_catalog_image_id=None;await db.flush();await db.delete(entry);await db.flush()
        await self._resequence(db,await self._ordered(db,user_id));return entry

    async def move(self,db,user_id,value,target):
        await self._lock_owner(db,user_id);entry=await self.resolve(db,user_id,value);entries=await self._ordered(db,user_id)
        if target<1 or target>len(entries): raise ValueError(f"position must be between 1 and {len(entries)}")
        entries.remove(entry);entries.insert(target-1,entry);await self._resequence(db,entries);return entry

    async def set_metadata(self,db,user_id,value,*,nickname=None,note=None,reset=None):
        entry=await self.resolve(db,user_id,value)
        if reset=="nickname":entry.nickname=None
        elif reset=="note":entry.note=None
        elif nickname is not None:entry.nickname=validate_safe_text(nickname,maximum=100,field="nickname")
        elif note is not None:entry.note=validate_safe_text(note,maximum=1000,field="note")
        return entry

    async def undo(self,db,user_id):
        await self._lock_owner(db,user_id)
        record=(await db.scalars(select(UndoRecord).where(UndoRecord.user_id==user_id).order_by(UndoRecord.created_at.desc(),UndoRecord.id.desc()).limit(1))).first()
        if not record:raise NotFound("nothing to undo")
        p=record.payload;character=await db.get(Character,p["character_id"])
        if not character or character.review_status in {ReviewStatus.hidden,ReviewStatus.rejected}:raise ValueError("removed character is no longer available")
        if await db.scalar(select(CollectionEntry.id).where(CollectionEntry.user_id==user_id,CollectionEntry.character_id==p["character_id"])):raise ValueError("character has already been re-added")
        entries=await self._ordered(db,user_id);entry=CollectionEntry(user_id=user_id,character_id=p["character_id"],position=-(len(entries)+1000000),nickname=p.get("nickname"),note=p.get("note"),selected_catalog_image_id=p.get("selected_catalog_image_id"));db.add(entry);await db.flush()
        selected=None
        for saved in p.get("images",[]):
            if isinstance(saved,str): saved={"url":saved,"source":"user","review_status":"pending"}
            image=PersonalImage(entry_id=entry.id,url=saved["url"],source=saved.get("source","user"),review_status=ReviewStatus(saved.get("review_status","pending")));db.add(image);await db.flush()
            if image.url==p.get("selected_personal_url") or image.url==p.get("selected_url"):selected=image.id
        entry.selected_personal_image_id=selected;entries.insert(min(max(0,p["position"]-1),len(entries)),entry);await self._resequence(db,entries);await db.delete(record);return entry
