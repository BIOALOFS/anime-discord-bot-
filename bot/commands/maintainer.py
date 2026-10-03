from __future__ import annotations
import logging
from discord.ext import commands
from sqlalchemy import delete, select
from sqlalchemy.orm import selectinload
from bot.database.models import AuditLog, CatalogImage, Character, CharacterWork, CollectionEntry, CustomSubmission, PersonalImage, Report, ReviewStatus, Work
from bot.services.content import validate_safe_text

log=logging.getLogger(__name__)

def maintainer():
    async def predicate(ctx):
        if not await ctx.bot.trust.is_global(ctx.author.id): raise commands.CheckFailure("global trusted users only")
        return True
    return commands.check(predicate)

class MaintainerCog(commands.Cog):
    def __init__(self,bot): self.bot=bot
    async def audit(self,db,ctx,action,target,details=None):
        db.add(AuditLog(maintainer_id=ctx.author.id,action=action,target=target,details=details or {})); log.warning("catalog action maintainer=%s action=%s target=%s",ctx.author.id,action,target)
    @commands.hybrid_command(name="reviewqueue",description="Maintainer: list pending submissions and reports")
    @maintainer()
    async def reviewqueue(self,ctx):
        async with self.bot.db.session() as db:
            submissions=list((await db.scalars(select(CustomSubmission).where(CustomSubmission.status==ReviewStatus.pending).limit(20))).all()); reports=list((await db.scalars(select(Report).where(Report.status==ReviewStatus.pending).limit(20))).all())
        await ctx.send("Submissions: "+(", ".join(str(x.id) for x in submissions) or "—")+"\nReports: "+(", ".join(str(x.id) for x in reports) or "—"))
    @commands.hybrid_command(name="review",description="Maintainer: approve/reject/hide a catalog character")
    @maintainer()
    async def review(self,ctx,character_id:str,status:str,*,note:str=""):
        if status not in {"approved","rejected","hidden"}: raise ValueError("status must be approved, rejected, or hidden")
        async with self.bot.db.session() as db:
            character=await db.get(Character,character_id)
            if not character: raise ValueError("character not found")
            character.review_status=ReviewStatus(status)
            submission=(await db.scalars(select(CustomSubmission).where(CustomSubmission.character_id==character_id))).first()
            if submission: submission.status=ReviewStatus(status);submission.reviewer_id=ctx.author.id;submission.review_note=note[:2000]
            await self.audit(db,ctx,"review",character_id,{"status":status})
        await ctx.send("Review saved")
    @commands.hybrid_command(name="mergecharacters",description="Maintainer: merge a duplicate into a target")
    @maintainer()
    async def mergecharacters(self,ctx,source_id:str,target_id:str):
        if source_id==target_id: raise ValueError("source and target differ")
        async with self.bot.db.session() as db:
            source,target=await db.get(Character,source_id),await db.get(Character,target_id)
            if not source or not target: raise ValueError("character not found")
            target_entries={x.user_id:x for x in (await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.personal_images)).where(CollectionEntry.character_id==target_id))).all()}
            source_entries=list((await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.personal_images)).where(CollectionEntry.character_id==source_id))).all());affected=set()
            for entry in source_entries:
                affected.add(entry.user_id)
                if entry.user_id in target_entries:
                    survivor=target_entries[entry.user_id]
                    survivor.nickname=survivor.nickname or entry.nickname;survivor.note=survivor.note or entry.note
                    for image in list(entry.personal_images):
                        entry.personal_images.remove(image);survivor.personal_images.append(image)
                    await db.flush();await db.delete(entry)
                else: entry.character_id=target_id
            await db.flush()
            # Normalize affected owners through temporary negative positions to satisfy both database engines.
            for owner in affected:
                entries=list((await db.scalars(select(CollectionEntry).where(CollectionEntry.user_id==owner).order_by(CollectionEntry.position))).all())
                for index,entry in enumerate(entries,1): entry.position=-index
                await db.flush()
                for index,entry in enumerate(entries,1): entry.position=index
            source.review_status=ReviewStatus.hidden
            await self.audit(db,ctx,"merge",source_id,{"target":target_id})
        await ctx.send("Merged; private metadata/images preserved and source hidden.")

    @commands.hybrid_command(name="editcatalog",description="Maintainer: correct catalog character metadata")
    @maintainer()
    async def editcatalog(self,ctx,character_id:str,field:str,*,value:str):
        fields={"name_original":300,"name_en":300,"name_ar":300,"category":50,"version":200,"description":2000}
        if field not in fields: raise ValueError("unsupported field")
        async with self.bot.db.session() as db:
            character=await db.get(Character,character_id)
            if not character: raise ValueError("character not found")
            previous=getattr(character,field);setattr(character,field,validate_safe_text(value,maximum=fields[field],field=field))
            character.review_status=ReviewStatus.pending
            await self.audit(db,ctx,"edit_character",character_id,{"field":field,"previous":previous})
        await ctx.send("Metadata updated and returned to pending review")

    @commands.hybrid_command(name="editwork",description="Maintainer: correct work metadata")
    @maintainer()
    async def editwork(self,ctx,work_id:str,field:str,*,value:str):
        fields={"title_original":300,"title_en":300,"title_ar":300,"category":50,"version":200}
        if field not in fields: raise ValueError("unsupported field")
        async with self.bot.db.session() as db:
            work=await db.get(Work,work_id)
            if not work: raise ValueError("work not found")
            previous=getattr(work,field);setattr(work,field,validate_safe_text(value,maximum=fields[field],field=field));work.review_status=ReviewStatus.pending
            await self.audit(db,ctx,"edit_work",work_id,{"field":field,"previous":previous})
        await ctx.send("Work updated and returned to pending review")

    @commands.hybrid_command(name="replacecatalogimage",description="Maintainer: replace a broken catalog image")
    @maintainer()
    async def replacecatalogimage(self,ctx,image_id:int,url:str):
        await self.bot.images.fetch(url)
        async with self.bot.db.session() as db:
            image=await db.get(CatalogImage,image_id)
            if not image: raise ValueError("catalog image not found")
            previous=image.url;image.url=url;image.review_status=ReviewStatus.pending
            await self.audit(db,ctx,"replace_catalog_image",str(image_id),{"previous":previous})
        await ctx.send("Image replaced and returned to pending review")

    @commands.hybrid_command(name="reports",description="Maintainer: list or resolve catalog reports")
    @maintainer()
    async def reports(self,ctx,report_id:int|None=None,status:str|None=None):
        async with self.bot.db.session() as db:
            if report_id is None:
                rows=list((await db.scalars(select(Report).where(Report.status==ReviewStatus.pending).limit(20))).all())
                return await ctx.send("\n".join(f"#{x.id} {x.reason} character={x.character_id}" for x in rows) or "No pending reports")
            if status not in {"approved","rejected"}: raise ValueError("status must be approved or rejected")
            report=await db.get(Report,report_id)
            if not report: raise ValueError("report not found")
            report.status=ReviewStatus(status);await self.audit(db,ctx,"resolve_report",str(report_id),{"status":status})
        await ctx.send("Report resolved")
    @commands.hybrid_command(name="providerstatus",description="Maintainer: inspect configured provider capabilities")
    @maintainer()
    async def providerstatus(self,ctx):
        await ctx.send("\n".join(f"{p.name}: {', '.join(sorted(x.value for x in p.capabilities)) or 'none'}" for p in self.bot.providers.providers))

async def setup(bot): await bot.add_cog(MaintainerCog(bot))
