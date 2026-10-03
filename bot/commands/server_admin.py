from __future__ import annotations
import discord
from discord import app_commands
from discord.ext import commands
from bot.views.confirm import ConfirmView


async def require_server_admin(ctx):
    if not ctx.guild or not isinstance(ctx.author,discord.Member): raise commands.NoPrivateMessage("server administration is unavailable in direct messages")
    if not await ctx.bot.trust.can_manage_server(ctx.author,ctx.guild): raise commands.CheckFailure("server owner, Manage Server, or configured server trust required")
    return True


class ServerAdminCog(commands.Cog):
    def __init__(self,bot): self.bot=bot
    async def _allowed(self,interaction):
        return bool(interaction.guild and isinstance(interaction.user,discord.Member) and await self.bot.trust.can_manage_server(interaction.user,interaction.guild))
    async def _validate(self,ctx,channel):
        if not ctx.guild: raise commands.NoPrivateMessage()
        self.bot.server_configs.validate_channel(ctx.guild,channel,ctx.guild.me)
    async def _setup(self,ctx,channel):
        await self._validate(ctx,channel)
        async with self.bot.db.session() as db: config=await self.bot.server_configs.get(db,ctx.guild.id); configured=bool(config and config.setup_complete)
        async def apply():
            await self._validate(ctx,channel)
            async with self.bot.db.session() as db: await self.bot.server_configs.replace(db,ctx.guild.id,channel.id,ctx.author.id)
        if configured:
            return await ctx.send("Replace the existing channel configuration? / هل تريد استبدال إعداد القنوات الحالي؟",view=ConfirmView(ctx.author.id,apply,self._allowed))
        await apply();await ctx.send(f"Configured / تم الإعداد: {channel.mention}")
    @commands.hybrid_command(name="setup",description="Configure the initial allowed bot channel")
    @commands.check(require_server_admin)
    async def setup_command(self,ctx,channel:discord.TextChannel): await self._setup(ctx,channel)

    @commands.group(name="channel",invoke_without_command=True)
    @commands.check(require_server_admin)
    async def channel_prefix(self,ctx): await ctx.send("Use: `!channel add #channel`, `remove`, `list`, or `all`.")
    @channel_prefix.command(name="add")
    @commands.check(require_server_admin)
    async def channel_add_prefix(self,ctx,channel:discord.TextChannel): await self._add(ctx,channel)
    @channel_prefix.command(name="remove")
    @commands.check(require_server_admin)
    async def channel_remove_prefix(self,ctx,channel:discord.TextChannel): await self._remove(ctx,channel)
    @channel_prefix.command(name="list")
    @commands.check(require_server_admin)
    async def channel_list_prefix(self,ctx): await self._list(ctx)
    @channel_prefix.command(name="all")
    @commands.check(require_server_admin)
    async def channel_all_prefix(self,ctx): await self._all(ctx)

    channel_group=app_commands.Group(name="channel",description="Manage allowed bot channels")
    @channel_group.command(name="add",description="Add an allowed channel")
    async def channel_add_slash(self,interaction:discord.Interaction,channel:discord.TextChannel):
        if not await self._allowed(interaction): raise app_commands.CheckFailure("server administration required")
        await self._add(await commands.Context.from_interaction(interaction),channel)
    @channel_group.command(name="remove",description="Remove an allowed channel")
    async def channel_remove_slash(self,interaction:discord.Interaction,channel:discord.TextChannel):
        if not await self._allowed(interaction): raise app_commands.CheckFailure("server administration required")
        await self._remove(await commands.Context.from_interaction(interaction),channel)
    @channel_group.command(name="list",description="List allowed channels")
    async def channel_list_slash(self,interaction:discord.Interaction):
        if not await self._allowed(interaction): raise app_commands.CheckFailure("server administration required")
        await self._list(await commands.Context.from_interaction(interaction))
    @channel_group.command(name="all",description="Enable all-channel mode")
    async def channel_all_slash(self,interaction:discord.Interaction):
        if not await self._allowed(interaction): raise app_commands.CheckFailure("server administration required")
        await self._all(await commands.Context.from_interaction(interaction))

    async def _add(self,ctx,channel):
        await self._validate(ctx,channel)
        async with self.bot.db.session() as db: config=await self.bot.server_configs.get(db,ctx.guild.id); all_mode=bool(config and config.all_channels)
        async def apply():
            await self._validate(ctx,channel)
            async with self.bot.db.session() as db:
                if all_mode: await self.bot.server_configs.switch_to_restricted(db,ctx.guild.id,channel.id,ctx.author.id)
                else: await self.bot.server_configs.add(db,ctx.guild.id,channel.id,ctx.author.id)
        if all_mode: return await ctx.send("Switch from all channels to this restricted channel? / التحويل من كل القنوات إلى هذه القناة؟",view=ConfirmView(ctx.author.id,apply,self._allowed))
        await apply();await ctx.send(f"Allowed / مسموحة: {channel.mention}")
    async def _remove(self,ctx,channel):
        if channel.guild.id!=ctx.guild.id: raise ValueError("channel must belong to this server")
        async with self.bot.db.session() as db: await self.bot.server_configs.remove(db,ctx.guild.id,channel.id,ctx.author.id)
        await ctx.send(f"Removed / تمت الإزالة: {channel.mention}")
    async def _list(self,ctx):
        async with self.bot.db.session() as db: config=await self.bot.server_configs.get(db,ctx.guild.id); state=self.bot.server_configs.snapshot(config)
        if not state["setup_complete"]: message="Not configured / غير مُعد"
        elif state["all_channels"]: message="All accessible channels / جميع القنوات المتاحة"
        else: message="Allowed / المسموحة: "+", ".join(f"<#{x}>" for x in state["channel_ids"])
        await ctx.send(message,allowed_mentions=discord.AllowedMentions.none())
    async def _all(self,ctx):
        async def apply():
            async with self.bot.db.session() as db: await self.bot.server_configs.enable_all(db,ctx.guild.id,ctx.author.id)
        await ctx.send("Enable all-channel mode? Discord permissions still apply. / تفعيل وضع جميع القنوات؟",view=ConfirmView(ctx.author.id,apply,self._allowed))

async def setup(bot): await bot.add_cog(ServerAdminCog(bot))
