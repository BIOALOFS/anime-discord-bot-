"""Add approval metadata and catalog image selection without losing existing data."""
import sqlalchemy as sa
from alembic import op

revision="0003"
down_revision="0002"
branch_labels=None
depends_on=None

def upgrade():
    inspector=sa.inspect(op.get_bind());columns=lambda table:{x["name"] for x in inspector.get_columns(table)}
    user_columns=columns("users")
    if "avatar_review_status" not in user_columns or "banner_review_status" not in user_columns:
        with op.batch_alter_table("users") as batch:
            if "avatar_review_status" not in user_columns:batch.add_column(sa.Column("avatar_review_status",sa.Enum("pending","approved","rejected","hidden",name="reviewstatus"),nullable=False,server_default="pending"))
            if "banner_review_status" not in user_columns:batch.add_column(sa.Column("banner_review_status",sa.Enum("pending","approved","rejected","hidden",name="reviewstatus"),nullable=False,server_default="pending"))
    entry_columns=columns("collection_entries")
    if "selected_catalog_image_id" not in entry_columns:
        with op.batch_alter_table("collection_entries") as batch:
            batch.add_column(sa.Column("selected_catalog_image_id",sa.Integer(),nullable=True));batch.create_foreign_key("fk_entry_selected_catalog_image","catalog_images",["selected_catalog_image_id"],["id"])
    image_columns=columns("personal_images")
    if not {"review_status","reviewed_by","reviewed_at"}<=image_columns:
        with op.batch_alter_table("personal_images") as batch:
            if "review_status" not in image_columns:batch.add_column(sa.Column("review_status",sa.Enum("pending","approved","rejected","hidden",name="reviewstatus"),nullable=False,server_default="pending"));batch.create_index("ix_personal_images_review_status",["review_status"])
            if "reviewed_by" not in image_columns:batch.add_column(sa.Column("reviewed_by",sa.BigInteger(),nullable=True))
            if "reviewed_at" not in image_columns:batch.add_column(sa.Column("reviewed_at",sa.DateTime(timezone=True),nullable=True))

def downgrade():
    with op.batch_alter_table("personal_images") as batch:
        batch.drop_index("ix_personal_images_review_status");batch.drop_column("reviewed_at");batch.drop_column("reviewed_by");batch.drop_column("review_status")
    with op.batch_alter_table("collection_entries") as batch:
        batch.drop_constraint("fk_entry_selected_catalog_image",type_="foreignkey");batch.drop_column("selected_catalog_image_id")
    with op.batch_alter_table("users") as batch:
        batch.drop_column("banner_review_status");batch.drop_column("avatar_review_status")
