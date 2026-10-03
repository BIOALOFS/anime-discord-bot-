from __future__ import annotations
import discord
from discord import app_commands
from discord.ext import commands
from sqlalchemy import select
from bot.database.models import Character, CollectionEntry, CustomSubmission, ReviewStatus, Work, CharacterWork
from bot.services.collections import CollectionService
from bot.services.content import validate_safe_text


class ExtrasCog(commands.Cog):
    def __init__(self,bot): self.bot=bot; self.collections=CollectionService()
    async def _search(self,ctx,query,images=False):
        outcome=await self.bot.search.characters(query)
        lines=[]
        for x in outcome.results[:20]:
            works=", ".join(w.title for w in x.works[:2]) or "—"
            lines.append(f"**{x.name}** (`{x.provider}:{x.external_id}`) — {works}")
        warning=("\n⚠️ "+"; ".join(outcome.warnings)) if outcome.warnings else ""
        embed=discord.Embed(title="Character search / بحث الشخصيات",description="\n".join(lines) or "No approved results / لا توجد نتائج معتمدة")
        if images and outcome.results and outcome.results[0].images: embed.set_thumbnail(url=outcome.results[0].images[0].url)
        await ctx.send(embed=embed,content=warning or None,allowed_mentions=discord.AllowedMentions.none())
    @commands.hybrid_command(name="p",description="Text-only character search")
    async def p(self,ctx,*,query:str): await self._search(ctx,query,False)
    @commands.hybrid_command(name="sp",description="Character search with images")
    async def sp(self,ctx,*,query:str): await self._search(ctx,query,True)

    async def _work_search(self,ctx,query,images=False):
        outputs=[]; warnings=[]
        for provider in self.bot.providers.providers:
            try:
                works=await provider.search_works(query)
                for work in works[:5]:
                    chars=await provider.characters_for_work(work)
                    outputs.extend(f"**{c.name}** — {work.title} ({work.category})" for c in chars[:10])
            except Exception as exc: warnings.append(f"{provider.name}: {exc}")
        await ctx.send(("\n".join(outputs[:30]) or "No results / لا نتائج")+("\n⚠️ "+"; ".join(warnings) if warnings else ""),allowed_mentions=discord.AllowedMentions.none())
    @commands.hybrid_command(name="sa",description="Text-only characters-from-work search")
    async def sa(self,ctx,*,work:str): await self._work_search(ctx,work,False)
    @commands.hybrid_command(name="sap",description="Characters-from-work search with images")
    async def sap(self,ctx,*,work:str): await self._work_search(ctx,work,True)

    async def _add_image(self,ctx,character,url,attachment):
        if (url is None)==(attachment is None): raise ValueError("provide exactly one URL or attachment")
        source=url or attachment.url
        async with self.bot.db.session() as db: await self.bot.images.add(db,ctx.author.id,character,source)
        await ctx.send("Image saved / تم حفظ الصورة")
    @commands.command(name="pi")
    async def pi(self,ctx,*,argument:str=""):
        parts=argument.rsplit(maxsplit=1); url=parts[-1] if parts and parts[-1].startswith("https://") else None; character=parts[0] if url and len(parts)>1 else argument
        attachment=ctx.message.attachments[0] if ctx.message.attachments else None
        await self._add_image(ctx,character,url,attachment)
    @commands.hybrid_command(name="images",description="List personal character images")
    async def images(self,ctx,*,character:str):
        async with self.bot.db.session() as db: e=await self.collections.resolve(db,ctx.author.id,character); await db.refresh(e,["personal_images"]); await ctx.send("\n".join(f"{i+1}. {x.url}" for i,x in enumerate(e.personal_images)) or "No personal images")
    @commands.hybrid_command(name="pic",description="Select a personal image")
    async def pic(self,ctx,character:str,number:int):
        async with self.bot.db.session() as db: await self.bot.images.select(db,ctx.author.id,character,number)
        await ctx.send("Image selected / تم اختيار الصورة")
    @commands.hybrid_command(name="piremove",description="Remove a personal image")
    async def piremove(self,ctx,character:str,number:int):
        async with self.bot.db.session() as db: await self.bot.images.remove(db,ctx.author.id,character,number)
        await ctx.send("Image removed / تمت إزالة الصورة")
    @commands.hybrid_command(name="pireset",description="Restore catalog default image")
    async def pireset(self,ctx,*,character:str):
        async with self.bot.db.session() as db: await self.bot.images.reset(db,ctx.author.id,character)
        await ctx.send("Image reset / تمت إعادة الصورة")

    async def _appearance_image(self,ctx,field,url,attachment):
        if (url is None)==(attachment is None): raise ValueError("provide exactly one URL or attachment")
        source=url or attachment.url; await self.bot.images.fetch(source)
        async with self.bot.db.session() as db: u=await self.collections.user(db,ctx.author.id); setattr(u,field,source)
        await ctx.send("Saved / تم الحفظ")
    @commands.command(name="avatar")
    async def avatar(self,ctx,url:str|None=None): await self._appearance_image(ctx,"avatar_url",url,ctx.message.attachments[0] if ctx.message.attachments else None)
    @commands.command(name="banner")
    async def banner(self,ctx,url:str|None=None): await self._appearance_image(ctx,"banner_url",url,ctx.message.attachments[0] if ctx.message.attachments else None)

    async def _custom(self,ctx,name,work,category,image,description="",shared=False):
        name=validate_safe_text(name,maximum=300,field="name"); work=validate_safe_text(work,maximum=300,field="work"); category=validate_safe_text(category,maximum=50,field="category")
        await self.bot.images.fetch(image)
        async with self.bot.db.session() as db:
            await self.collections.user(db,ctx.author.id)
            c=Character(name_original=name,name_en=name,category=category,description=validate_safe_text(description,maximum=2000) if description else None,is_custom=True,owner_id=ctx.author.id,review_status=ReviewStatus.pending)
            w=Work(title_original=work,title_en=work,category=category,review_status=ReviewStatus.pending); db.add_all([c,w]); await db.flush(); db.add(CharacterWork(character_id=c.id,work_id=w.id)); db.add(CustomSubmission(owner_id=ctx.author.id,character_id=c.id,submit_shared=shared)); await self.collections.add(db,ctx.author.id,c.id)
        await ctx.send("Created pending-review personal character / تم إنشاء شخصية شخصية قيد المراجعة")
    @commands.command(name="custom")
    async def custom_prefix(self,ctx,name:str,work:str,category:str,image:str,*,description:str=""): await self._custom(ctx,name,work,category,image,description)
    @app_commands.command(name="custom",description="Create a pending-review personal character")
    async def custom_slash(self,interaction:discord.Interaction,name:str,work:str,category:str,image:str,description:str="",submit_shared:bool=False):
        ctx=await commands.Context.from_interaction(interaction); await self._custom(ctx,name,work,category,image,description,submit_shared)
    @commands.hybrid_command(name="editcustom",description="Edit your custom character name")
    async def editcustom(self,ctx,character:str,*,name:str):
        async with self.bot.db.session() as db:
            e=await self.collections.resolve(db,ctx.author.id,character)
            if not e.character.is_custom or e.character.owner_id!=ctx.author.id: raise ValueError("not your custom character")
            e.character.name_original=validate_safe_text(name,maximum=300,field="name"); e.character.review_status=ReviewStatus.pending
        await ctx.send("Updated; review reset to pending / تم التحديث وإعادة المراجعة")

async def setup(bot): await bot.add_cog(ExtrasCog(bot))
