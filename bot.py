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
    "traffic_active": True,   # 🟢 Global Switch for Auto-Join Tracking (New)
    "rejoin": True,
    "delay": 0.6,
    "current_target": None,
    "force_lock": False,      
    "locked_channel_id": None 
}

is_hopping = False

@bot.event
async def on_ready():
    print(f"========================================")
    print(f"🤖 Bot Status: ONLINE as {bot.user.name}")
    print(f"📋 Loaded {len(targets)} users from targets.json")
    print(f"========================================")
    try:
        synced = await bot.tree.sync()
        print(f"Slash commands synced: {len(synced)}")
    except Exception as e:
        print(f"Sync failed: {e}")

# Core Traffic Mechanic
async def start_traffic(member: discord.Member, voice_channel: discord.VoiceChannel):
    global is_hopping
    if is_hopping or settings["force_lock"] or not settings["traffic_active"]:
        return
    
    is_hopping = True
    settings["current_target"] = member.id
    print(f"[TRAFFIC] Starting on {member.name} in {voice_channel.name}")

    while is_hopping and settings["current_target"] == member.id and not settings["force_lock"] and settings["traffic_active"]:
        if member.voice is None or member.voice.channel != voice_channel:
            break
            
        try:
            vc = await voice_channel.connect()
            await asyncio.sleep(settings["delay"])
            await vc.disconnect()
            await asyncio.sleep(settings["delay"])
        except Exception:
            await asyncio.sleep(settings["delay"])
            continue

    is_hopping = False
    settings["current_target"] = None

# Heavy Anti-Disconnect / Force Lock Mechanism
async def keep_locked(voice_channel: discord.VoiceChannel):
    print(f"[LOCK] Force lock activated in {voice_channel.name}")
    while settings["force_lock"] and settings["locked_channel_id"] == voice_channel.id:
        try:
            if not voice_channel.guild.voice_client:
                await voice_channel.connect()
            elif voice_channel.guild.voice_client.channel.id != voice_channel.id:
                await voice_channel.guild.voice_client.move_to(voice_channel)
        except Exception:
            pass
        await asyncio.sleep(0.3) 

# Auto-Join & Priority & Rejoin Listener
@bot.event
async def on_voice_state_update(member, before, after):
    global is_hopping
    
    # 1. Handle Bot Disconnects / Force Lock Bypass Prevention
    if member.id == bot.user.id:
        if settings["force_lock"] and settings["locked_channel_id"]:
            locked_channel = bot.get_channel(settings["locked_channel_id"])
            if locked_channel and (after.channel is None or after.channel.id != locked_channel.id):
                asyncio.create_task(keep_locked(locked_channel))
            return

        # Regular Rejoin for Traffic Mode (Only works if traffic is globally active)
        if before.channel is not None and after.channel is None and settings["rejoin"] and settings["traffic_active"]:
            current_target_id = settings["current_target"]
            if current_target_id and before.channel:
                target_member = before.channel.guild.get_member(current_target_id)
                if target_member and target_member.voice and target_member.voice.channel == before.channel:
                    is_hopping = False 
                    asyncio.create_task(start_traffic(target_member, before.channel))
        return

    # If Force Lock is active or Traffic System is turned OFF globally, do nothing.
    if settings["force_lock"] or not settings["traffic_active"]:
        return

    # 2. Automated Traffic Trigger
    if member.id in targets and after.channel is not None:
        user_data = targets[member.id]
        member_priority = user_data.get("priority", 3)
        current_target_id = settings["current_target"]

        if current_target_id is None:
            asyncio.create_task(start_traffic(member, after.channel))
            
        elif current_target_id != member.id:
            current_priority = targets.get(current_target_id, {}).get("priority", 99)
            if member_priority < current_priority:
                print(f"[PRIORITY] Higher priority user ({member.name}) detected! Switching...")
                is_hopping = False 
                for vc in bot.voice_clients:
                    if vc.guild == member.guild:
                        await vc.disconnect()
                await asyncio.sleep(0.4)
                asyncio.create_task(start_traffic(member, after.channel))

    if member.id == settings["current_target"] and after.channel is None:
        is_hopping = False

