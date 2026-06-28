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
intents.members = True
intents.guilds = True

bot = commands.Bot(command_prefix="!", intents=intents)
DATA_FILE = "targets.json"

def load_targets():
    if os.path.exists(DATA_FILE):
        try:
            with open(DATA_FILE, "r") as f:
                data = json.load(f)
                return {int(k): v for k, v in data.items()}
        except Exception as e:
            print(f"Error loading targets.json: {e}")
            return {}
    return {}

targets = load_targets()

# Config & State
settings = {
    "tracking_active": True,   # Toggle the automated follow system
    "current_target": None,
    "force_lock": False,      
    "locked_channel_id": None 
}

@bot.event
async def on_ready():
    print(f"========================================")
    print(f"🤖 Bot Status: ONLINE as {bot.user.name}")
    print(f"📋 Mode: Clean Follow (Join/Leave/Lock)")
    print(f"========================================")
    try:
        await bot.tree.sync()
        print(f"Panel Command Synced Successfully.")
    except Exception as e:
        print(f"Sync failed: {e}")

# Heavy Anti-Disconnect / Force Lock Mechanism
async def keep_locked(voice_channel: discord.VoiceChannel):
    while settings["force_lock"] and settings["locked_channel_id"] == voice_channel.id:
        try:
            if not voice_channel.guild.voice_client:
                await voice_channel.connect()
            elif voice_channel.guild.voice_client.channel.id != voice_channel.id:
                await voice_channel.guild.voice_client.move_to(voice_channel)
        except Exception:
            pass
        await asyncio.sleep(0.3) 

# Join, Leave, Move & Priority Listener
@bot.event
async def on_voice_state_update(member, before, after):
    # 1. Handle Force Lock for the Bot
    if member.id == bot.user.id:
        if settings["force_lock"] and settings["locked_channel_id"]:
            locked_channel = bot.get_channel(settings["locked_channel_id"])
            if locked_channel and (after.channel is None or after.channel.id != locked_channel.id):
                asyncio.create_task(keep_locked(locked_channel))
        return

    # If Force Lock is on or system is disabled, completely freeze automated tracking
    if settings["force_lock"] or not settings["tracking_active"]:
        return

    # 2. Automated Follow System (Join, Move, Leave)
    if member.id in targets:
        user_priority = targets[member.id].get("priority", 3)
        current_target_id = settings["current_target"]

        # CASE A: Target joins or moves to a voice channel
        if after.channel is not None:
            # If bot is not following anyone right now, join them
            if current_target_id is None:
                settings["current_target"] = member.id
                if not member.guild.voice_client:
                    await after.channel.connect()
                else:
                    await member.guild.voice_client.move_to(after.channel)
            
            # If target moves channel while being followed, stay with them
            elif current_target_id == member.id:
                if member.guild.voice_client and member.guild.voice_client.channel != after.channel:
                    await member.guild.voice_client.move_to(after.channel)

            # Priority handling if another whitelisted user switches channels
            elif current_target_id != member.id:
                current_priority = targets.get(current_target_id, {}).get("priority", 99)
                if user_priority < current_priority:
                    print(f"[PRIORITY] Switching tracking to higher priority user: {member.name}")
                    settings["current_target"] = member.id
                    if member.guild.voice_client:
                        await member.guild.voice_client.move_to(after.channel)
                    else:
                        await after.channel.connect()

        # CASE B: Target leaves voice entirely
        elif after.channel is None and current_target_id == member.id:
            settings["current_target"] = None
            if member.guild.voice_client:
                await member.guild.voice_client.disconnect()


