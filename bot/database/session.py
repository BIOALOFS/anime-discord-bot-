from contextlib import asynccontextmanager
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine
from sqlalchemy import event


class Database:
    def __init__(self, url: str):
        self.engine = create_async_engine(url, pool_pre_ping=True)
        if url.startswith("sqlite"):
            @event.listens_for(self.engine.sync_engine, "connect")
            def _sqlite_pragmas(connection, _record):
                cursor=connection.cursor();cursor.execute("PRAGMA foreign_keys=ON");cursor.execute("PRAGMA busy_timeout=5000");cursor.close()
        self.sessions = async_sessionmaker(self.engine, expire_on_commit=False)

    @asynccontextmanager
    async def session(self):
        async with self.sessions() as session:
            try:
                yield session
                await session.commit()
            except Exception:
                await session.rollback()
                raise

    async def close(self) -> None:
        await self.engine.dispose()
