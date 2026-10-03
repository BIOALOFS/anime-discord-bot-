from logging.config import fileConfig
import asyncio, os
from alembic import context
from sqlalchemy.ext.asyncio import async_engine_from_config
from sqlalchemy import pool
from bot.database.models import Base

config = context.config
if config.config_file_name: fileConfig(config.config_file_name)
config.set_main_option("sqlalchemy.url", os.getenv("DATABASE_URL", config.get_main_option("sqlalchemy.url")))
target_metadata = Base.metadata

def offline():
    context.configure(url=config.get_main_option("sqlalchemy.url"), target_metadata=target_metadata, literal_binds=True, compare_type=True)
    with context.begin_transaction(): context.run_migrations()

async def online_async():
    engine = async_engine_from_config(config.get_section(config.config_ini_section), prefix="sqlalchemy.", poolclass=pool.NullPool)
    async with engine.connect() as connection:
        def run(conn):
            context.configure(connection=conn, target_metadata=target_metadata, compare_type=True)
            with context.begin_transaction(): context.run_migrations()
        await connection.run_sync(run)
    await engine.dispose()

if context.is_offline_mode(): offline()
else: asyncio.run(online_async())