# --- Interactive Control Panel View ---
class ControlPanelView(View):
    def __init__(self):
        super().__init__(timeout=None)
        self.update_buttons()

    def update_buttons(self):
        self.clear_items()
        
        # 🟢 Global Traffic Toggle Button (New)
        traffic_style = discord.ButtonStyle.success if settings["traffic_active"] else discord.ButtonStyle.danger
        traffic_label = "🟢 Auto-Traffic: ON" if settings["traffic_active"] else "🔴 Auto-Traffic: OFF"
        traffic_btn = Button(label=traffic_label, style=traffic_style, custom_id="btn_traffic_toggle")
        traffic_btn.callback = self.traffic_toggle_callback
        self.add_item(traffic_btn)

        # Lock Button
        lock_style = discord.ButtonStyle.danger if settings["force_lock"] else discord.ButtonStyle.secondary
        lock_label = "🔒 Force Unlock" if settings["force_lock"] else "🔒 Force Lock VC"
        lock_btn = Button(label=lock_label, style=lock_style, custom_id="btn_lock", row=1)
        lock_btn.callback = self.lock_callback
        self.add_item(lock_btn)

        # Rejoin Button
        rejoin_style = discord.ButtonStyle.success if settings["rejoin"] else discord.ButtonStyle.danger
        rejoin_label = "Rejoin: ENABLED" if settings["rejoin"] else "Rejoin: DISABLED"
        rejoin_btn = Button(label=rejoin_label, style=rejoin_style, custom_id="btn_rejoin", row=1)
        rejoin_btn.callback = self.rejoin_callback
        self.add_item(rejoin_btn)

        # Stop Button
        stop_btn = Button(label="🛑 Stop & Reset All", style=discord.ButtonStyle.primary, custom_id="btn_stop", row=1)
        stop_btn.callback = self.stop_callback
        self.add_item(stop_btn)

    async def interaction_check(self, interaction: discord.Interaction) -> bool:
        user_id = interaction.user.id
        if user_id in targets and targets[user_id].get("is_admin", False):
            return True
        await interaction.response.send_message("❌ Access Denied: Unauthorized.", ephemeral=True)
        return False

    async def traffic_toggle_callback(self, interaction: discord.Interaction):
        global is_hopping
        settings["traffic_active"] = not settings["traffic_active"]
        
        # If disabled while running, shut down active hopping immediately
        if not settings["traffic_active"]:
            is_hopping = False
            settings["current_target"] = None
            for vc in bot.voice_clients:
                if vc.guild == interaction.guild:
                    await vc.disconnect()
            msg = "🔴 **Auto-Join Traffic System has been completely Deactivated.**"
        else:
            msg = "🟢 **Auto-Join Traffic System is now Active.** The bot will watch for whitelisted targets."

        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(msg, ephemeral=True)

    async def lock_callback(self, interaction: discord.Interaction):
        global is_hopping
        if not settings["force_lock"]:
            if not interaction.user.voice:
                await interaction.response.send_message("❌ You must be in a Voice Channel to lock the bot!", ephemeral=True)
                return
            is_hopping = False
            settings["force_lock"] = True
            settings["locked_channel_id"] = interaction.user.voice.channel.id
            asyncio.create_task(keep_locked(interaction.user.voice.channel))
            msg = f"🔒 **Bot heavily locked inside:** {interaction.user.voice.channel.name}."
        else:
            settings["force_lock"] = False
            settings["locked_channel_id"] = None
            for vc in bot.voice_clients:
                if vc.guild == interaction.guild:
                    await vc.disconnect()
            msg = "🔓 **Force Lock Deactivated.**"

        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send(msg, ephemeral=True)

    async def rejoin_callback(self, interaction: discord.Interaction):
        settings["rejoin"] = not settings["rejoin"]
        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)

    async def stop_callback(self, interaction: discord.Interaction):
        global is_hopping
        is_hopping = False
        settings["force_lock"] = False
        settings["locked_channel_id"] = None
        settings["current_target"] = None
        for vc in bot.voice_clients:
            if vc.guild == interaction.guild:
                await vc.disconnect()
        self.update_buttons()
        await interaction.response.edit_message(embed=get_status_embed(interaction.guild), view=self)
        await interaction.followup.send("🛑 All loops, tracking, and locks terminated.", ephemeral=True)

