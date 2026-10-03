from __future__ import annotations
import discord


class ProfileView(discord.ui.View):
    def __init__(self, opener_id: int, render, position: int, total: int):
        super().__init__(timeout=180); self.opener_id, self.render, self.position, self.total = opener_id, render, position, total
    async def interaction_check(self, interaction: discord.Interaction):
        if interaction.user.id != self.opener_id:
            await interaction.response.send_message("يمكن للشخص الذي فتح الواجهة فقط التحكم بها. / Only the opener may control this view.", ephemeral=True); return False
        allowed,message=await interaction.client.ordinary_access(interaction.guild,interaction.channel)
        if not allowed:
            await interaction.response.send_message(message,ephemeral=True,allowed_mentions=discord.AllowedMentions.none());return False
        return True
    async def _go(self, interaction, position):
        self.position = max(1, min(self.total, position)); embed = await self.render(self.position); await interaction.response.edit_message(embed=embed, view=self)
    @discord.ui.button(label="⏮", style=discord.ButtonStyle.secondary)
    async def first(self, interaction, button): await self._go(interaction, 1)
    @discord.ui.button(label="◀", style=discord.ButtonStyle.secondary)
    async def previous(self, interaction, button): await self._go(interaction, self.position-1)
    @discord.ui.button(label="▶", style=discord.ButtonStyle.secondary)
    async def next(self, interaction, button): await self._go(interaction, self.position+1)
    @discord.ui.button(label="⏭", style=discord.ButtonStyle.secondary)
    async def last(self, interaction, button): await self._go(interaction, self.total)
    async def on_timeout(self):
        for item in self.children: item.disabled = True
