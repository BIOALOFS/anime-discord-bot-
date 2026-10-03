import discord
class ConfirmView(discord.ui.View):
    def __init__(self, user_id, callback, permission_check=None): super().__init__(timeout=60); self.user_id=user_id; self.callback=callback; self.permission_check=permission_check
    async def interaction_check(self, interaction):
        if interaction.user.id != self.user_id:
            await interaction.response.send_message("This confirmation belongs to another user / هذا التأكيد لمستخدم آخر",ephemeral=True);return False
        if self.permission_check and not await self.permission_check(interaction):
            await interaction.response.send_message("Permission revoked or unavailable / تم إلغاء الصلاحية أو أنها غير متاحة",ephemeral=True);return False
        if not self.permission_check:
            allowed,message=await interaction.client.ordinary_access(interaction.guild,interaction.channel)
            if not allowed:
                await interaction.response.send_message(message,ephemeral=True);return False
        return True
    @discord.ui.button(label="Confirm / تأكيد", style=discord.ButtonStyle.danger)
    async def confirm(self, interaction, button): await self.callback(); await interaction.response.edit_message(content="Change complete / تم التغيير", view=None)
    @discord.ui.button(label="Cancel / إلغاء", style=discord.ButtonStyle.secondary)
    async def cancel(self, interaction, button): await interaction.response.edit_message(content="Cancelled / أُلغي", view=None)
