from __future__ import annotations
import io, json, random
import discord
from discord import app_commands
from discord.ext import commands
from sqlalchemy import String,cast,func,or_,select
from sqlalchemy.orm import selectinload
from bot.database.models import CatalogImage, Character, CharacterWork, CollectionEntry, ProviderReference, Report, ReviewStatus, User,Work
from bot.locales import t,value
from bot.services.collections import Ambiguous, CollectionService
from bot.services.content import validate_safe_text
from bot.views.profile import ProfileView
from bot.views.confirm import ConfirmView
from bot.views.search import CharacterSelectionView
from bot.views.help import HelpView
from bot.commands.parsing import find_arguments,profile_arguments

PLACEHOLDER = "https://placehold.co/600x900/5865F2/FFFFFF.png?text=Character"

class CoreCog(commands.Cog):
    def __init__(self, bot): self.bot=bot; self.collections=CollectionService()
    async def lang(self, db, uid):
        user = await db.get(User, uid); return user.language if user else self.bot.settings.default_language
    async def cog_command_error(self, ctx, error):
        if isinstance(error,commands.CheckFailure) and not isinstance(error,commands.NoPrivateMessage): return
        original = getattr(error, "original", error); await ctx.send(f"Error / خطأ: {original}", allowed_mentions=discord.AllowedMentions.none())

    @commands.hybrid_command(description="Add an approved catalog character by stable ID")
    async def add(self, ctx, *, character: str):
        if ctx.interaction: await ctx.defer(ephemeral=True)
        async with self.bot.db.session() as db:
            found = await db.get(Character, character)
            if found and found.review_status==ReviewStatus.approved:
                entry=await self.collections.add(db,ctx.author.id,found.id);language=await self.lang(db,ctx.author.id)
                return await ctx.send(t(language,"added",name=found.name_original,position=entry.position),allowed_mentions=discord.AllowedMentions.none())
            term=f"%{character.casefold()}%"
            local=list((await db.scalars(select(Character).outerjoin(CharacterWork).outerjoin(Work).options(selectinload(Character.works).selectinload(CharacterWork.work)).where(Character.review_status==ReviewStatus.approved,or_(func.lower(Character.name_original).like(term),func.lower(func.coalesce(Character.name_en,"")).like(term),func.lower(func.coalesce(Character.name_ar,"")).like(term),func.lower(cast(Character.aliases,String)).like(term),((Work.review_status==ReviewStatus.approved)&func.lower(func.coalesce(Work.title_original,"")).like(term)))).distinct().limit(25))).unique().all())
        outcome=await self.bot.providers.search_characters(character);candidates=[x for x in outcome.results if x.content_approved]
        if not local and not candidates:
            detail="; ".join(outcome.warnings) or "Candidates may require human review"
            return await ctx.send(f"No approved result / لا توجد نتيجة معتمدة. {detail}")
        view=CharacterSelectionView(ctx.author.id,local,candidates);view.message=await ctx.send("Select the intended character / اختر الشخصية المقصودة",view=view)

    @commands.hybrid_command(description="Remove a collected character")
    async def remove(self, ctx, *, character: str):
        async with self.bot.db.session() as db:
            entry=await self.collections.remove(db,ctx.author.id,character); await ctx.send(f"Removed / تمت الإزالة: {entry.character.name_original}")

    @commands.hybrid_command(description="Move a character to a collection position")
    async def move(self, ctx, character: str, position: int):
        async with self.bot.db.session() as db:
            entry=await self.collections.move(db,ctx.author.id,character,position); await ctx.send(f"Moved / تم النقل: {entry.character.name_original} → {position}")

    async def _show_profile(self,ctx,target,position):
        if position<1: raise ValueError("position must be at least 1")
        async def render(p,image_index=0):
            async with self.bot.db.session() as db: return await self.bot.profiles.render(db,target,ctx.author.id,p,image_index)
        card=await render(position)
        if not card.total: return await ctx.send(embed=card.embed)
        if position>card.total: raise ValueError(f"position must be between 1 and {card.total}")
        view=ProfileView(ctx.author.id,render,position,card.total)
        view.image_index=card.image_index
        view.message=await ctx.send(embed=card.embed,view=view)

    @commands.command(name="profile")
    async def profile_prefix(self,ctx,*,arguments:str=""):
        target,position=profile_arguments(arguments,ctx.message.mentions,ctx.author);await self._show_profile(ctx,target,position)

    @app_commands.command(name="profile",description="Browse a collection profile")
    async def profile_slash(self,interaction:discord.Interaction,user:discord.User|None=None,position:app_commands.Range[int,1]|None=None):
        await self._show_profile(await commands.Context.from_interaction(interaction),user or interaction.user,position or 1)

    @commands.hybrid_command(name="random", description="Show a random collected character")
    async def random_character(self, ctx):
        async with self.bot.db.session() as db: _,total=await self.collections.page(db,ctx.author.id,1,viewer_id=ctx.author.id)
        if not total: return await ctx.send("Empty collection / مجموعة فارغة")
        async with self.bot.db.session() as db: card=await self.bot.profiles.render(db,ctx.author,ctx.author.id,random.randint(1,total))
        await ctx.send(embed=card.embed)

    async def _metadata(self,ctx,character,text,kind):
        async with self.bot.db.session() as db: await self.collections.set_metadata(db,ctx.author.id,character,**{kind:text}); await ctx.send("Saved / تم الحفظ")
    @commands.hybrid_command(name="nick",description="Set a personal nickname")
    async def nick(self,ctx,character:str,*,nickname:str): await self._metadata(ctx,character,nickname,"nickname")
    @commands.hybrid_command(name="unnick",description="Reset a personal nickname")
    async def unnick(self,ctx,*,character:str):
        async with self.bot.db.session() as db: await self.collections.set_metadata(db,ctx.author.id,character,reset="nickname"); await ctx.send("Saved / تم الحفظ")
    nickname_group=app_commands.Group(name="nickname",description="Manage personal nicknames")
    @nickname_group.command(name="set",description="Set a personal nickname")
    async def nickname_set(self,interaction:discord.Interaction,character:str,text:str):await self._metadata(await commands.Context.from_interaction(interaction),character,text,"nickname")
    @nickname_group.command(name="reset",description="Reset a personal nickname")
    async def nickname_reset(self,interaction:discord.Interaction,character:str):
        ctx=await commands.Context.from_interaction(interaction)
        async with self.bot.db.session() as db:await self.collections.set_metadata(db,interaction.user.id,character,reset="nickname")
        await ctx.send("Saved / تم الحفظ")
    @commands.command(name="note")
    async def note(self,ctx,character:str,*,text:str): await self._metadata(ctx,character,text,"note")
    @commands.command(name="unnote")
    async def unnote(self,ctx,*,character:str):
        async with self.bot.db.session() as db: await self.collections.set_metadata(db,ctx.author.id,character,reset="note"); await ctx.send("Saved / تم الحفظ")
    note_group=app_commands.Group(name="note",description="Manage personal notes")
    @note_group.command(name="set",description="Set a private collection note")
    async def note_set(self,interaction:discord.Interaction,character:str,text:str):await self._metadata(await commands.Context.from_interaction(interaction),character,text,"note")
    @note_group.command(name="reset",description="Reset a private collection note")
    async def note_reset(self,interaction:discord.Interaction,character:str):
        ctx=await commands.Context.from_interaction(interaction)
        async with self.bot.db.session() as db:await self.collections.set_metadata(db,interaction.user.id,character,reset="note")
        await ctx.send("Saved / تم الحفظ")

    @commands.hybrid_command(name="undo",description="Restore the most recently removed character")
    async def undo(self,ctx):
        async with self.bot.db.session() as db: entry=await self.collections.undo(db,ctx.author.id); await ctx.send(f"Restored / تمت الاستعادة: #{entry.position}")

    async def _find(self,ctx,query,user=None):
        target=user or ctx.author
        async with self.bot.db.session() as db: matches=await self.collections.search(db,target.id,query,viewer_id=ctx.author.id)
        result="\n".join(f"#{x.position} {x.nickname or x.character.name_original}" for x in matches) or "No matches / لا نتائج"
        await ctx.send(result,allowed_mentions=discord.AllowedMentions.none())

    @commands.command(name="find")
    async def find_prefix(self,ctx,*,query:str):
        target,cleaned=find_arguments(query,ctx.message.mentions,ctx.author);await self._find(ctx,cleaned,target)

    @app_commands.command(name="find",description="Search a collection")
    async def find_slash(self,interaction:discord.Interaction,query:str,user:discord.User|None=None):
        await self._find(await commands.Context.from_interaction(interaction),query,user)

    @commands.hybrid_command(name="compare",description="Compare collection overlap")
    async def compare(self,ctx,user:discord.User):
        async with self.bot.db.session() as db:
            mine=set((await db.scalars(select(CollectionEntry.character_id).where(CollectionEntry.user_id==ctx.author.id))).all()); theirs=set((await db.scalars(select(CollectionEntry.character_id).where(CollectionEntry.user_id==user.id))).all())
            shared=mine&theirs; denom=len(mine|theirs); score=(len(shared)/denom*100) if denom else 0
        await ctx.send(f"Shared / مشتركة: {len(shared)}\nJaccard overlap = intersection ÷ union: {score:.1f}%")

    @commands.hybrid_command(name="export",description="Export collection and settings as versioned JSON")
    async def export(self,ctx):
        async with self.bot.db.session() as db:
            user=await self.collections.user(db,ctx.author.id);entries=list((await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.personal_images),selectinload(CollectionEntry.character).selectinload(Character.references),selectinload(CollectionEntry.character).selectinload(Character.works).selectinload(CharacterWork.work)).where(CollectionEntry.user_id==ctx.author.id).order_by(CollectionEntry.position))).unique().all())
            data={"schema_version":2,"discord_user_id":str(ctx.author.id),"settings":{"language":user.language,"avatar_url":user.avatar_url,"banner_url":user.banner_url,"bio":user.bio,"title":user.title,"color":user.color},"collection":[{"position":e.position,"character_id":e.character_id,"name":e.character.name_original,"works":[{"id":x.work.id,"title":x.work.title_original,"category":x.work.category} for x in e.character.works],"providers":[{"provider":x.provider,"external_id":x.external_id,"source_url":x.source_url} for x in e.character.references],"nickname":e.nickname,"note":e.note,"selected_image":{"personal_id":e.selected_personal_image_id,"catalog_id":e.selected_catalog_image_id},"personal_images":[{"id":i.id,"url":i.url,"review_status":i.review_status.value} for i in e.personal_images]} for e in entries],"warning":"Descriptive JSON only; not a complete database backup or tested restore format. External image URLs may expire."}
        await ctx.send(file=discord.File(io.BytesIO(json.dumps(data,ensure_ascii=False,indent=2).encode()),filename="anpot-export.json"))

    @commands.hybrid_command(name="report",description="Report metadata, duplicate, image, or excluded content")
    async def report(self,ctx,character:str,reason:str,*,details:str=""):
        if reason not in {"metadata","duplicate","broken_image","excluded_content"}: raise ValueError("invalid report reason")
        async with self.bot.db.session() as db:
            entry=await self.collections.resolve(db,ctx.author.id,character); db.add(Report(reporter_id=ctx.author.id,character_id=entry.character_id,reason=reason,details=validate_safe_text(details,maximum=1000) if details else None))
        await ctx.send("Report submitted / تم إرسال البلاغ")

    @commands.hybrid_command(name="language",description="Set Arabic or English responses")
    async def language(self,ctx,value:str):
        if value not in {"ar","en"}: raise ValueError("language must be ar or en")
        async with self.bot.db.session() as db: user=await self.collections.user(db,ctx.author.id); user.language=value
        await ctx.send("Saved / تم الحفظ")

    @commands.hybrid_command(name="bio",description="Set profile biography")
    async def bio(self,ctx,*,text:str):
        async with self.bot.db.session() as db: user=await self.collections.user(db,ctx.author.id); user.bio=validate_safe_text(text,maximum=500,field="bio")
        await ctx.send("Saved / تم الحفظ")
    @commands.hybrid_command(name="title",description="Set profile title")
    async def title(self,ctx,*,text:str):
        async with self.bot.db.session() as db: user=await self.collections.user(db,ctx.author.id); user.title=validate_safe_text(text,maximum=100,field="title")
        await ctx.send("Saved / تم الحفظ")
    @commands.hybrid_command(name="color",description="Set profile hexadecimal color")
    async def color(self,ctx,hex:str):
        if len(hex.lstrip("#"))!=6 or any(c not in "0123456789abcdefABCDEF" for c in hex.lstrip("#")): raise ValueError("invalid hex color")
        async with self.bot.db.session() as db: user=await self.collections.user(db,ctx.author.id); user.color="#"+hex.lstrip("#")
        await ctx.send("Saved / تم الحفظ")
    @commands.command(name="settings")
    async def settings_view(self,ctx):
        async with self.bot.db.session() as db: u=await self.collections.user(db,ctx.author.id); await ctx.send(f"language={u.language}\ncolor={u.color}\ntitle={u.title or '-'}\nbio={u.bio or '-'}",allowed_mentions=discord.AllowedMentions.none())
    @commands.hybrid_command(name="reset",description="Reset one appearance option")
    async def reset(self,ctx,option:str):
        valid={"avatar":"avatar_url","banner":"banner_url","bio":"bio","title":"title","color":"color","language":"language"}
        async def execute():
            async with self.bot.db.session() as db:
                u=await self.collections.user(db,ctx.author.id)
                if option=="all": u.avatar_url=u.banner_url=u.bio=u.title=None; u.color="#5865F2";u.language=self.bot.settings.default_language
                elif option in valid: setattr(u,valid[option],{"color":"#5865F2","language":self.bot.settings.default_language}.get(option))
                else: raise ValueError("invalid reset option")
        if option=="all":
            view=ConfirmView(ctx.author.id,execute);view.message=await ctx.send("Confirm appearance reset? / تأكيد إعادة المظهر؟",view=view);return
        await execute(); await ctx.send("Reset / تمت إعادة الضبط")

    @commands.hybrid_command(name="help",description="Show the complete command guide")
    async def help_command(self,ctx):
        async with self.bot.db.session() as db:language=await self.lang(db,ctx.author.id)
        pages=[(section["title"],section["body"]) for section in value(language,"help_sections")]
        if await self.bot.trust.can_manage_server(ctx.author,ctx.guild):pages.append((value(language,"help_admin_title"),value(language,"help_admin_body")))
        if await self.bot.trust.is_global(ctx.author.id):pages.append((value(language,"help_catalog_title"),value(language,"help_catalog_body")))
        view=HelpView(ctx.author.id,pages,(t(language,"previous"),t(language,"next")));view.message=await ctx.send(embed=view.embed(),view=view)

async def setup(bot): await bot.add_cog(CoreCog(bot))
