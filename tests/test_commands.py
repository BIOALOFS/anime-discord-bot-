import discord
import pytest
from discord.ext import commands

@pytest.mark.asyncio
async def test_all_extensions_load_and_required_slash_groups_exist():
    bot=commands.Bot(command_prefix="!",intents=discord.Intents.none(),help_command=None)
    for extension in ("bot.commands.core","bot.commands.extras","bot.commands.maintainer","bot.commands.server_admin"):await bot.load_extension(extension)
    roots={command.name:command for command in bot.tree.get_commands()}
    assert {"profile","find","image","settings","search","nickname","note","setup","channel"}<=set(roots)
    assert {x.name for x in roots["image"].commands}>={"add","list","select","remove","reset"}
    assert {x.name for x in roots["settings"].commands}>={"avatar","banner"}
    await bot.close()
