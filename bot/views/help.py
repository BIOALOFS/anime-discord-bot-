import discord
class HelpView(discord.ui.View):
    def __init__(self,user_id,pages,labels):
        super().__init__(timeout=180);self.user_id=user_id;self.pages=pages;self.index=0;self.message=None
        previous=discord.ui.Button(label=labels[0],style=discord.ButtonStyle.secondary);previous.callback=self.previous;self.add_item(previous)
        nxt=discord.ui.Button(label=labels[1],style=discord.ButtonStyle.secondary);nxt.callback=self.next;self.add_item(nxt)
    def embed(self):
        title,body=self.pages[self.index];embed=discord.Embed(title=title,description=body);embed.set_footer(text=f"{self.index+1} / {len(self.pages)}");return embed
    async def interaction_check(self,i):
        if i.user.id!=self.user_id:await i.response.send_message("Not your help session / ليست جلسة المساعدة الخاصة بك",ephemeral=True);return False
        allowed,message=await i.client.ordinary_access(i.guild,i.channel)
        if not allowed:await i.response.send_message(message,ephemeral=True);return False
        return True
    async def previous(self,i):self.index=(self.index-1)%len(self.pages);await i.response.edit_message(embed=self.embed(),view=self)
    async def next(self,i):self.index=(self.index+1)%len(self.pages);await i.response.edit_message(embed=self.embed(),view=self)
    async def on_timeout(self):
        for child in self.children:child.disabled=True
        if self.message:
            try:await self.message.edit(view=self)
            except discord.HTTPException:pass
