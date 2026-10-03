from __future__ import annotations
import time
from dataclasses import dataclass
import discord
from sqlalchemy.ext.asyncio import AsyncSession
from bot.database.models import ReviewStatus, User
from .collections import CollectionService

PLACEHOLDER="https://placehold.co/600x900/5865F2/FFFFFF.png?text=Character"
DEFAULT_BANNER="https://placehold.co/1200x300/23272A/FFFFFF.png?text=Anpot"

@dataclass(slots=True)
class ProfileCard:
    embed: discord.Embed
    total: int
    position: int
    image_index: int
    image_count: int

class ProfileRenderer:
    def __init__(self,bot): self.bot=bot;self.collections=CollectionService();self._users={}
    async def _discord_user(self,user):
        cached=self._users.get(user.id)
        if cached and cached[0]>time.monotonic():return cached[1]
        result=user
        if getattr(user,"banner",None) is None:
            try: result=await self.bot.fetch_user(user.id)
            except discord.HTTPException: pass
        self._users[user.id]=(time.monotonic()+600,result);return result
    async def render(self,db:AsyncSession,user,viewer_id:int,position:int,image_index:int=0) -> ProfileCard:
        settings=await db.get(User,user.id);rows,total=await self.collections.page(db,user.id,position,viewer_id=viewer_id)
        if not rows:return ProfileCard(discord.Embed(title="Empty collection / مجموعة فارغة"),0,0,0,0)
        entry=rows[0];character=entry.character;discord_user=await self._discord_user(user)
        catalog=[x for x in sorted(character.images,key=lambda x:x.id) if x.review_status==ReviewStatus.approved]
        personal=[x for x in sorted(entry.personal_images,key=lambda x:x.id) if x.review_status==ReviewStatus.approved]
        gallery=[("catalog",x.id,x.url) for x in catalog]+[("personal",x.id,x.url) for x in personal]
        selected=next((i for i,x in enumerate(gallery) if (x[0]=="personal" and x[1]==entry.selected_personal_image_id) or (x[0]=="catalog" and x[1]==entry.selected_catalog_image_id)),0)
        if image_index<0:image_index=len(gallery)-1 if gallery else 0
        elif image_index>=len(gallery):image_index=0
        elif image_index==0:selected=max(0,min(selected,len(gallery)-1))
        else:selected=image_index
        image_index=selected;image_url=gallery[image_index][2] if gallery else PLACEHOLDER
        color="#5865F2" if not settings else settings.color
        try:embed_color=discord.Color.from_str(color)
        except ValueError:embed_color=discord.Color.blurple()
        title=f"{entry.nickname} — {character.name_original}" if entry.nickname else character.name_original
        profile_bits=[x for x in ((settings.title if settings else None),(settings.bio if settings else None)) if x]
        description="\n".join(profile_bits+[entry.note or character.description or ""])
        embed=discord.Embed(title=title,description=description[:4096],color=embed_color)
        avatar=settings.avatar_url if settings and settings.avatar_url and settings.avatar_review_status==ReviewStatus.approved else discord_user.display_avatar.url
        banner=settings.banner_url if settings and settings.banner_url and settings.banner_review_status==ReviewStatus.approved else (discord_user.banner.url if getattr(discord_user,"banner",None) else DEFAULT_BANNER)
        embed.set_author(name=user.display_name,icon_url=avatar);embed.set_thumbnail(url=banner);embed.set_image(url=image_url)
        works=", ".join(link.work.title_original for link in character.works if link.work.review_status==ReviewStatus.approved) or "—"
        embed.add_field(name="Work / العمل",value=works[:1024]);embed.add_field(name="Category / الفئة",value=character.category)
        if character.review_status==ReviewStatus.pending:embed.add_field(name="Visibility / الظهور",value="Owner-only pending review / للمالك فقط بانتظار المراجعة",inline=False)
        embed.set_footer(text=f"Character {position} / {total} — Image {image_index+1 if gallery else 0} / {len(gallery)}")
        return ProfileCard(embed,total,position,image_index,len(gallery))
