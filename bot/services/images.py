from __future__ import annotations
import asyncio,ipaddress,socket
from io import BytesIO
from urllib.parse import urljoin,urlparse
import aiohttp
from PIL import Image
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from bot.database.models import CatalogImage,PersonalImage,ReviewStatus
from .collections import CollectionService

class PublicResolver(aiohttp.abc.AbstractResolver):
    async def resolve(self,host,port=0,family=socket.AF_INET):
        infos=await asyncio.get_running_loop().getaddrinfo(host,port,type=socket.SOCK_STREAM,family=family)
        records=[]
        for family,_,proto,_,address in infos:
            ip=ipaddress.ip_address(address[0])
            if not ip.is_global:raise ValueError("private or unsafe image host")
            records.append({"hostname":host,"host":str(ip),"port":port,"family":family,"proto":proto,"flags":0})
        if not records:raise ValueError("image host did not resolve")
        return records
    async def close(self):pass

class ImageService:
    allowed_types={"image/jpeg","image/png","image/webp","image/gif"}
    def __init__(self,session,maximum=8*1024*1024):self.http=session;self.maximum=maximum
    async def fetch(self,url):
        current=url;connector=aiohttp.TCPConnector(resolver=PublicResolver(),ttl_dns_cache=0)
        async with aiohttp.ClientSession(connector=connector,headers=self.http.headers,timeout=aiohttp.ClientTimeout(total=20,sock_read=5)) as client:
            for _ in range(4):
                parsed=urlparse(current)
                if parsed.scheme!="https" or not parsed.hostname or parsed.username or parsed.password:raise ValueError("a public HTTPS image URL is required")
                async with client.get(current,allow_redirects=False) as response:
                    if response.status in {301,302,303,307,308}:
                        current=urljoin(current,response.headers.get("Location",""));continue
                    if response.status!=200:raise ValueError(f"image returned HTTP {response.status}")
                    content_type=response.headers.get("Content-Type","").split(";")[0].lower()
                    if content_type not in self.allowed_types:raise ValueError("unsupported image type")
                    chunks=[];total=0
                    async for chunk in response.content.iter_chunked(64*1024):
                        total+=len(chunk)
                        if total>self.maximum:raise ValueError("image is too large")
                        chunks.append(chunk)
                    data=b"".join(chunks);await asyncio.to_thread(self._verify,data);return data
        raise ValueError("too many image redirects")
    @staticmethod
    def _verify(data):
        with Image.open(BytesIO(data)) as image:
            width,height=image.size;image.verify()
            if width*height>40_000_000:raise ValueError("image dimensions are too large")
    async def gallery(self,db,entry):
        catalog=list((await db.scalars(select(CatalogImage).where(CatalogImage.character_id==entry.character_id,CatalogImage.review_status==ReviewStatus.approved).order_by(CatalogImage.id))).all())
        personal=list((await db.scalars(select(PersonalImage).where(PersonalImage.entry_id==entry.id,PersonalImage.review_status==ReviewStatus.approved).order_by(PersonalImage.id))).all())
        return [("catalog",x) for x in catalog]+[("personal",x) for x in personal]
    async def add(self,db,user_id,character,url):
        await self.fetch(url);entry=await CollectionService().resolve(db,user_id,character)
        # Validation proves file safety, not content safety. Human/moderation approval is separate.
        image=PersonalImage(entry_id=entry.id,url=url,review_status=ReviewStatus.pending);db.add(image);await db.flush();return image
    async def select(self,db,user_id,character,number):
        entry=await CollectionService().resolve(db,user_id,character);gallery=await self.gallery(db,entry)
        if number<1 or number>len(gallery):raise ValueError("invalid approved image number")
        kind,image=gallery[number-1]
        if kind=="catalog":entry.selected_catalog_image_id=image.id;entry.selected_personal_image_id=None
        else:entry.selected_personal_image_id=image.id;entry.selected_catalog_image_id=None
        return image
    async def remove(self,db,user_id,character,number):
        entry=await CollectionService().resolve(db,user_id,character)
        images=list((await db.scalars(select(PersonalImage).where(PersonalImage.entry_id==entry.id).order_by(PersonalImage.id))).all())
        if number<1 or number>len(images):raise ValueError("invalid personal image number")
        image=images[number-1]
        if entry.selected_personal_image_id==image.id:entry.selected_personal_image_id=None;await db.flush()
        await db.delete(image);return image
    async def reset(self,db,user_id,character):
        entry=await CollectionService().resolve(db,user_id,character);entry.selected_personal_image_id=None;entry.selected_catalog_image_id=None;return entry
