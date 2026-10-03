"""Initial complete schema."""
from alembic import op
from bot.database.models import Base
revision = "0001"
down_revision = None
branch_labels = None
depends_on = None
def upgrade():
    bind = op.get_bind()
    later = {"server_configurations", "allowed_channels", "server_audit_logs"}
    Base.metadata.create_all(bind=bind, tables=[table for table in Base.metadata.sorted_tables if table.name not in later])
def downgrade():
    bind = op.get_bind()
    later = {"server_configurations", "allowed_channels", "server_audit_logs"}
    Base.metadata.drop_all(bind=bind, tables=[table for table in reversed(Base.metadata.sorted_tables) if table.name not in later])
