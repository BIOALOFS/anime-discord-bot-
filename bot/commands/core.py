from __future__ import annotations
import io, json, random
import discord
from discord import app_commands
from discord.ext import commands
from sqlalchemy import func, select
from sqlalchemy.orm import selectinload
from bot.database.models import CatalogImage, Character, CollectionEntry, Report, ReviewStatus, User
from bot.locales import t
from bot.services.collections import Ambiguous, CollectionService
from bot.services.content import validate_safe_text
from bot.views.profile import ProfileView
from bot.views.confirm import ConfirmView

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
        async with self.bot.db.session() as db:
            # Search adapters present choices instead of guessing; direct add accepts stable IDs or one exact approved name.
            found = await db.get(Character, character)
            if not found:
                rows=list((await db.scalars(select(Character).where(Character.review_status==ReviewStatus.approved, func.lower(Character.name_original)==character.casefold()).limit(2))).all())
                if len(rows)!=1: raise ValueError("Use !p or /search character to select an unambiguous stable ID")
                found=rows[0]
            entry=await self.collections.add(db,ctx.author.id,found.id); language=await self.lang(db,ctx.author.id)
            await ctx.send(t(language,"added",name=found.name_original,position=entry.position), allowed_mentions=discord.AllowedMentions.none())

    @commands.hybrid_command(description="Remove a collected character")
    async def remove(self, ctx, *, character: str):
        async with self.bot.db.session() as db:
            entry=await self.collections.remove(db,ctx.author.id,character); await ctx.send(f"Removed / تمت الإزالة: {entry.character.name_original}")

    @commands.hybrid_command(description="Move a character to a collection position")
    async def move(self, ctx, character: str, position: int):
        async with self.bot.db.session() as db:
            entry=await self.collections.move(db,ctx.author.id,character,position); await ctx.send(f"Moved / تم النقل: {entry.character.name_original} → {position}")

    async def profile_embed(self, user: discord.User|discord.Member, position: int):
        async with self.bot.db.session() as db:
            rows,total=await self.collections.page(db,user.id,position)
            if not rows: return discord.Embed(title="Empty collection / مجموعة فارغة")
            e=rows[0]; c=e.character
            personal=next((x.url for x in e.personal_images if x.id==e.selected_personal_image_id),None)
            catalog=next((x.url for x in c.images if x.is_default and x.review_status==ReviewStatus.approved),None)
            embed=discord.Embed(title=f"{e.nickname + ' — ' if e.nickname else ''}{c.name_original}",description=e.note or c.description or "",color=discord.Color.from_str("#5865F2"))
            embed.set_author(name=user.display_name,icon_url=user.display_avatar.url); embed.set_image(url=personal or catalog or PLACEHOLDER)
            embed.add_field(name="Category / الفئة",value=c.category); embed.set_footer(text=f"{e.position} / {total} • images {len(e.personal_images)+len(c.images)}")
            return embed

    @commands.hybrid_command(description="Browse a collection profile")
    async def profile(self, ctx, user: discord.User|None=None, position: int=1):
        target=user or ctx.author
        async with self.bot.db.session() as db: _,total=await self.collections.page(db,target.id,position)
        if not total: return await ctx.send(embed=discord.Embed(title="Empty collection / مجموعة فارغة"))
        position=max(1,min(position,total)); render=lambda p:self.profile_embed(target,p)
        await ctx.send(embed=await render(position),view=ProfileView(ctx.author.id,render,position,total))

    @commands.hybrid_command(name="random", description="Show a random collected character")
    async def random_character(self, ctx):
        async with self.bot.db.session() as db: total=await db.scalar(select(func.count()).select_from(CollectionEntry).where(CollectionEntry.user_id==ctx.author.id)) or 0
        if not total: return await ctx.send("Empty collection / مجموعة فارغة")
        await ctx.send(embed=await self.profile_embed(ctx.author,random.randint(1,total)))

    async def _metadata(self,ctx,character,text,kind):
        async with self.bot.db.session() as db: await self.collections.set_metadata(db,ctx.author.id,character,**{kind:text}); await ctx.send("Saved / تم الحفظ")
    @commands.hybrid_command(name="nick",description="Set a personal nickname")
    async def nick(self,ctx,character:str,*,nickname:str): await self._metadata(ctx,character,nickname,"nickname")
    @commands.hybrid_command(name="unnick",description="Reset a personal nickname")
    async def unnick(self,ctx,*,character:str):
        async with self.bot.db.session() as db: await self.collections.set_metadata(db,ctx.author.id,character,reset="nickname"); await ctx.send("Saved / تم الحفظ")
    @commands.hybrid_command(name="note",description="Set a private collection note")
    async def note(self,ctx,character:str,*,text:str): await self._metadata(ctx,character,text,"note")
    @commands.hybrid_command(name="unnote",description="Reset a collection note")
    async def unnote(self,ctx,*,character:str):
        async with self.bot.db.session() as db: await self.collections.set_metadata(db,ctx.author.id,character,reset="note"); await ctx.send("Saved / تم الحفظ")

    @commands.hybrid_command(name="undo",description="Restore the most recently removed character")
    async def undo(self,ctx):
        async with self.bot.db.session() as db: entry=await self.collections.undo(db,ctx.author.id); await ctx.send(f"Restored / تمت الاستعادة: #{entry.position}")

    @commands.hybrid_command(name="find",description="Search a collection by name or nickname")
    async def find(self,ctx,query:str,user:discord.User|None=None):
        target=user or ctx.author
        async with self.bot.db.session() as db:
            try: entry=await self.collections.resolve(db,target.id,query); result=f"#{entry.position} {entry.nickname or entry.character.name_original}"
            except Ambiguous as exc: result="\n".join(f"#{x.position} {x.nickname or x.character.name_original}" for x in exc.matches)
        await ctx.send(result,allowed_mentions=discord.AllowedMentions.none())

    @commands.hybrid_command(name="compare",description="Compare collection overlap")
    async def compare(self,ctx,user:discord.User):
        async with self.bot.db.session() as db:
            mine=set((await db.scalars(select(CollectionEntry.character_id).where(CollectionEntry.user_id==ctx.author.id))).all()); theirs=set((await db.scalars(select(CollectionEntry.character_id).where(CollectionEntry.user_id==user.id))).all())
            shared=mine&theirs; denom=len(mine|theirs); score=(len(shared)/denom*100) if denom else 0
        await ctx.send(f"Shared / مشتركة: {len(shared)}\nJaccard overlap = intersection ÷ union: {score:.1f}%")

    @commands.hybrid_command(name="export",description="Export collection and settings as versioned JSON")
    async def export(self,ctx):
        async with self.bot.db.session() as db:
            user=await self.collections.user(db,ctx.author.id); entries=list((await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.personal_images)).where(CollectionEntry.user_id==ctx.author.id).order_by(CollectionEntry.position))).all())
            data={"schema_version":1,"discord_user_id":str(ctx.author.id),"settings":{"language":user.language,"avatar_url":user.avatar_url,"banner_url":user.banner_url,"bio":user.bio,"title":user.title,"color":user.color},"collection":[{"position":e.position,"character_id":e.character_id,"nickname":e.nickname,"note":e.note,"personal_images":[i.url for i in e.personal_images]} for e in entries],"warning":"External image URLs may expire and are not permanent backups."}
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
    @commands.hybrid_command(name="settings",description="View appearance settings")
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
        if option=="all": return await ctx.send("Confirm appearance reset? / تأكيد إعادة المظهر؟",view=ConfirmView(ctx.author.id,execute))
        await execute(); await ctx.send("Reset / تمت إعادة الضبط")

    @commands.hybrid_command(name="help",description="Show the complete command guide")
    async def help_command(self,ctx):
        text="""**Collection / المجموعة** `add remove move profile random undo compare export`\n**Search / البحث** `p sp sa sap find`\n**Images / الصور** `pi images pic piremove pireset avatar banner`\n**Nicknames & Notes / الألقاب والملاحظات** `nick unnick note unnote`\n**Appearance / المظهر** `bio title color language settings reset`\n**Utilities / أدوات** `custom editcustom report help`\n**Server Administration / إدارة الخادم** `setup #channel` • `channel add|remove|list|all`\nExample / مثال: `!setup #characters`. Requires server owner, Manage Server, global trust, or this server's trust. Confirmations recheck access.\nQuote multi-word prefix arguments: `!move \"Sailor Moon\" 2`. Use slash forms for guided arguments. See COMMANDS.md for every example."""
        await ctx.send(text)

async def setup(bot): await bot.add_cog(CoreCog(bot))
