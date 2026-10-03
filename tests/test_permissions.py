import json
from types import SimpleNamespace
import pytest
from bot.services.permissions import TrustedUsers


@pytest.mark.asyncio
async def test_global_and_server_scopes_are_independent_and_hot_reload(tmp_path):
    path=tmp_path/"trusted_users.json"
    path.write_text(json.dumps({"global_admins":["1"],"servers":{"10":["2"]}}))
    trust=TrustedUsers(path);await trust.reload_if_changed(force=True)
    guild=SimpleNamespace(id=10,owner_id=3)
    member=lambda ident,manage=False: SimpleNamespace(id=ident,guild_permissions=SimpleNamespace(manage_guild=manage))
    assert await trust.is_global(1)
    assert await trust.can_manage_server(member(1),guild)
    assert await trust.can_manage_server(member(2),guild)
    assert not await trust.can_manage_server(member(2),SimpleNamespace(id=11,owner_id=99))
    assert await trust.can_manage_server(member(3),guild)
    assert await trust.can_manage_server(member(4,True),guild)
    assert not await trust.is_global(2)  # server trust cannot administer the shared catalog
    path.write_text(json.dumps({"global_admins":[],"servers":{}}))
    assert not await trust.is_global(1)  # revoked without restart


@pytest.mark.asyncio
async def test_invalid_or_missing_file_fails_closed(tmp_path):
    path=tmp_path/"trusted_users.json";path.write_text('{"global_admins":[1],"servers":{}}')
    trust=TrustedUsers(path)
    assert not await trust.is_global(1)
    path.write_text(json.dumps({"global_admins":["1"],"servers":{}}));assert await trust.is_global(1)
    path.unlink();assert not await trust.is_global(1)