def get_status_embed(guild):
    embed = discord.Embed(title="🎛️ Traffic System Control Panel", color=discord.Color.dark_red())
    
    current_target = "None (Idle)"
    if settings["current_target"]:
        user = guild.get_member(settings["current_target"])
        if user: current_target = f"🎯 {user.name}"

    if settings["force_lock"] and settings["locked_channel_id"]:
        ch = guild.get_channel(settings["locked_channel_id"])
        status_text = f"🔒 LOCKED IN: **{ch.name if ch else 'Unknown'}**"
    elif settings["traffic_active"]:
        status_text = "⚡ Running Traffic Tracking Mode (Active)" if is_hopping else "🟢 Auto-Join System: Active (Waiting for Targets)"
    else:
        status_text = "🔴 Auto-Join System: Deactivated (Sleeping)"

    embed.add_field(name="System Status", value=status_text, inline=False)
    embed.add_field(name="Current Target", value=current_target, inline=True)
    embed.add_field(name="Auto-Rejoin", value="✅ ON" if settings["rejoin"] else "❌ OFF", inline=True)
    embed.add_field(name="Speed Delay", value=f"{settings['delay']}s", inline=True)
    
    return embed

# --- Slash Commands ---

@bot.tree.command(name="panel", description="Send the interactive control panel.")
async def panel(interaction: discord.Interaction):
    user_id = interaction.user.id
    if user_id not in targets or not targets[user_id].get("is_admin", False):
        await interaction.response.send_message("❌ Unauthorized.", ephemeral=True)
        return
        
    view = ControlPanelView()
    await interaction.response.send_message(embed=get_status_embed(interaction.guild), view=view, ephemeral=True)

# Command-line alternative to toggle tracking without panel
@bot.tree.command(name="toggle_traffic", description="Enable or Disable the automated auto-join tracking system.")
async def toggle_traffic(interaction: discord.Interaction, active: bool):
    user_id = interaction.user.id
    if user_id not in targets or not targets[user_id].get("is_admin", False):
        await interaction.response.send_message("❌ Unauthorized.", ephemeral=True)
        return
    
    global is_hopping
    settings["traffic_active"] = active
    if not active:
        is_hopping = False
        settings["current_target"] = None
        for vc in bot.voice_clients:
            if vc.guild == interaction.guild:
                await vc.disconnect()
        await interaction.response.send_message("🔴 Auto-Join Tracking Turned OFF. Bot is now ignoring targets.", ephemeral=True)
    else:
        await interaction.response.send_message("🟢 Auto-Join Tracking Turned ON. Bot will now follow targets automatically.", ephemeral=True)

@bot.tree.command(name="set_speed", description="Change traffic loop intervals.")
async def set_speed(interaction: discord.Interaction, seconds: float):
    user_id = interaction.user.id
    if user_id not in targets or not targets[user_id].get("is_admin", False):
        await interaction.response.send_message("❌ Unauthorized.", ephemeral=True)
        return
    if seconds < 0.2:
        await interaction.response.send_message("⚠️ Too fast! Keep it >= 0.2s.", ephemeral=True)
        return
    settings["delay"] = seconds
    await interaction.response.send_message(f"⚡ Interval updated to **{seconds}s**", ephemeral=True)

bot.run(TOKEN)
