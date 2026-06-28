import discord
from discord.ext import commands
from discord.ui import Button, View
import asyncio
import json
import os
from dotenv import load_dotenv

load_dotenv()
TOKEN = os.getenv("DISCORD_TOKEN")

intents = discord.Intents.default()
intents.voice_states = True
intents.guilds = True
intents.members = True

bot = commands.Bot(command_prefix="!", intents=intents)
DATA_FILE = "targets.json"

def load_targets():
    if os.path.exists(DATA_FILE):
        try:
            with open(DATA_FILE, "r") as f:
                data = json.load(f)
                return {int(k): v for k, v in data.items()}
        except Exception:
            return {}
    return {}

targets = load_targets()

# إعدادات البوت الحالية
settings = {
    "auto_rejoin": True,        # ميزة إعادة الدخول عند الطرد
    "force_lock": False,        # ميزة التثبيت ومنع الطرد
    "current_channel_id": None  # القناة الصوتية المتواجد بها البوت حالياً
}

@bot.event
async def on_ready():
    print(f"========================================")
    print(f"🤖 Bot is ONLINE as {bot.user.name}")
    print(f"📋 Mode: Manual Control (Join / Leave / Pin / Rejoin)")
    print(f"========================================")
    await bot.tree.sync()

# حلقة التثبيت الإجباري (Pin) لمنع أي طرد أو سحب
async def keep_pinned(voice_channel: discord.VoiceChannel):
    while settings["force_lock"] and settings["current_channel_id"] == voice_channel.id:
        try:
            if not voice_channel.guild.voice_client:
                await voice_channel.connect()
            elif voice_channel.guild.voice_client.channel.id != voice_channel.id:
                await voice_channel.guild.voice_client.move_to(voice_channel)
        except Exception:
            pass
        await asyncio.sleep(0.3)

@bot.event
async def on_voice_state_update(member, before, after):
    # مراقبة ما يحدث للبوت نفسه فقط
    if member.id == bot.user.id:
        # إذا تم تفعيل التثبيت الإجباري (Pin) وحاول أحد طرده
        if settings["force_lock"] and settings["current_channel_id"]:
            locked_channel = bot.get_channel(settings["current_channel_id"])
            if locked_channel and (after.channel is None or after.channel.id != locked_channel.id):
                asyncio.create_task(keep_pinned(locked_channel))
            return

        # إذا تم طرد البوت وكانت ميزة الـ Auto Rejoin مفعّلة (بدون تثبيت إجباري)
        if before.channel is not None and after.channel is None and settings["auto_rejoin"] and not settings["force_lock"]:
            if settings["current_channel_id"] == before.channel.id:
                print(f"[REJOIN] Bot was disconnected. Rejoining {before.channel.name}...")
                try:
                    await before.channel.connect()
                except Exception:
                    pass

