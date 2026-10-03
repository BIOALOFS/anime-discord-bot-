from types import SimpleNamespace
import pytest
from bot.database.models import CatalogImage,Character,CharacterWork,ReviewStatus,User,Work
from bot.services.collections import CollectionService
from bot.services.profile import DEFAULT_BANNER,ProfileRenderer

class FakeBot:
    def __init__(self):self.calls=0
    async def fetch_user(self,user_id):self.calls+=1;return SimpleNamespace(id=user_id,display_avatar=SimpleNamespace(url="https://discord/avatar"),banner=None)

@pytest.mark.asyncio
async def test_profile_uses_saved_settings_work_and_safe_fallbacks_without_mutation(db):
    service=CollectionService();user=await service.user(db,1);user.title="Title";user.bio="Bio";user.color="#112233";user.avatar_url="https://custom/avatar";user.avatar_review_status=ReviewStatus.approved;user.banner_url="https://custom/banner";user.banner_review_status=ReviewStatus.approved
    character=Character(name_original="Hero",category="game",review_status=ReviewStatus.approved);work=Work(title_original="Game",category="game",review_status=ReviewStatus.approved);db.add_all([character,work]);await db.flush();db.add(CharacterWork(character_id=character.id,work_id=work.id));db.add(CatalogImage(character_id=character.id,url="https://image/hero",is_default=True,review_status=ReviewStatus.approved));await service.add(db,1,character.id);await db.flush()
    discord_user=SimpleNamespace(id=1,display_name="Display",display_avatar=SimpleNamespace(url="https://discord/avatar"),banner=None)
    renderer=ProfileRenderer(FakeBot());card=await renderer.render(db,discord_user,1,1)
    assert card.embed.author.icon_url=="https://custom/avatar" and card.embed.thumbnail.url=="https://custom/banner"
    assert "Title" in card.embed.description and "Bio" in card.embed.description and any(field.value=="Game" for field in card.embed.fields)
    assert user.avatar_url=="https://custom/avatar" and user.banner_url=="https://custom/banner"

@pytest.mark.asyncio
async def test_unapproved_appearance_and_missing_banner_use_safe_fallback(db):
    service=CollectionService();user=await service.user(db,2);user.avatar_url="https://pending/avatar";user.banner_url="https://pending/banner"
    character=Character(name_original="Hero",category="game",review_status=ReviewStatus.approved);db.add(character);await db.flush();await service.add(db,2,character.id)
    discord_user=SimpleNamespace(id=2,display_name="Display",display_avatar=SimpleNamespace(url="https://discord/avatar"),banner=None);bot=FakeBot();card=await ProfileRenderer(bot).render(db,discord_user,2,1)
    assert card.embed.author.icon_url=="https://discord/avatar" and card.embed.thumbnail.url==DEFAULT_BANNER and bot.calls==1
