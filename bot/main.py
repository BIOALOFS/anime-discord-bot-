from __future__ import annotations
import asyncio, logging
import aiohttp, discord
from discord import app_commands
from discord.ext import commands
from .config import Settings
from .database import Database
from .providers import AniListProvider, ProviderRegistry, TMDBProvider, WikidataProvider
from .providers.http import CachedHTTP
from .services import CatalogImportService, ImageService, ProfileRenderer, SearchService
from .services.permissions import TrustedUsers
from .services.servers import ServerConfigurationService
from .services.collections import CollectionService

ADMIN_ROOTS={"setup","channel","reviewsearch","reviewqueue","review","mergecharacters","editcatalog","editwork","reviewwork","replacecatalogimage","reviewimage","reviewappearance","reports","providerstatus"}

class RestrictedTree(app_commands.CommandTree):
    async def interaction_check(self,interaction):
        root=interaction.command.root_parent.name if interaction.command and interaction.command.root_parent else (interaction.command.name if interaction.command else "")
        if root in ADMIN_ROOTS: return True
        allowed,message=await interaction.client.ordinary_access(interaction.guild,interaction.channel)
        if not allowed:
            if not interaction.response.is_done(): await interaction.response.send_message(message,ephemeral=True,allowed_mentions=discord.AllowedMentions.none())
            return False
        return True
    async def on_error(self,interaction,error):
        message=f"Error / خطأ: {getattr(error,'original',error)}"
        if interaction.response.is_done(): await interaction.followup.send(message,ephemeral=True)
        else: await interaction.response.send_message(message,ephemeral=True)

class AnpotBot(commands.Bot):
    def __init__(self,settings):
        intents=discord.Intents.default(); intents.message_content=True
        super().__init__(command_prefix=settings.prefix,intents=intents,help_command=None,allowed_mentions=discord.AllowedMentions.none(),strip_after_prefix=True,tree_cls=RestrictedTree)
        self.settings=settings; self.db=Database(settings.database_url)
        self.collection_service=CollectionService()
        self.trust=TrustedUsers(settings.trusted_users_file);self.server_configs=ServerConfigurationService()
        self.check(self._prefix_access)
    async def ordinary_access(self,guild,channel):
        if guild is None: return False,"Commands are unavailable in direct messages / الأوامر غير متاحة في الرسائل الخاصة"
        async with self.db.session() as db: access=await self.server_configs.access(db,guild,channel)
        if not access.setup_complete: return False,f"Server setup is required. An administrator can run `{self.settings.prefix}setup #channel` or `/setup`. / يلزم إعداد الخادم."
        channels=", ".join(f"<#{x}>" for x in access.channel_ids)
        if access.allowed:
            try: self.server_configs.validate_channel(guild,channel,guild.me)
            except ValueError as exc: return False,f"Bot permissions are insufficient here: {exc} / صلاحيات البوت غير كافية"
        return access.allowed,(f"Use an allowed channel: {channels} / استخدم قناة مسموحة" if channels else "This channel is not available / هذه القناة غير متاحة")
    async def _prefix_access(self,ctx):
        root=ctx.command.root_parent.name if ctx.command and ctx.command.root_parent else (ctx.command.name if ctx.command else "")
        if root in ADMIN_ROOTS: return True
        allowed,message=await self.ordinary_access(ctx.guild,ctx.channel)
        if not allowed:
            async with self.db.session() as db: config=await self.server_configs.get(db,ctx.guild.id) if ctx.guild else None
            if not config or not config.setup_complete: await ctx.send(message,allowed_mentions=discord.AllowedMentions.none())
            ctx.access_gate_handled=True
            raise commands.CheckFailure(message)
        return True
    async def on_command_error(self,ctx,error):
        if getattr(ctx,"access_gate_handled",False): return
        original=getattr(error,"original",error)
        await ctx.send(f"Error / خطأ: {original}",allowed_mentions=discord.AllowedMentions.none())
    async def setup_hook(self):
        await self.trust.reload_if_changed(force=True)
        self.http=aiohttp.ClientSession(headers={"User-Agent":self.settings.user_agent})
        cached=CachedHTTP(self.http,self.settings.cache_ttl)
        self.providers=ProviderRegistry([AniListProvider(cached),TMDBProvider(cached,self.settings.tmdb_token),WikidataProvider(cached)])
        self.search=SearchService(self.providers); self.catalog=CatalogImportService(); self.images=ImageService(self.http,self.settings.max_image_bytes);self.profiles=ProfileRenderer(self)
        await self.load_extension("bot.commands.core"); await self.load_extension("bot.commands.extras"); await self.load_extension("bot.commands.maintainer"); await self.load_extension("bot.commands.server_admin")
        if self.settings.test_guild_id:
            guild=discord.Object(self.settings.test_guild_id); self.tree.copy_global_to(guild=guild); await self.tree.sync(guild=guild)
        else: await self.tree.sync()
    async def close(self): await self.http.close(); await self.db.close(); await super().close()

def main():
    logging.basicConfig(level=logging.INFO)
    settings=Settings(); settings.validate(); AnpotBot(settings).run(settings.discord_token,log_handler=None)
if __name__=="__main__": main()
