from __future__ import annotations
import discord
from bot.database.models import ReviewStatus
from bot.services.profile import PLACEHOLDER

class ExpiringView(discord.ui.View):
    def __init__(self,owner_id,timeout=180):super().__init__(timeout=timeout);self.owner_id=owner_id;self.message=None
    async def interaction_check(self,interaction):
        if interaction.user.id!=self.owner_id:
            await interaction.response.send_message("This menu belongs to another user / هذه القائمة لمستخدم آخر",ephemeral=True);return False
        allowed,message=await interaction.client.ordinary_access(interaction.guild,interaction.channel)
        if not allowed:await interaction.response.send_message(message,ephemeral=True);return False
        return True
    async def on_timeout(self):
        for child in self.children:child.disabled=True
        if self.message:
            try:await self.message.edit(view=self)
            except (discord.HTTPException,discord.NotFound,discord.Forbidden):pass

class AddSelection(discord.ui.Select):
    def __init__(self,view,options):super().__init__(placeholder="Select character / اختر الشخصية",options=options[:25]);self.host=view
    async def callback(self,interaction):
        await interaction.response.defer(ephemeral=True)
        value=self.values[0]
        async with interaction.client.db.session() as db:
            if value.startswith("local:"):character_id=value[6:]
            else:
                candidate=self.host.candidates[int(value.split(":",1)[1])]
                if not candidate.content_approved:
                    return await interaction.followup.send("Pending human review; it cannot be added yet / بانتظار المراجعة",ephemeral=True)
                character=(await interaction.client.catalog.import_candidate(db,candidate))
                if character.review_status!=ReviewStatus.approved:return await interaction.followup.send("Pending review / بانتظار المراجعة",ephemeral=True)
                character_id=character.id
            entry=await interaction.client.collection_service.add(db,interaction.user.id,character_id)
        await interaction.followup.send(f"Added at position {entry.position} / تمت الإضافة",ephemeral=True)

class CharacterSelectionView(ExpiringView):
    def __init__(self,owner_id,local,candidates):
        super().__init__(owner_id);self.candidates=candidates
        options=[]
        for character in local:
            works=", ".join(x.work.title_original for x in character.works if x.work.review_status==ReviewStatus.approved) or "Catalog"
            options.append(discord.SelectOption(label=character.name_original[:100],description=works[:100],value=f"local:{character.id}"))
        for index,candidate in enumerate(candidates):
            works=", ".join(x.title for x in candidate.works[:2]) or candidate.provider
            options.append(discord.SelectOption(label=candidate.name[:100],description=works[:100],value=f"candidate:{index}"))
        self.add_item(AddSelection(self,options))

class SearchCardView(ExpiringView):
    def __init__(self,owner_id,candidates,collected):super().__init__(owner_id);self.candidates=candidates;self.collected=collected;self.index=0
    def embed(self):
        item=self.candidates[self.index];works=", ".join(x.title for x in item.works) or "—"
        embed=discord.Embed(title=item.name,description=f"{works}\nProvider: {item.provider}")
        image=next((x.url for x in item.images if x.verified_character_image and item.content_approved),PLACEHOLDER);embed.set_image(url=image);embed.set_footer(text=f"Result {self.index+1} / {len(self.candidates)} — provider coverage may be incomplete")
        return embed
    @discord.ui.button(label="◀",style=discord.ButtonStyle.secondary)
    async def previous(self,interaction,button):self.index=(self.index-1)%len(self.candidates);await interaction.response.edit_message(embed=self.embed(),view=self)
    @discord.ui.button(label="▶",style=discord.ButtonStyle.secondary)
    async def next(self,interaction,button):self.index=(self.index+1)%len(self.candidates);await interaction.response.edit_message(embed=self.embed(),view=self)
    @discord.ui.button(label="Add / إضافة",style=discord.ButtonStyle.success)
    async def add(self,interaction,button):
        item=self.candidates[self.index]
        if item.external_id in self.collected.get(item.provider,set()):return await interaction.response.send_message("Already collected / مضافة مسبقاً",ephemeral=True)
        await interaction.response.defer(ephemeral=True)
        async with interaction.client.db.session() as db:
            character=await interaction.client.catalog.import_candidate(db,item)
            if character.review_status!=ReviewStatus.approved:return await interaction.followup.send("Submitted for review; not publicly visible yet / أرسلت للمراجعة",ephemeral=True)
            await interaction.client.collection_service.add(db,interaction.user.id,character.id)
        self.collected.setdefault(item.provider,set()).add(item.external_id);await interaction.followup.send("Added / تمت الإضافة",ephemeral=True)

