from __future__ import annotations
import discord
from discord import app_commands
from discord.ext import commands
from sqlalchemy import select
from bot.database.models import CatalogImage,Character,CharacterWork,CollectionEntry,CustomSubmission,ProviderReference,ReviewStatus,User,Work
from bot.services.collections import CollectionService
from bot.services.content import validate_safe_text
from bot.views.search import ImageGalleryView,SearchCardView,TextSearchView,WorkSelectionView
from bot.views.confirm import ConfirmView

class ExtrasCog(commands.Cog):
    def __init__(self,bot):self.bot=bot;self.collections=CollectionService()
    async def _defer(self,ctx,ephemeral=False):
        if ctx.interaction and not ctx.interaction.response.is_done():await ctx.defer(ephemeral=ephemeral)
    async def _search(self,ctx,query,images=False):
        await self._defer(ctx);outcome=await self.bot.search.characters(query)
        if not outcome.results:return await ctx.send("No approved results; unknown candidates require human review / لا توجد نتائج معتمدة")
        if images:
            refs={}
            async with self.bot.db.session() as db:
                rows=(await db.execute(select(ProviderReference.provider,ProviderReference.external_id).join(Character,Character.id==ProviderReference.character_id).join(CollectionEntry,CollectionEntry.character_id==Character.id).where(CollectionEntry.user_id==ctx.author.id))).all()
                for provider,external_id in rows:refs.setdefault(provider,set()).add(external_id)
            view=SearchCardView(ctx.author.id,outcome.results,refs);view.message=await ctx.send(embed=view.embed(),view=view)
        else:
            view=TextSearchView(ctx.author.id,outcome.results,outcome.warnings);view.message=await ctx.send(embed=view.embed(),view=view,suppress_embeds=True)
    @commands.hybrid_command(name="p",description="Text-only character search")
    async def p(self,ctx,*,query:str):await self._search(ctx,query,False)
    @commands.hybrid_command(name="sp",description="Visual character search")
    async def sp(self,ctx,*,query:str):await self._search(ctx,query,True)

    async def _work_search(self,ctx,query,images=False):
        await self._defer(ctx);items=[];warnings=[]
        for provider in self.bot.providers.providers:
            try:items.extend((provider,w) for w in await provider.search_works(query) if w.content_approved)
            except Exception as exc:warnings.append(f"{provider.name}: {exc}")
        if not items:return await ctx.send("No works / لا توجد أعمال"+("; "+"; ".join(warnings) if warnings else ""))
        view=WorkSelectionView(ctx.author.id,items,images);view.message=await ctx.send("Select the exact production/adaptation / اختر العمل أو الاقتباس",view=view)
    @commands.hybrid_command(name="sa",description="Find a work then show its characters")
    async def sa(self,ctx,*,work:str):await self._work_search(ctx,work,False)
    @commands.hybrid_command(name="sap",description="Find a work then visually browse characters")
    async def sap(self,ctx,*,work:str):await self._work_search(ctx,work,True)
    search_group=app_commands.Group(name="search",description="Search characters and works")
    @search_group.command(name="character",description="Search characters")
    async def search_character(self,interaction:discord.Interaction,query:str,images:bool=False):await self._search(await commands.Context.from_interaction(interaction),query,images)
    @search_group.command(name="work",description="Select a work and browse its characters")
    async def search_work(self,interaction:discord.Interaction,query:str,images:bool=False):await self._work_search(await commands.Context.from_interaction(interaction),query,images)

    async def _add_image(self,ctx,character,url,attachment):
        if (url is None)==(attachment is None):raise ValueError("provide exactly one URL or attachment")
        await self._defer(ctx,True);source=url or attachment.url
        async with self.bot.db.session() as db:await self.bot.images.add(db,ctx.author.id,character,source)
        await ctx.send("Image retained pending content review / الصورة محفوظة بانتظار المراجعة")
    @commands.command(name="pi")
    async def pi(self,ctx,*,argument:str=""):
        parts=argument.rsplit(maxsplit=1);url=parts[-1] if parts and parts[-1].startswith("https://") else None;character=parts[0] if url and len(parts)>1 else argument
        await self._add_image(ctx,character,url,ctx.message.attachments[0] if ctx.message.attachments else None)

    async def _images(self,ctx,character):
        async with self.bot.db.session() as db:
            entry=await self.collections.resolve(db,ctx.author.id,character);gallery=await self.bot.images.gallery(db,entry)
        if not gallery:return await ctx.send("No approved images / لا توجد صور معتمدة")
        view=ImageGalleryView(ctx.author.id,entry.character.name_original,gallery);view.message=await ctx.send(embed=view.embed(),view=view)
    @commands.hybrid_command(name="images",description="Browse approved character images")
    async def images(self,ctx,*,character:str):await self._images(ctx,character)
    async def _select_image(self,ctx,character,number):
        async with self.bot.db.session() as db:await self.bot.images.select(db,ctx.author.id,character,number)
        await ctx.send("Image selected / تم اختيار الصورة")
    @commands.hybrid_command(name="pic",description="Select an approved image")
    async def pic(self,ctx,character:str,number:int):await self._select_image(ctx,character,number)
    async def _remove_image(self,ctx,character,number):
        async with self.bot.db.session() as db:await self.bot.images.remove(db,ctx.author.id,character,number)
        await ctx.send("Image removed / تمت إزالة الصورة")
    @commands.hybrid_command(name="piremove",description="Remove a personal image")
    async def piremove(self,ctx,character:str,number:int):await self._remove_image(ctx,character,number)
    async def _reset_image(self,ctx,character):
        async with self.bot.db.session() as db:await self.bot.images.reset(db,ctx.author.id,character)
        await ctx.send("Image reset / تمت إعادة الصورة")
    @commands.hybrid_command(name="pireset",description="Restore catalog default image")
    async def pireset(self,ctx,*,character:str):await self._reset_image(ctx,character)

    image_group=app_commands.Group(name="image",description="Manage character images")
    @image_group.command(name="add",description="Add one URL or attachment for review")
    async def image_add(self,interaction:discord.Interaction,character:str,url:str|None=None,attachment:discord.Attachment|None=None):await self._add_image(await commands.Context.from_interaction(interaction),character,url,attachment)
    @image_group.command(name="list",description="Browse approved images")
    async def image_list(self,interaction:discord.Interaction,character:str):await self._images(await commands.Context.from_interaction(interaction),character)
    @image_group.command(name="select",description="Persist an approved default image")
    async def image_select(self,interaction:discord.Interaction,character:str,number:int):await self._select_image(await commands.Context.from_interaction(interaction),character,number)
    @image_group.command(name="remove",description="Remove a personal image")
    async def image_remove(self,interaction:discord.Interaction,character:str,number:int):await self._remove_image(await commands.Context.from_interaction(interaction),character,number)
    @image_group.command(name="reset",description="Restore catalog default")
    async def image_reset(self,interaction:discord.Interaction,character:str):await self._reset_image(await commands.Context.from_interaction(interaction),character)

    async def _appearance_image(self,ctx,field,url,attachment):
        if (url is None)==(attachment is None):raise ValueError("provide exactly one URL or attachment")
        await self._defer(ctx,True);source=url or attachment.url;await self.bot.images.fetch(source)
        async with self.bot.db.session() as db:
            user=await self.collections.user(db,ctx.author.id);setattr(user,field,source);setattr(user,field.replace("_url","_review_status"),ReviewStatus.pending)
        await ctx.send("Upload retained pending content review / محفوظة بانتظار المراجعة")
    @commands.command(name="avatar")
    async def avatar(self,ctx,url:str|None=None):await self._appearance_image(ctx,"avatar_url",url,ctx.message.attachments[0] if ctx.message.attachments else None)
    @commands.command(name="banner")
    async def banner(self,ctx,url:str|None=None):await self._appearance_image(ctx,"banner_url",url,ctx.message.attachments[0] if ctx.message.attachments else None)
    settings_group=app_commands.Group(name="settings",description="Profile appearance settings")
    @settings_group.command(name="avatar",description="Upload avatar for review")
    async def settings_avatar(self,interaction:discord.Interaction,url:str|None=None,attachment:discord.Attachment|None=None):await self._appearance_image(await commands.Context.from_interaction(interaction),"avatar_url",url,attachment)
    @settings_group.command(name="banner",description="Upload banner for review")
    async def settings_banner(self,interaction:discord.Interaction,url:str|None=None,attachment:discord.Attachment|None=None):await self._appearance_image(await commands.Context.from_interaction(interaction),"banner_url",url,attachment)
    @settings_group.command(name="view",description="View profile settings")
    async def settings_view(self,interaction:discord.Interaction):
        async with self.bot.db.session() as db:user=await self.collections.user(db,interaction.user.id)
        await interaction.response.send_message(f"language={user.language}\ncolor={user.color}\ntitle={user.title or '-'}\nbio={user.bio or '-'}",ephemeral=True)
    @settings_group.command(name="bio",description="Set profile biography")
    async def settings_bio(self,interaction:discord.Interaction,text:str):
        async with self.bot.db.session() as db:user=await self.collections.user(db,interaction.user.id);user.bio=validate_safe_text(text,maximum=500,field="bio")
        await interaction.response.send_message("Saved / تم الحفظ",ephemeral=True)
    @settings_group.command(name="title",description="Set profile title")
    async def settings_title(self,interaction:discord.Interaction,text:str):
        async with self.bot.db.session() as db:user=await self.collections.user(db,interaction.user.id);user.title=validate_safe_text(text,maximum=100,field="title")
        await interaction.response.send_message("Saved / تم الحفظ",ephemeral=True)
    @settings_group.command(name="color",description="Set profile hexadecimal color")
    async def settings_color(self,interaction:discord.Interaction,hex:str):
        if len(hex.lstrip("#"))!=6 or any(x not in "0123456789abcdefABCDEF" for x in hex.lstrip("#")):raise ValueError("invalid color")
        async with self.bot.db.session() as db:user=await self.collections.user(db,interaction.user.id);user.color="#"+hex.lstrip("#")
        await interaction.response.send_message("Saved / تم الحفظ",ephemeral=True)
    @settings_group.command(name="language",description="Set Arabic or English")
    async def settings_language(self,interaction:discord.Interaction,value:str):
        if value not in {"ar","en"}:raise ValueError("invalid language")
        async with self.bot.db.session() as db:user=await self.collections.user(db,interaction.user.id);user.language=value
        await interaction.response.send_message("Saved / تم الحفظ",ephemeral=True)
    @settings_language.autocomplete("value")
    async def language_choices(self,interaction,current):return [app_commands.Choice(name="Arabic",value="ar"),app_commands.Choice(name="English",value="en")]
    @settings_group.command(name="reset",description="Reset an appearance option")
    async def settings_reset(self,interaction:discord.Interaction,option:str):
        async def execute():
            async with self.bot.db.session() as db:
                user=await self.collections.user(db,interaction.user.id)
                if option=="all":user.avatar_url=user.banner_url=user.bio=user.title=None;user.color="#5865F2";user.language=self.bot.settings.default_language
                elif option in {"avatar","banner","bio","title"}:setattr(user,option+"_url" if option in {"avatar","banner"} else option,None)
                elif option=="color":user.color="#5865F2"
                elif option=="language":user.language=self.bot.settings.default_language
                else:raise ValueError("invalid reset option")
        if option=="all":
            view=ConfirmView(interaction.user.id,execute);await interaction.response.send_message("Confirm reset / تأكيد إعادة الضبط",view=view,ephemeral=True);view.message=await interaction.original_response();return
        await execute();await interaction.response.send_message("Reset / تمت إعادة الضبط",ephemeral=True)

    async def _custom(self,ctx,name,work,category,image,description="",shared=False):
        await self._defer(ctx,True);name=validate_safe_text(name,maximum=300,field="name");work=validate_safe_text(work,maximum=300,field="work");category=validate_safe_text(category,maximum=50,field="category");await self.bot.images.fetch(image)
        async with self.bot.db.session() as db:
            await self.collections.user(db,ctx.author.id);character=Character(name_original=name,name_en=name,category=category,description=validate_safe_text(description,maximum=2000) if description else None,is_custom=True,owner_id=ctx.author.id,review_status=ReviewStatus.pending);work_row=Work(title_original=work,title_en=work,category=category,review_status=ReviewStatus.pending);db.add_all([character,work_row]);await db.flush();db.add(CharacterWork(character_id=character.id,work_id=work_row.id));db.add(CatalogImage(character_id=character.id,url=image,is_default=True,source_url=image,review_status=ReviewStatus.pending));db.add(CustomSubmission(owner_id=ctx.author.id,character_id=character.id,submit_shared=shared));await self.collections.add(db,ctx.author.id,character.id)
        await ctx.send("Created with image, pending review / تم الإنشاء مع الصورة بانتظار المراجعة")
    @commands.command(name="custom")
    async def custom_prefix(self,ctx,name:str,work:str,category:str,image:str,*,description:str=""):await self._custom(ctx,name,work,category,image,description)
    @app_commands.command(name="custom",description="Create a pending-review personal character")
    async def custom_slash(self,interaction:discord.Interaction,name:str,work:str,category:str,image:str,description:str="",submit_shared:bool=False):await self._custom(await commands.Context.from_interaction(interaction),name,work,category,image,description,submit_shared)
    @commands.hybrid_command(name="editcustom",description="Edit one of your custom characters")
    async def editcustom(self,ctx,character:str,field:str,*,value:str):
        limits={"name":300,"work":300,"category":50,"description":2000,"image":2000}
        if field not in limits:raise ValueError("field must be name, work, category, description, or image")
        value=validate_safe_text(value,maximum=limits[field],field=field)
        if field=="image":await self._defer(ctx,True);await self.bot.images.fetch(value)
        async with self.bot.db.session() as db:
            entry=await self.collections.resolve(db,ctx.author.id,character);item=entry.character
            if not item.is_custom or item.owner_id!=ctx.author.id:raise ValueError("not your custom character")
            if field=="name":item.name_original=value
            elif field=="category":item.category=value
            elif field=="description":item.description=value
            elif field=="work":
                if not item.works:raise ValueError("custom character has no associated work")
                item.works[0].work.title_original=value;item.works[0].work.review_status=ReviewStatus.pending
            else:db.add(CatalogImage(character_id=item.id,url=value,source_url=value,is_default=False,review_status=ReviewStatus.pending))
            item.review_status=ReviewStatus.pending
        await ctx.send("Updated; approval reset / تم التحديث وإعادة المراجعة")

async def setup(bot):await bot.add_cog(ExtrasCog(bot))