# --- لوحة التحكم بأزرار رمادية موحدة (Secondary Style) ---
class ControlPanelView(View):
    def __init__(self):
        super().__init__(timeout=None)
        self.update_buttons()

    def update_buttons(self):
        self.clear_items()
        
        # جميع الأزرار تستخدم ميزة discord.ButtonStyle.secondary لتبدو باللون الرمادي وبدون ألوان أخرى
        
        # 1. زر الانضمام Join
        join_btn = Button(label="📥 Join My VC", style=discord.ButtonStyle.secondary, custom_id="btn_join", row=0)
        join_btn.callback = self.join_callback
        self.add_item(join_btn)

        # 2. زر الخروج Leave
        leave_btn = Button(label="📤 Leave VC", style=discord.ButtonStyle.secondary, custom_id="btn_leave", row=0)
        leave_btn.callback = self.leave_callback
        self.add_item(leave_btn)

        # 3. زر التثبيت Pin / Unpin
        lock_label = "🔓 Unpin Bot" if settings["force_lock"] else "🔒 Pin Bot in VC"
        lock_btn = Button(label=lock_label, style=discord.ButtonStyle.secondary, custom_id="btn_lock", row=1)
        lock_btn.callback = self.lock_callback
        self.add_item(lock_btn)

        # 4. زر التبديل الخاص بالـ Auto Rejoin
        rejoin_label = "🔄 Auto-Rejoin: ON" if settings["auto_rejoin"] else "🔄 Auto-Rejoin: OFF"
        rejoin_btn = Button(label=rejoin_label, style=discord.ButtonStyle.secondary, custom_id="btn_rejoin", row=1)
        rejoin_btn.callback = self.rejoin_callback
        self.add_item(rejoin_btn)

    async def interaction_check(self, interaction: discord.Interaction) -> bool:
        # التحقق من أن الشخص لديه صلاحية أدمين من ملف JSON
        user_id = interaction.user.id
        if user_id in targets and targets[user_id].get("is_admin", False):
            return True
        await interaction.response.send_message("❌ Access Denied: Unauthorized.", ephemeral=True)
        return False

    async def join_callback(self, interaction: discord.Interaction):
        if not interaction.user.voice:
            await interaction.response.send_message("❌ You must be in a Voice Channel first!", ephemeral=True)
            return
        
        channel = interaction.user.voice.channel
        settings["current_channel_id"] = channel.id
        
        if not interaction.guild.voice_client:
            await channel.connect()
        else:
            await interaction.guild.voice_client.move_to(channel)
            
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(f"📥 Joined: **{channel.name}**", ephemeral=True)

    async def leave_callback(self, interaction: discord.Interaction):
        settings["force_lock"] = False
        settings["current_channel_id"] = None
        
        for vc in bot.voice_clients:
            if vc.guild == interaction.guild:
                await vc.disconnect()
                
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send("📤 Disconnected from voice channel.", ephemeral=True)

    async def lock_callback(self, interaction: discord.Interaction):
        if not settings["force_lock"]:
            if not interaction.guild.voice_client:
                await interaction.response.send_message("❌ Bot is not in any Voice Channel to pin it!", ephemeral=True)
                return
            settings["force_lock"] = True
            settings["current_channel_id"] = interaction.guild.voice_client.channel.id
            asyncio.create_task(keep_pinned(interaction.guild.voice_client.channel))
            msg = "🔒 **Bot Pinned!** Kicking or moving the bot is now disabled."
        else:
            settings["force_lock"] = False
            msg = "🔓 **Bot Unpinned!** Safe to move or disconnect."

        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(msg, ephemeral=True)

    async def rejoin_callback(self, interaction: discord.Interaction):
        settings["auto_rejoin"] = not settings["auto_rejoin"]
        state = "ENABLED" if settings["auto_rejoin"] else "DISABLED"
        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(f"🔄 Auto-Rejoin is now **{state}**", ephemeral=True)

def get_status_embed(guild):
    embed = discord.Embed(title="🎛️ Manual Voice Control Panel", color=discord.Color.blue())
    
    current_vc = "Not Connected"
    if guild.voice_client:
        current_vc = f"🔊 {guild.voice_client.channel.name}"

    if settings["force_lock"]:
        status = "🔒 Pinned (Anti-Disconnect Active)"
    else:
        status = "🟢 Connected & Standard" if guild.voice_client else "💤 Disconnected"

    embed.add_field(name="Current Channel", value=current_vc, inline=True)
    embed.add_field(name="Pin Status", value=status, inline=True)
    embed.add_field(name="Auto-Rejoin", value="✅ ON" if settings["auto_rejoin"] else "❌ OFF", inline=False)
    
    return embed

# الأمر الوحيد المتاح في البوت بالكامل
@bot.tree.command(name="panel", description="Open the manual voice controller panel.")
async def panel(interaction: discord.Interaction):
    user_id = interaction.user.id
    if user_id not in targets or not targets[user_id].get("is_admin", False):
        await interaction.response.send_message("❌ Unauthorized.", ephemeral=True)
        return
        
    view = ControlPanelView()
    await interaction.response.send_message(embed=get_status_embed(interaction.guild), view=view, ephemeral=True)

bot.run(TOKEN)
