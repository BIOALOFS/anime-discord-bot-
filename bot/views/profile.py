from __future__ import annotations
import discord

class ProfileView(discord.ui.View):
    def __init__(self,opener_id,render,position,total):
        super().__init__(timeout=180);self.opener_id=opener_id;self.render=render;self.position=position;self.total=total;self.image_index=0;self.message=None
    async def interaction_check(self,interaction):
        if interaction.user.id!=self.opener_id:
            await interaction.response.send_message("Only the opener may control this view / يمكن للفاتح فقط التحكم",ephemeral=True);return False
        allowed,message=await interaction.client.ordinary_access(interaction.guild,interaction.channel)
        if not allowed:await interaction.response.send_message(message,ephemeral=True);return False
        return True
    async def _go(self,interaction,position=None,image_delta=0):
        await interaction.response.defer()
        if position is not None:self.position=max(1,min(self.total,position));self.image_index=0
        else:self.image_index+=image_delta
        card=await self.render(self.position,self.image_index);self.image_index=card.image_index
        await interaction.edit_original_response(embed=card.embed,view=self)
    @discord.ui.button(label="⏮",style=discord.ButtonStyle.secondary,row=0)
    async def first(self,i,b):await self._go(i,1)
    @discord.ui.button(label="◀",style=discord.ButtonStyle.secondary,row=0)
    async def previous(self,i,b):await self._go(i,self.position-1)
    @discord.ui.button(label="▶",style=discord.ButtonStyle.secondary,row=0)
    async def next(self,i,b):await self._go(i,self.position+1)
    @discord.ui.button(label="⏭",style=discord.ButtonStyle.secondary,row=0)
    async def last(self,i,b):await self._go(i,self.total)
    @discord.ui.button(label="🖼️ ◀",style=discord.ButtonStyle.primary,row=1)
    async def image_previous(self,i,b):await self._go(i,image_delta=-1)
    @discord.ui.button(label="🖼️ ▶",style=discord.ButtonStyle.primary,row=1)
    async def image_next(self,i,b):await self._go(i,image_delta=1)
    async def on_timeout(self):
        for item in self.children:item.disabled=True
        if self.message:
            try:await self.message.edit(view=self)
            except (discord.NotFound,discord.Forbidden,discord.HTTPException):pass
