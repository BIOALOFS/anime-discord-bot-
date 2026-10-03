"""Add persistent server channel configuration and audit trail."""
import sqlalchemy as sa
from alembic import op

revision = "0002"
down_revision = "0001"
branch_labels = None
depends_on = None

def upgrade():
    op.create_table("server_configurations",
        sa.Column("guild_id",sa.BigInteger(),primary_key=True,autoincrement=False),
        sa.Column("setup_complete",sa.Boolean(),nullable=False),
        sa.Column("all_channels",sa.Boolean(),nullable=False),
        sa.Column("changed_by",sa.BigInteger()),
        sa.Column("changed_at",sa.DateTime(timezone=True),nullable=False))
    op.create_index("ix_server_configurations_setup_complete","server_configurations",["setup_complete"])
    op.create_table("allowed_channels",
        sa.Column("guild_id",sa.BigInteger(),sa.ForeignKey("server_configurations.guild_id",ondelete="CASCADE"),primary_key=True),
        sa.Column("channel_id",sa.BigInteger(),primary_key=True),
        sa.Column("added_by",sa.BigInteger(),nullable=False),
        sa.Column("added_at",sa.DateTime(timezone=True),nullable=False))
    op.create_table("server_audit_logs",
        sa.Column("id",sa.Integer(),primary_key=True),
        sa.Column("actor_id",sa.BigInteger(),nullable=False),
        sa.Column("guild_id",sa.BigInteger(),nullable=False),
        sa.Column("action",sa.String(100),nullable=False),
        sa.Column("previous",sa.JSON(),nullable=False),
        sa.Column("current",sa.JSON(),nullable=False),
        sa.Column("created_at",sa.DateTime(timezone=True),nullable=False))
    op.create_index("ix_server_audit_logs_actor_id","server_audit_logs",["actor_id"])
    op.create_index("ix_server_audit_logs_guild_id","server_audit_logs",["guild_id"])
    op.create_index("ix_server_audit_logs_created_at","server_audit_logs",["created_at"])

def downgrade():
    op.drop_table("server_audit_logs")
    op.drop_table("allowed_channels")
    op.drop_table("server_configurations")