class TextSearchView(ExpiringView):
    def __init__(self,owner_id,candidates,warnings):super().__init__(owner_id);self.items=candidates;self.warnings=warnings;self.page=0
    def embed(self):
        chunk=self.items[self.page*10:(self.page+1)*10];lines=[f"**{x.name}** — {', '.join(w.title for w in x.works) or '—'} ({x.provider})" for x in chunk]
        embed=discord.Embed(title="Character search / بحث الشخصيات",description="\n".join(lines) or "No approved results / لا توجد نتائج معتمدة");pages=max(1,(len(self.items)+9)//10);embed.set_footer(text=f"Page {self.page+1}/{pages}. Provider coverage may be incomplete. {'; '.join(self.warnings)}");return embed
    @discord.ui.button(label="◀",style=discord.ButtonStyle.secondary)
    async def previous(self,i,b):self.page=max(0,self.page-1);await i.response.edit_message(embed=self.embed(),view=self)
    @discord.ui.button(label="▶",style=discord.ButtonStyle.secondary)
    async def next(self,i,b):self.page=min(max(0,(len(self.items)-1)//10),self.page+1);await i.response.edit_message(embed=self.embed(),view=self)

class WorkSelect(discord.ui.Select):
    def __init__(self,host,items):
        self.host=host;super().__init__(placeholder="Select work / اختر العمل",options=[discord.SelectOption(label=w.title[:100],description=f"{p.name} • {w.category}"[:100],value=str(i)) for i,(p,w) in enumerate(items[:25])])
    async def callback(self,interaction):
        await interaction.response.defer()
        provider,work=self.host.items[int(self.values[0])]
        try:characters=[x for x in await provider.characters_for_work(work) if x.content_approved]
        except Exception as exc:return await interaction.followup.send(f"Provider unavailable: {exc}",ephemeral=True)
        if not characters:return await interaction.followup.send("No approved characters; candidates may require review / لا توجد شخصيات معتمدة",ephemeral=True)
        if self.host.images:
            view=SearchCardView(interaction.user.id,characters,{});view.message=await interaction.followup.send(embed=view.embed(),view=view)
        else:
            view=TextSearchView(interaction.user.id,characters,[]);view.message=await interaction.followup.send(embed=view.embed(),view=view,suppress_embeds=True)

class WorkSelectionView(ExpiringView):
    def __init__(self,owner_id,items,images):super().__init__(owner_id);self.items=items;self.images=images;self.add_item(WorkSelect(self,items))

class ReviewImportSelect(discord.ui.Select):
    def __init__(self,host):super().__init__(placeholder="Import pending candidate for review",options=[discord.SelectOption(label=x.name[:100],description=f"{x.provider}: {', '.join(w.title for w in x.works) or 'unknown work'}"[:100],value=str(i)) for i,x in enumerate(host.candidates[:25])]);self.host=host
    async def callback(self,interaction):
        if not await interaction.client.trust.is_global(interaction.user.id):return await interaction.response.send_message("Global trust was revoked",ephemeral=True)
        await interaction.response.defer(ephemeral=True);candidate=self.host.candidates[int(self.values[0])]
        async with interaction.client.db.session() as db:character=await interaction.client.catalog.import_candidate(db,candidate)
        await interaction.followup.send(f"Imported as `{character.id}` with status `{character.review_status.value}`. Review metadata, work and every image separately.",ephemeral=True)
class ReviewImportView(ExpiringView):
    def __init__(self,owner_id,candidates):super().__init__(owner_id);self.candidates=candidates;self.add_item(ReviewImportSelect(self))
    async def interaction_check(self,interaction):
        if interaction.user.id!=self.owner_id:
            await interaction.response.send_message("This menu belongs to another user",ephemeral=True);return False
        if not await interaction.client.trust.is_global(interaction.user.id):
            await interaction.response.send_message("Global trust was revoked",ephemeral=True);return False
        return True

class ImageGalleryView(ExpiringView):
    def __init__(self,owner_id,name,gallery):super().__init__(owner_id);self.name=name;self.gallery=gallery;self.index=0
    def embed(self):
        kind,image=self.gallery[self.index];embed=discord.Embed(title=f"{self.name} — {kind}");embed.set_image(url=image.url);embed.set_footer(text=f"Image {self.index+1} / {len(self.gallery)}");return embed
    @discord.ui.button(label="◀",style=discord.ButtonStyle.secondary)
    async def previous(self,i,b):self.index=(self.index-1)%len(self.gallery);await i.response.edit_message(embed=self.embed(),view=self)
    @discord.ui.button(label="▶",style=discord.ButtonStyle.secondary)
    async def next(self,i,b):self.index=(self.index+1)%len(self.gallery);await i.response.edit_message(embed=self.embed(),view=self)
