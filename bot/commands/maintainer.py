from __future__ import annotations
import logging
from discord.ext import commands
from sqlalchemy import delete, select, text
from sqlalchemy.orm import selectinload
from bot.database.models import AuditLog, CatalogImage, Character, CharacterWork, CollectionEntry, CustomSubmission, PersonalImage, ProviderReference, Report, ReviewStatus, User, Work
from bot.services.content import validate_safe_text
from bot.views.search import ReviewImportView

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
    @commands.hybrid_command(name="reviewsearch",description="Maintainer: search and import one provider candidate as pending")
    @maintainer()
    async def reviewsearch(self,ctx,*,query:str):
        if ctx.interaction:await ctx.defer(ephemeral=True)
        outcome=await self.bot.providers.search_characters(query)
        if not outcome.results:return await ctx.send("No provider candidates; "+"; ".join(outcome.warnings))
        view=ReviewImportView(ctx.author.id,outcome.results);view.message=await ctx.send("Select one candidate to import for review. Nothing is approved automatically.",view=view)
    @commands.hybrid_command(name="reviewqueue",description="Maintainer: list pending submissions and reports")
    @maintainer()
    async def reviewqueue(self,ctx):
        async with self.bot.db.session() as db:
            submissions=list((await db.scalars(select(CustomSubmission).where(CustomSubmission.status==ReviewStatus.pending).limit(20))).all()); reports=list((await db.scalars(select(Report).where(Report.status==ReviewStatus.pending).limit(20))).all())
            lines=[]
            for submission in submissions:
                images=list((await db.scalars(select(CatalogImage).where(CatalogImage.character_id==submission.character_id))).all())
                lines.append(f"submission #{submission.id} character={submission.character_id} images="+(", ".join(f"#{x.id} {x.url}" for x in images) or "—"))
            personal=list((await db.scalars(select(PersonalImage).where(PersonalImage.review_status==ReviewStatus.pending).limit(20))).all())
            appearances=list((await db.scalars(select(User).where(((User.avatar_url.is_not(None))&(User.avatar_review_status==ReviewStatus.pending))|((User.banner_url.is_not(None))&(User.banner_review_status==ReviewStatus.pending))).limit(20))).all())
        await ctx.send("Submissions:\n"+("\n".join(lines) or "—")+"\nPersonal images: "+(", ".join(f"#{x.id} {x.url}" for x in personal) or "—")+"\nAppearance users: "+(", ".join(str(x.discord_id) for x in appearances) or "—")+"\nReports: "+(", ".join(str(x.id) for x in reports) or "—"),allowed_mentions=__import__("discord").AllowedMentions.none())
    @commands.hybrid_command(name="review",description="Maintainer: approve/reject/hide a catalog character")
    @maintainer()
    async def review(self,ctx,character_id:str,status:str,*,note:str=""):
        if status not in {"approved","rejected","hidden"}: raise ValueError("status must be approved, rejected, or hidden")
        async with self.bot.db.session() as db:
            character=await db.get(Character,character_id)
            if not character: raise ValueError("character not found")
            submission=(await db.scalars(select(CustomSubmission).where(CustomSubmission.character_id==character_id))).first()
            if status=="approved" and character.is_custom and submission and not submission.submit_shared:
                raise ValueError("private custom entries cannot be approved into the shared catalog; owner must submit it")
            character.review_status=ReviewStatus(status)
            if submission: submission.status=ReviewStatus(status);submission.reviewer_id=ctx.author.id;submission.review_note=note[:2000]
            await self.audit(db,ctx,"review",character_id,{"status":status})
        await ctx.send("Review saved")
    @commands.hybrid_command(name="mergecharacters",description="Maintainer: merge a duplicate into a target")
    @maintainer()
    async def mergecharacters(self,ctx,source_id:str,target_id:str):
        if source_id==target_id: raise ValueError("source and target differ")
        async with self.bot.db.session() as db:
            if db.bind.dialect.name=="sqlite" and not db.in_transaction():await db.execute(text("BEGIN IMMEDIATE"))
            source,target=await db.get(Character,source_id),await db.get(Character,target_id)
            if not source or not target: raise ValueError("character not found")
            owners=set((await db.scalars(select(CollectionEntry.user_id).where(CollectionEntry.character_id.in_([source_id,target_id])))).all())
            if db.bind.dialect.name=="postgresql":
                for owner in sorted(owners):await db.execute(text("SELECT pg_advisory_xact_lock(:owner)"),{"owner":owner & 0x7FFF_FFFF_FFFF_FFFF})
            target_entries={x.user_id:x for x in (await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.personal_images)).where(CollectionEntry.character_id==target_id))).all()}
            source_entries=list((await db.scalars(select(CollectionEntry).options(selectinload(CollectionEntry.personal_images)).where(CollectionEntry.character_id==source_id))).all());affected=set()
            for entry in source_entries:
                affected.add(entry.user_id)
                if entry.user_id in target_entries:
                    survivor=target_entries[entry.user_id]
                    survivor.nickname=survivor.nickname or entry.nickname;survivor.note=survivor.note or entry.note
                    for image in list(entry.personal_images):
                        entry.personal_images.remove(image);survivor.personal_images.append(image)
                        if entry.selected_personal_image_id==image.id and survivor.selected_personal_image_id is None:survivor.selected_personal_image_id=image.id
                    if survivor.selected_catalog_image_id is None:survivor.selected_catalog_image_id=entry.selected_catalog_image_id
                    entry.selected_personal_image_id=None;entry.selected_catalog_image_id=None
                    await db.flush();await db.delete(entry)
                else: entry.character_id=target_id
            await db.flush()
            # Normalize affected owners through temporary negative positions to satisfy both database engines.
            for owner in affected:
                entries=list((await db.scalars(select(CollectionEntry).where(CollectionEntry.user_id==owner).order_by(CollectionEntry.position))).all())
                for index,entry in enumerate(entries,1): entry.position=-index
                await db.flush()
                for index,entry in enumerate(entries,1): entry.position=index
            target_images={x.url:x for x in (await db.scalars(select(CatalogImage).where(CatalogImage.character_id==target_id))).all()};target_urls=set(target_images)
            for image in (await db.scalars(select(CatalogImage).where(CatalogImage.character_id==source_id))).all():
                if image.url in target_urls:
                    selected=list((await db.scalars(select(CollectionEntry).where(CollectionEntry.selected_catalog_image_id==image.id))).all())
                    for entry in selected:entry.selected_catalog_image_id=target_images[image.url].id
                    await db.flush();await db.delete(image)
                else:image.character_id=target_id;target_urls.add(image.url);target_images[image.url]=image
            target_works=set((await db.scalars(select(CharacterWork.work_id).where(CharacterWork.character_id==target_id))).all())
            for link in (await db.scalars(select(CharacterWork).where(CharacterWork.character_id==source_id))).all():
                if link.work_id in target_works:await db.delete(link)
                else:link.character_id=target_id;target_works.add(link.work_id)
            target_refs={(x.provider,x.external_id) for x in (await db.scalars(select(ProviderReference).where(ProviderReference.character_id==target_id))).all()}
            for reference in (await db.scalars(select(ProviderReference).where(ProviderReference.character_id==source_id))).all():
                if (reference.provider,reference.external_id) in target_refs:await db.delete(reference)
                else:reference.character_id=target_id;target_refs.add((reference.provider,reference.external_id))
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

    @commands.hybrid_command(name="reviewwork",description="Maintainer: approve, reject, or hide a work")
    @maintainer()
    async def reviewwork(self,ctx,work_id:str,status:str):
        if status not in {"approved","rejected","hidden"}:raise ValueError("invalid status")
        async with self.bot.db.session() as db:
            work=await db.get(Work,work_id)
            if not work:raise ValueError("work not found")
            work.review_status=ReviewStatus(status);await self.audit(db,ctx,"review_work",work_id,{"status":status})
        await ctx.send("Work review saved")

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

    @commands.hybrid_command(name="reviewimage",description="Maintainer: approve or reject a catalog/personal image")
    @maintainer()
    async def reviewimage(self,ctx,kind:str,image_id:int,status:str):
        if kind not in {"catalog","personal"} or status not in {"approved","rejected","hidden"}:raise ValueError("kind catalog/personal; status approved/rejected/hidden")
        async with self.bot.db.session() as db:
            image=await db.get(CatalogImage if kind=="catalog" else PersonalImage,image_id)
            if not image:raise ValueError("image not found")
            image.review_status=ReviewStatus(status)
            if isinstance(image,PersonalImage):image.reviewed_by=ctx.author.id;image.reviewed_at=__import__("datetime").datetime.now(__import__("datetime").timezone.utc)
            await self.audit(db,ctx,"review_image",f"{kind}:{image_id}",{"status":status})
        await ctx.send("Image review saved")

    @commands.hybrid_command(name="reviewappearance",description="Maintainer: approve or reject a profile avatar/banner")
    @maintainer()
    async def reviewappearance(self,ctx,user_id:str,field:str,status:str):
        if field not in {"avatar","banner"} or status not in {"approved","rejected","hidden"}:raise ValueError("invalid field or status")
        async with self.bot.db.session() as db:
            user=await db.get(User,int(user_id))
            if not user:raise ValueError("user not found")
            setattr(user,f"{field}_review_status",ReviewStatus(status));await self.audit(db,ctx,"review_appearance",f"{user_id}:{field}",{"status":status})
        await ctx.send("Appearance review saved")

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
