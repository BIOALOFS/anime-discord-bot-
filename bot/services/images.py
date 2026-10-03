from __future__ import annotations
import asyncio, ipaddress, socket
from io import BytesIO
from urllib.parse import urljoin, urlparse
import aiohttp
from PIL import Image
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from bot.database.models import PersonalImage
from .collections import CollectionService


class ImageService:
    allowed_types = {"image/jpeg", "image/png", "image/webp", "image/gif"}
    def __init__(self, session: aiohttp.ClientSession, maximum: int = 8 * 1024 * 1024): self.http, self.maximum = session, maximum

    async def _public(self, hostname: str) -> None:
        infos = await asyncio.get_running_loop().getaddrinfo(hostname, None, type=socket.SOCK_STREAM)
        if not infos or any(not ipaddress.ip_address(info[4][0]).is_global for info in infos): raise ValueError("private or unsafe image host")

    async def fetch(self, url: str) -> bytes:
        current = url
        for _ in range(4):
            parsed = urlparse(current)
            if parsed.scheme != "https" or not parsed.hostname or parsed.username: raise ValueError("a public HTTPS image URL is required")
            await self._public(parsed.hostname)
            async with self.http.get(current, allow_redirects=False, timeout=aiohttp.ClientTimeout(total=15)) as response:
                if response.status in {301,302,303,307,308}:
                    current = urljoin(current, response.headers.get("Location", "")); continue
                if response.status != 200: raise ValueError(f"image returned HTTP {response.status}")
                content_type = response.headers.get("Content-Type", "").split(";")[0]
                if content_type not in self.allowed_types: raise ValueError("unsupported image type")
                data = await response.content.read(self.maximum + 1)
                if len(data) > self.maximum: raise ValueError("image is too large")
                await asyncio.to_thread(self._verify, data); return data
        raise ValueError("too many image redirects")

    @staticmethod
    def _verify(data: bytes) -> None:
        with Image.open(BytesIO(data)) as image:
            image.verify()
            if image.width * image.height > 40_000_000: raise ValueError("image dimensions are too large")

    async def add(self, db: AsyncSession, user_id: int, character: str, url: str):
        await self.fetch(url)
        entry = await CollectionService().resolve(db, user_id, character)
        image = PersonalImage(entry_id=entry.id, url=url); db.add(image); await db.flush(); entry.selected_personal_image_id = image.id
        return image

    async def select(self, db: AsyncSession, user_id: int, character: str, number: int):
        entry = await CollectionService().resolve(db, user_id, character)
        images = list((await db.scalars(select(PersonalImage).where(PersonalImage.entry_id == entry.id).order_by(PersonalImage.id))).all())
        if number < 1 or number > len(images): raise ValueError("invalid personal image number")
        entry.selected_personal_image_id = images[number-1].id; return images[number-1]

    async def remove(self, db: AsyncSession, user_id: int, character: str, number: int):
        entry = await CollectionService().resolve(db, user_id, character)
        images = list((await db.scalars(select(PersonalImage).where(PersonalImage.entry_id == entry.id).order_by(PersonalImage.id))).all())
        if number < 1 or number > len(images): raise ValueError("invalid personal image number")
        image = images[number-1]
        if entry.selected_personal_image_id == image.id: entry.selected_personal_image_id = next((x.id for x in images if x.id != image.id), None)
        await db.delete(image); return image

    async def reset(self, db: AsyncSession, user_id: int, character: str):
        entry = await CollectionService().resolve(db, user_id, character); entry.selected_personal_image_id = None; return entry

