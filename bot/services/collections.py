from __future__ import annotations
from sqlalchemy import case, delete, func, or_, select, update
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload
from bot.database.models import Character, CollectionEntry, PersonalImage, ReviewStatus, UndoRecord, User
from .content import normalize, validate_safe_text


class NotFound(ValueError): pass
class Ambiguous(ValueError):
    def __init__(self, matches): self.matches = matches; super().__init__("multiple characters match; select one")


class CollectionService:
    async def user(self, db: AsyncSession, discord_id: int, language: str = "ar") -> User:
        user = await db.get(User, discord_id)
        if not user:
            user = User(discord_id=discord_id, language=language); db.add(user); await db.flush()
        return user

    async def add(self, db: AsyncSession, user_id: int, character_id: str) -> CollectionEntry:
        character = await db.get(Character, character_id)
        if not character or character.review_status not in {ReviewStatus.approved, ReviewStatus.pending}:
            raise NotFound("character is unavailable")
        if character.review_status == ReviewStatus.pending and character.owner_id != user_id:
            raise ValueError("character is pending review")
        await self.user(db, user_id)
        if await db.scalar(select(CollectionEntry.id).where(CollectionEntry.user_id == user_id, CollectionEntry.character_id == character_id)):
            raise ValueError("character is already collected")
        position = (await db.scalar(select(func.max(CollectionEntry.position)).where(CollectionEntry.user_id == user_id)) or 0) + 1
        entry = CollectionEntry(user_id=user_id, character_id=character_id, position=position); db.add(entry); await db.flush()
        return entry

    async def resolve(self, db: AsyncSession, user_id: int, value: str) -> CollectionEntry:
        query = select(CollectionEntry).join(Character).options(selectinload(CollectionEntry.character)).where(CollectionEntry.user_id == user_id)
        if value.isdigit():
            by_position = (await db.scalars(query.where(CollectionEntry.position == int(value)))).first()
            if by_position: return by_position
        exact = normalize(value)
        entries = list((await db.scalars(query.order_by(CollectionEntry.position))).all())
        matches = [e for e in entries if exact in {normalize(e.character.name_original), normalize(e.character.name_en or ""), normalize(e.character.name_ar or ""), normalize(e.nickname or ""), e.character_id.casefold()}]
        if not matches:
            matches = [e for e in entries if exact and any(exact in normalize(x) for x in [e.character.name_original, e.character.name_en or "", e.character.name_ar or "", e.nickname or "", *e.character.aliases])]
        if not matches: raise NotFound("character not found in collection")
        if len(matches) > 1: raise Ambiguous(matches[:25])
        return matches[0]

    async def page(self, db: AsyncSession, user_id: int, position: int, limit: int = 1):
        total = await db.scalar(select(func.count()).select_from(CollectionEntry).where(CollectionEntry.user_id == user_id)) or 0
        rows = list((await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.character).selectinload(Character.images), selectinload(CollectionEntry.personal_images)).where(CollectionEntry.user_id == user_id, CollectionEntry.position >= position).order_by(CollectionEntry.position).limit(limit))).all())
        return rows, total

    async def remove(self, db: AsyncSession, user_id: int, value: str) -> CollectionEntry:
        entry = await self.resolve(db, user_id, value)
        await db.refresh(entry, ["personal_images"])
        payload = {"character_id":entry.character_id,"position":entry.position,"nickname":entry.nickname,"note":entry.note,"images":[i.url for i in entry.personal_images],"selected_url":next((i.url for i in entry.personal_images if i.id == entry.selected_personal_image_id),None)}
        db.add(UndoRecord(user_id=user_id, payload=payload))
        pos = entry.position
        await db.delete(entry); await db.flush()
        await db.execute(update(CollectionEntry).where(CollectionEntry.user_id == user_id, CollectionEntry.position > pos).values(position=CollectionEntry.position - 1))
        return entry

    async def move(self, db: AsyncSession, user_id: int, value: str, target: int) -> CollectionEntry:
        entry = await self.resolve(db, user_id, value)
        count = await db.scalar(select(func.count()).select_from(CollectionEntry).where(CollectionEntry.user_id == user_id)) or 0
        if target < 1 or target > count: raise ValueError(f"position must be between 1 and {count}")
        old = entry.position
        if old == target: return entry
        # Temporarily vacate the unique position; one CASE update keeps other positions contiguous.
        entry.position = -1; await db.flush()
        if target < old:
            await db.execute(update(CollectionEntry).where(CollectionEntry.user_id == user_id, CollectionEntry.position >= target, CollectionEntry.position < old).values(position=CollectionEntry.position + 1))
        else:
            await db.execute(update(CollectionEntry).where(CollectionEntry.user_id == user_id, CollectionEntry.position > old, CollectionEntry.position <= target).values(position=CollectionEntry.position - 1))
        entry.position = target; await db.flush(); return entry

    async def set_metadata(self, db: AsyncSession, user_id: int, value: str, *, nickname: str | None = None, note: str | None = None, reset: str | None = None):
        entry = await self.resolve(db, user_id, value)
        if reset == "nickname": entry.nickname = None
        elif reset == "note": entry.note = None
        elif nickname is not None: entry.nickname = validate_safe_text(nickname, maximum=100, field="nickname")
        elif note is not None: entry.note = validate_safe_text(note, maximum=1000, field="note")
        return entry

    async def undo(self, db: AsyncSession, user_id: int):
        record = (await db.scalars(select(UndoRecord).where(UndoRecord.user_id == user_id).order_by(UndoRecord.created_at.desc(), UndoRecord.id.desc()).limit(1))).first()
        if not record: raise NotFound("nothing to undo")
        p = record.payload
        count = await db.scalar(select(func.count()).select_from(CollectionEntry).where(CollectionEntry.user_id == user_id)) or 0
        position = min(max(1, p["position"]), count + 1)
        await db.execute(update(CollectionEntry).where(CollectionEntry.user_id == user_id, CollectionEntry.position >= position).values(position=CollectionEntry.position + 1))
        entry = CollectionEntry(user_id=user_id, character_id=p["character_id"], position=position, nickname=p.get("nickname"), note=p.get("note")); db.add(entry); await db.flush()
        selected = None
        for url in p.get("images", []):
            image = PersonalImage(entry_id=entry.id, url=url); db.add(image); await db.flush()
            if url == p.get("selected_url"): selected = image.id
        entry.selected_personal_image_id = selected
        await db.delete(record); return entry

