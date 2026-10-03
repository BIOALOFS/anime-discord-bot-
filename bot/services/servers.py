from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any
import discord
from sqlalchemy import delete, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload
from bot.database.models import AllowedChannel, ServerAuditLog, ServerConfiguration


@dataclass(slots=True)
class ChannelAccess:
    allowed: bool
    setup_complete: bool
    channel_ids: list[int]
    all_channels: bool


class ServerConfigurationService:
    async def get(self, db: AsyncSession, guild_id: int) -> ServerConfiguration | None:
        return (await db.scalars(select(ServerConfiguration).options(selectinload(ServerConfiguration.channels)).where(ServerConfiguration.guild_id == guild_id))).first()

    @staticmethod
    def snapshot(config: ServerConfiguration | None) -> dict[str, Any]:
        if not config:
            return {"setup_complete": False, "all_channels": False, "channel_ids": []}
        return {"setup_complete": config.setup_complete, "all_channels": config.all_channels, "channel_ids": sorted(x.channel_id for x in config.channels)}

    async def _audit(self, db, actor_id, guild_id, action, previous, current):
        db.add(ServerAuditLog(actor_id=actor_id, guild_id=guild_id, action=action, previous=previous, current=current))

    async def replace(self, db: AsyncSession, guild_id: int, channel_id: int, actor_id: int):
        config = await self.get(db, guild_id); previous = self.snapshot(config)
        if not config: config = ServerConfiguration(guild_id=guild_id); db.add(config); await db.flush()
        await db.execute(delete(AllowedChannel).where(AllowedChannel.guild_id == guild_id))
        config.setup_complete = True; config.all_channels = False; config.changed_by = actor_id; config.changed_at = datetime.now(timezone.utc)
        db.add(AllowedChannel(guild_id=guild_id, channel_id=channel_id, added_by=actor_id)); await db.flush(); await db.refresh(config, ["channels"])
        await self._audit(db, actor_id, guild_id, "setup_replace", previous, self.snapshot(config)); return config

    async def add(self, db, guild_id, channel_id, actor_id):
        config = await self.get(db, guild_id); previous = self.snapshot(config)
        if not config: config = ServerConfiguration(guild_id=guild_id); db.add(config); await db.flush()
        if config.all_channels: raise ValueError("confirmation is required to leave all-channel mode")
        if channel_id not in {x.channel_id for x in config.channels}: db.add(AllowedChannel(guild_id=guild_id, channel_id=channel_id, added_by=actor_id))
        config.setup_complete=True;config.changed_by=actor_id;config.changed_at=datetime.now(timezone.utc);await db.flush();await db.refresh(config,["channels"])
        await self._audit(db,actor_id,guild_id,"channel_add",previous,self.snapshot(config));return config

    async def switch_to_restricted(self, db, guild_id, channel_id, actor_id):
        config=await self.get(db,guild_id);previous=self.snapshot(config)
        if not config: config=ServerConfiguration(guild_id=guild_id);db.add(config);await db.flush()
        config.all_channels=False;config.setup_complete=True;config.changed_by=actor_id;config.changed_at=datetime.now(timezone.utc)
        if channel_id not in {x.channel_id for x in config.channels}: db.add(AllowedChannel(guild_id=guild_id,channel_id=channel_id,added_by=actor_id))
        await db.flush();await db.refresh(config,["channels"]);await self._audit(db,actor_id,guild_id,"all_to_restricted",previous,self.snapshot(config));return config

    async def remove(self, db, guild_id, channel_id, actor_id):
        config=await self.get(db,guild_id)
        if not config or config.all_channels: raise ValueError("server is not in restricted-channel mode")
        ids={x.channel_id for x in config.channels}
        if channel_id not in ids: raise ValueError("channel is not configured")
        if len(ids)==1: raise ValueError("cannot remove the final allowed channel; add a replacement or enable all-channel mode")
        previous=self.snapshot(config);await db.execute(delete(AllowedChannel).where(AllowedChannel.guild_id==guild_id,AllowedChannel.channel_id==channel_id))
        config.changed_by=actor_id;config.changed_at=datetime.now(timezone.utc);await db.flush();await db.refresh(config,["channels"]);await self._audit(db,actor_id,guild_id,"channel_remove",previous,self.snapshot(config));return config

    async def enable_all(self, db, guild_id, actor_id):
        config=await self.get(db,guild_id);previous=self.snapshot(config)
        if not config: config=ServerConfiguration(guild_id=guild_id);db.add(config);await db.flush()
        config.setup_complete=True;config.all_channels=True;config.changed_by=actor_id;config.changed_at=datetime.now(timezone.utc)
        await self._audit(db,actor_id,guild_id,"all_channels_enable",previous,self.snapshot(config));return config

    async def access(self, db, guild, channel) -> ChannelAccess:
        config=await self.get(db,guild.id)
        if not config or not config.setup_complete: return ChannelAccess(False,False,[],False)
        if config.all_channels: return ChannelAccess(True,True,[],True)
        # Delete stale rows. A restricted configuration with no live channels becomes unconfigured, never unrestricted.
        live_ids={x.channel_id for x in config.channels if guild.get_channel(x.channel_id) is not None}
        stale={x.channel_id for x in config.channels}-live_ids
        if stale:
            await db.execute(delete(AllowedChannel).where(AllowedChannel.guild_id==guild.id,AllowedChannel.channel_id.in_(stale)))
            if not live_ids: config.setup_complete=False
        effective_id=getattr(getattr(channel,"parent",None),"id",None) if isinstance(channel,discord.Thread) else getattr(channel,"id",None)
        return ChannelAccess(effective_id in live_ids,bool(live_ids),sorted(live_ids),False)

    @staticmethod
    def validate_channel(guild, channel, bot_member) -> None:
        if channel.guild.id != guild.id: raise ValueError("channel must belong to this server")
        if not isinstance(channel,(discord.TextChannel,discord.Thread)): raise ValueError("select a text channel or thread")
        permissions=channel.permissions_for(bot_member)
        required={"View Channel":permissions.view_channel,"Send Messages":permissions.send_messages,"Embed Links":permissions.embed_links,"Attach Files":permissions.attach_files}
        if isinstance(channel,discord.Thread): required["Send Messages in Threads"]=permissions.send_messages_in_threads
        missing=[name for name,value in required.items() if not value]
        if missing: raise ValueError("Bot is missing: "+", ".join(missing))
