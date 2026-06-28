import discord
from discord import app_commands
from discord.ext import commands
import asyncio
import json
import os
from dotenv import load_dotenv

# تحميل المتغيرات من ملف .env
load_dotenv()
TOKEN = os.getenv("DISCORD_TOKEN")

# إعداد الـ Intents
intents = discord.Intents.default()
intents.voice_states = True
intents.members = True
intents.guilds = True

bot = commands.Bot(command_prefix="!", intents=intents)

# اسم ملف قاعدة البيانات المؤقتة
DATA_FILE = "targets.json"

# دالة لقراءة الأهداف من ملف JSON
def load_targets():
    if os.path.exists(DATA_FILE):
        try:
            with open(DATA_FILE, "r") as f:
                # تحويل المفاتيح إلى int (لأن JSON يخزنها كنصوص)
                data = json.load(f)
                return {int(k): v for k, v in data.items()}
        except Exception:
            return {}
    return {}

# دالة لحفظ الأهداف في ملف JSON
def save_targets():
    with open(DATA_FILE, "w") as f:
        json.dump(targets, f, indent=4)

# تحميل الأهداف عند بدء تشغيل الكود
targets = load_targets()

# إعدادات البوت الافتراضية
settings = {
    "rejoin": True,
    "delay": 0.6,
    "current_target": None
}

is_hopping = False

@bot.event
async def on_ready():
    print(f"[{bot.user.name}] شغال ومستعد!")
    try:
        synced = await bot.tree.sync()
        print(f"تم مزامنة {len(synced)} أمر بنجاح.")
    except Exception as e:
        print(f"فشل المزامنة: {e}")

async def start_traffic(member: discord.Member, voice_channel: discord.VoiceChannel):
    global is_hopping
    if is_hopping:
        return
    
    is_hopping = True
    settings["current_target"] = member.id
    print(f"بدء التافيك على: {member.name}")

    while is_hopping and settings["current_target"] == member.id:
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

@bot.event
async def on_voice_state_update(member, before, after):
    global is_hopping
    
    # نظام الـ Rejoin عند طرد البوت
    if member.id == bot.user.id:
        if before.channel is not None and after.channel is None and settings["rejoin"]:
            current_target_id = settings["current_target"]
            if current_target_id and before.channel:
                target_member = before.channel.guild.get_member(current_target_id)
                if target_member and target_member.voice and target_member.voice.channel == before.channel:
                    is_hopping = False 
                    asyncio.create_task(start_traffic(target_member, before.channel))
        return

    # إذا دخل شخص مستهدف
    if member.id in targets and after.channel is not None:
        member_priority = targets[member.id]
        current_target_id = settings["current_target"]

        if current_target_id is None:
            asyncio.create_task(start_traffic(member, after.channel))
            
        # نظام الأولويات
        elif current_target_id != member.id:
            current_priority = targets.get(current_target_id, 99)
            if member_priority < current_priority:
                is_hopping = False 
                for vc in bot.voice_clients:
                    if vc.guild == member.guild:
                        await vc.disconnect()
                await asyncio.sleep(0.5)
                asyncio.create_task(start_traffic(member, after.channel))

    # إذا خرج المستهدف الحالي
    if member.id == settings["current_target"] and after.channel is None:
        is_hopping = False

# --- الأوامر المائلة (Slash Commands) ---

@bot.tree.command(name="traffic_add", description="إضافة شخص لقائمة التافيك وتحديد أولويته")
@app_commands.choices(priority=[
    app_commands.Choice(name="أولوية قصوى (1)", value=1),
    app_commands.Choice(name="أولوية متوسطة (2)", value=2),
    app_commands.Choice(name="أولوية عادية (3)", value=3)
])
async def traffic_add(interaction: discord.Interaction, user: discord.Member, priority: int):
    if user.bot:
        await interaction.response.send_message("❌ لا يمكنك استهداف البوتات!", ephemeral=True)
        return
        
    targets[user.id] = priority
    save_targets() # حفظ التعديل في ملف JSON
    await interaction.response.send_message(f"✅ تم إضافة **{user.name}** بأولوية ({priority}) وتم الحفظ في الملف.", ephemeral=True)

@bot.tree.command(name="traffic_remove", description="إزالة شخص من قائمة التافيك")
async def traffic_remove(interaction: discord.Interaction, user: discord.Member):
    if user.id in targets:
        del targets[user.id]
        save_targets() # تحديث ملف JSON
        if settings["current_target"] == user.id:
            global is_hopping
            is_hopping = False
        await interaction.response.send_message(f"❌ تم إزالة **{user.name}** من القائمة.", ephemeral=True)
    else:
        await interaction.response.send_message("ℹ️ الشخص ليس في القائمة.", ephemeral=True)

@bot.tree.command(name="traffic_toggle_rejoin", description="تفعيل أو إلغاء نظام الـ Rejoin التلقائي")
@app_commands.choices(status=[
    app_commands.Choice(name="تفعيل Rejoin", value=1),
    app_commands.Choice(name="تعطيل Rejoin", value=0)
])
async def toggle_rejoin(interaction: discord.Interaction, status: int):
    settings["rejoin"] = bool(status)
    status_text = "مفعّل ✅" if settings["rejoin"] else "معطّل ❌"
    await interaction.response.send_message(f"⚙️ نظام الـ Rejoin الآن: **{status_text}**", ephemeral=True)

@bot.tree.command(name="traffic_speed", description="تعديل سرعة التافيك")
async def traffic_speed(interaction: discord.Interaction, seconds: float):
    if seconds < 0.2:
        await interaction.response.send_message("⚠️ أقل حد مسموح للسرعة هو 0.2 ثانية لحماية البوت.", ephemeral=True)
        return
    settings["delay"] = seconds
    await interaction.response.send_message(f"⚡ تم تعديل السرعة إلى: **{seconds} ثانية**.", ephemeral=True)

@bot.tree.command(name="traffic_status", description="عرض حالة البوت الحالية والأهداف")
async def traffic_status(interaction: discord.Interaction):
    embed = discord.Embed(title="📊 حالة بوت التافيك والأولويات", color=discord.Color.purple())
    
    current_target_name = "لا يوجد"
    if settings["current_target"]:
        target_user = interaction.guild.get_member(settings["current_target"])
        if target_user: current_target_name = target_user.name
            
    embed.add_field(name="🎯 الهدف الحالي:", value=current_target_name, inline=False)
    embed.add_field(name="🔄 نظام Rejoin:", value="شغال ✅" if settings["rejoin"] else "مغلق ❌", inline=True)
    embed.add_field(name="⚡ السرعة:", value=f"{settings['delay']} ثانية", inline=True)
    
    list_text = ""
    for user_id, priority in targets.items():
        user = interaction.guild.get_member(user_id)
        if user: list_text += f"• {user.name} -> أولوية: {priority}\n"
            
    embed.add_field(name="📋 الأهداف المحفوظة في JSON:", value=list_text if list_text else "القائمة فارغة", inline=False)
    await interaction.response.send_message(embed=embed, ephemeral=True)

@bot.tree.command(name="traffic_off", description="إيقاف التافيك فوراً")
async def traffic_off(interaction: discord.Interaction):
    global is_hopping
    is_hopping = False
    settings["current_target"] = None
    for vc in bot.voice_clients:
        if vc.guild == interaction.guild:
            await vc.disconnect()
    await interaction.response.send_message(f"🛑 تم إيقاف التافيك وفصل البوت.", ephemeral=True)

bot.run(TOKEN)