# --- Interactive Control Panel View ---
class ControlPanelView(View):
    def __init__(self):
        super().__init__(timeout=None)
        self.update_buttons()

    def update_buttons(self):
        self.clear_items()
        
        # Button 1: Toggle Automated Tracking System
        track_style = discord.ButtonStyle.success if settings["tracking_active"] else discord.ButtonStyle.danger
        track_label = "🟢 Auto-Follow: ON" if settings["tracking_active"] else "🔴 Auto-Follow: OFF"
        track_btn = Button(label=track_label, style=track_style, custom_id="btn_track_toggle", row=0)
        track_btn.callback = self.track_toggle_callback
        self.add_item(track_btn)

        # Button 2: Lock Bot inside current VC
        lock_style = discord.ButtonStyle.danger if settings["force_lock"] else discord.ButtonStyle.secondary
        lock_label = "🔒 Force Unlock" if settings["force_lock"] else "🔒 Force Lock VC"
        lock_btn = Button(label=lock_label, style=lock_style, custom_id="btn_lock", row=1)
        lock_btn.callback = self.lock_callback
        self.add_item(lock_btn)

        # Button 3: Stop Everything and Leave VC
        stop_btn = Button(label="🛑 Disconnect & Clear", style=discord.ButtonStyle.primary, custom_id="btn_stop", row=1)
        stop_btn.callback = self.stop_callback
        self.add_item(stop_btn)

    async def interaction_check(self, interaction: discord.Interaction) -> bool:
        user_id = interaction.user.id
        if user_id in targets and targets[user_id].get("is_admin", False):
            return True
        await interaction.response.send_message("❌ Access Denied: Unauthorized.", ephemeral=True)
        return False

    async def track_toggle_callback(self, interaction: discord.Interaction):
        settings["tracking_active"] = not settings["tracking_active"]
        
        if not settings["tracking_active"]:
            settings["current_target"] = None
            for vc in bot.voice_clients:
                if vc.guild == interaction.guild:
                    await vc.disconnect()
            msg = "🔴 **Auto-Follow Mode completely deactivated.**"
        else:
            msg = "🟢 **Auto-Follow Mode activated.** Bot will now naturally join/leave/move with targets."

        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(msg, ephemeral=True)

    async def lock_callback(self, interaction: discord.Interaction):
        if not settings["force_lock"]:
            if not interaction.user.voice:
                await interaction.response.send_message("❌ You must be in a Voice Channel to lock the bot!", ephemeral=True)
                return
            settings["force_lock"] = True
            settings["locked_channel_id"] = interaction.user.voice.channel.id
            asyncio.create_task(keep_locked(interaction.user.voice.channel))
            msg = f"🔒 **Bot hard-locked inside:** {interaction.user.voice.channel.name}. It cannot be moved or kicked!"
        else:
            settings["force_lock"] = False
            settings["locked_channel_id"] = None
            for vc in bot.voice_clients:
                if vc.guild == interaction.guild:
                    await vc.disconnect()
            msg = "🔓 **Force Lock Deactivated.** Bot released."

        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(msg, ephemeral=True)

    async def stop_callback(self, interaction: discord.Interaction):
        settings["force_lock"] = False
        settings["locked_channel_id"] = None
        settings["current_target"] = None
        for vc in bot.voice_clients:
            if vc.guild == interaction.guild:
                await vc.disconnect()
        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send("🛑 System cleared and Bot disconnected.", ephemeral=True)

def get_status_embed(guild):
    embed = discord.Embed(title="🎛️ Follow & Lock Control Panel", color=discord.Color.blue())
    
    current_target = "None (Idle)"
    if settings["current_target"]:
        user = guild.get_member(settings["current_target"])
        if user: current_target = f"👥 Following: {user.name}"

    if settings["force_lock"] and settings["locked_channel_id"]:
        ch = guild.get_channel(settings["locked_channel_id"])
        status_text = f"🔒 HARD-LOCKED IN: **{ch.name if ch else 'Unknown'}**"
    elif settings["tracking_active"]:
        status_text = "🟢 Tracking Targets Automatically" if settings["current_target"] else "💤 Active & Waiting for Targets"
    else:
        status_text = "🔴 Tracking System: Disabled"

    embed.add_field(name="System Status", value=status_text, inline=False)
    embed.add_field(name="Active Tracking", value=current_target, inline=True)
    
    return embed

# --- The Single Command Allowed ---
@bot.tree.command(name="panel", description="Open the interactive control panel.")
async def panel(interaction: discord.Interaction):
    user_id = interaction.user.id
    if user_id not in targets or not targets[user_id].get("is_admin", False):
        await interaction.response.send_message("❌ Unauthorized.", ephemeral=True)
        return
        
    view = ControlPanelView()
    await interaction.response.send_message(embed=get_status_embed(interaction.guild), view=view, ephemeral=True)

bot.run(TOKEN)

