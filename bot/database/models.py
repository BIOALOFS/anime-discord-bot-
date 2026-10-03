from __future__ import annotations

import enum
import uuid
from datetime import datetime, timezone
from typing import Any
from sqlalchemy import BigInteger, Boolean, DateTime, Enum, ForeignKey, Index, Integer, JSON, String, Text, UniqueConstraint
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship


def now() -> datetime:
    return datetime.now(timezone.utc)


class Base(DeclarativeBase):
    pass


class ReviewStatus(str, enum.Enum):
    pending = "pending"
    approved = "approved"
    rejected = "rejected"
    hidden = "hidden"


class User(Base):
    __tablename__ = "users"
    discord_id: Mapped[int] = mapped_column(BigInteger, primary_key=True, autoincrement=False)
    language: Mapped[str] = mapped_column(String(2), default="ar")
    avatar_url: Mapped[str | None] = mapped_column(Text)
    avatar_review_status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending)
    banner_url: Mapped[str | None] = mapped_column(Text)
    banner_review_status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending)
    bio: Mapped[str | None] = mapped_column(String(500))
    title: Mapped[str | None] = mapped_column(String(100))
    color: Mapped[str] = mapped_column(String(7), default="#5865F2")
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)
    entries: Mapped[list[CollectionEntry]] = relationship(back_populates="user", cascade="all, delete-orphan")


class Franchise(Base):
    __tablename__ = "franchises"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=lambda: str(uuid.uuid4()))
    name: Mapped[str] = mapped_column(String(300), index=True)


class Work(Base):
    __tablename__ = "works"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=lambda: str(uuid.uuid4()))
    title_original: Mapped[str] = mapped_column(String(300), index=True)
    title_en: Mapped[str | None] = mapped_column(String(300), index=True)
    title_ar: Mapped[str | None] = mapped_column(String(300), index=True)
    category: Mapped[str] = mapped_column(String(50), index=True)
    version: Mapped[str | None] = mapped_column(String(200))
    franchise_id: Mapped[str | None] = mapped_column(ForeignKey("franchises.id"))
    review_status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending, index=True)


class Character(Base):
    __tablename__ = "characters"
    id: Mapped[str] = mapped_column(String(36), primary_key=True, default=lambda: str(uuid.uuid4()))
    name_original: Mapped[str] = mapped_column(String(300), index=True)
    name_en: Mapped[str | None] = mapped_column(String(300), index=True)
    name_ar: Mapped[str | None] = mapped_column(String(300), index=True)
    aliases: Mapped[list[str]] = mapped_column(JSON, default=list)
    category: Mapped[str] = mapped_column(String(50), index=True)
    version: Mapped[str | None] = mapped_column(String(200))
    description: Mapped[str | None] = mapped_column(Text)
    review_status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending, index=True)
    is_custom: Mapped[bool] = mapped_column(Boolean, default=False)
    owner_id: Mapped[int | None] = mapped_column(BigInteger, ForeignKey("users.discord_id"))
    works: Mapped[list[CharacterWork]] = relationship(back_populates="character", cascade="all, delete-orphan")
    images: Mapped[list[CatalogImage]] = relationship(back_populates="character", cascade="all, delete-orphan")
    references: Mapped[list[ProviderReference]] = relationship(back_populates="character", cascade="all, delete-orphan")


class CharacterWork(Base):
    __tablename__ = "character_works"
    character_id: Mapped[str] = mapped_column(ForeignKey("characters.id", ondelete="CASCADE"), primary_key=True)
    work_id: Mapped[str] = mapped_column(ForeignKey("works.id", ondelete="CASCADE"), primary_key=True)
    role: Mapped[str | None] = mapped_column(String(200))
    actor_name: Mapped[str | None] = mapped_column(String(300))
    character: Mapped[Character] = relationship(back_populates="works")
    work: Mapped[Work] = relationship()


class ProviderReference(Base):
    __tablename__ = "provider_references"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    character_id: Mapped[str | None] = mapped_column(ForeignKey("characters.id", ondelete="CASCADE"), index=True)
    work_id: Mapped[str | None] = mapped_column(ForeignKey("works.id", ondelete="CASCADE"), index=True)
    provider: Mapped[str] = mapped_column(String(30))
    external_id: Mapped[str] = mapped_column(String(200))
    source_url: Mapped[str | None] = mapped_column(Text)
    character: Mapped[Character | None] = relationship(back_populates="references")
    __table_args__ = (UniqueConstraint("provider", "external_id", "character_id", "work_id"),)


class CatalogImage(Base):
    __tablename__ = "catalog_images"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    character_id: Mapped[str] = mapped_column(ForeignKey("characters.id", ondelete="CASCADE"), index=True)
    url: Mapped[str] = mapped_column(Text)
    is_default: Mapped[bool] = mapped_column(Boolean, default=False)
    source_url: Mapped[str | None] = mapped_column(Text)
    license: Mapped[str | None] = mapped_column(String(200))
    review_status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending)
    character: Mapped[Character] = relationship(back_populates="images")


class CollectionEntry(Base):
    __tablename__ = "collection_entries"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    user_id: Mapped[int] = mapped_column(ForeignKey("users.discord_id", ondelete="CASCADE"), index=True)
    character_id: Mapped[str] = mapped_column(ForeignKey("characters.id"), index=True)
    position: Mapped[int] = mapped_column(Integer)
    nickname: Mapped[str | None] = mapped_column(String(100))
    note: Mapped[str | None] = mapped_column(String(1000))
    selected_personal_image_id: Mapped[int | None] = mapped_column(ForeignKey("personal_images.id", use_alter=True))
    selected_catalog_image_id: Mapped[int | None] = mapped_column(ForeignKey("catalog_images.id"))
    added_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)
    user: Mapped[User] = relationship(back_populates="entries")
    character: Mapped[Character] = relationship()
    personal_images: Mapped[list[PersonalImage]] = relationship(back_populates="entry", cascade="all, delete-orphan", foreign_keys="PersonalImage.entry_id")
    __table_args__ = (UniqueConstraint("user_id", "character_id"), UniqueConstraint("user_id", "position"), Index("ix_collection_page", "user_id", "position"))


class PersonalImage(Base):
    __tablename__ = "personal_images"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    entry_id: Mapped[int] = mapped_column(ForeignKey("collection_entries.id", ondelete="CASCADE"), index=True)
    url: Mapped[str] = mapped_column(Text)
    source: Mapped[str] = mapped_column(String(30), default="user")
    review_status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending, index=True)
    reviewed_by: Mapped[int | None] = mapped_column(BigInteger)
    reviewed_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True))
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)
    entry: Mapped[CollectionEntry] = relationship(back_populates="personal_images", foreign_keys=[entry_id])


class CustomSubmission(Base):
    __tablename__ = "custom_submissions"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    owner_id: Mapped[int] = mapped_column(ForeignKey("users.discord_id"), index=True)
    character_id: Mapped[str] = mapped_column(ForeignKey("characters.id"))
    submit_shared: Mapped[bool] = mapped_column(Boolean, default=False)
    status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending, index=True)
    reviewer_id: Mapped[int | None] = mapped_column(BigInteger)
    review_note: Mapped[str | None] = mapped_column(Text)


class Report(Base):
    __tablename__ = "reports"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    reporter_id: Mapped[int] = mapped_column(BigInteger, index=True)
    character_id: Mapped[str] = mapped_column(ForeignKey("characters.id"), index=True)
    reason: Mapped[str] = mapped_column(String(30))
    details: Mapped[str | None] = mapped_column(String(1000))
    status: Mapped[ReviewStatus] = mapped_column(Enum(ReviewStatus), default=ReviewStatus.pending, index=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)


class UndoRecord(Base):
    __tablename__ = "undo_records"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    user_id: Mapped[int] = mapped_column(BigInteger, index=True)
    payload: Mapped[dict[str, Any]] = mapped_column(JSON)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now, index=True)


class AuditLog(Base):
    __tablename__ = "audit_logs"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    maintainer_id: Mapped[int] = mapped_column(BigInteger, index=True)
    action: Mapped[str] = mapped_column(String(100))
    target: Mapped[str] = mapped_column(String(200))
    details: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)


class ServerConfiguration(Base):
    __tablename__ = "server_configurations"
    guild_id: Mapped[int] = mapped_column(BigInteger, primary_key=True, autoincrement=False)
    setup_complete: Mapped[bool] = mapped_column(Boolean, default=False, index=True)
    all_channels: Mapped[bool] = mapped_column(Boolean, default=False)
    changed_by: Mapped[int | None] = mapped_column(BigInteger)
    changed_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now, onupdate=now)
    channels: Mapped[list[AllowedChannel]] = relationship(back_populates="configuration", cascade="all, delete-orphan")


class AllowedChannel(Base):
    __tablename__ = "allowed_channels"
    guild_id: Mapped[int] = mapped_column(ForeignKey("server_configurations.guild_id", ondelete="CASCADE"), primary_key=True)
    channel_id: Mapped[int] = mapped_column(BigInteger, primary_key=True)
    added_by: Mapped[int] = mapped_column(BigInteger)
    added_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now)
    configuration: Mapped[ServerConfiguration] = relationship(back_populates="channels")


class ServerAuditLog(Base):
    __tablename__ = "server_audit_logs"
    id: Mapped[int] = mapped_column(Integer, primary_key=True)
    actor_id: Mapped[int] = mapped_column(BigInteger, index=True)
    guild_id: Mapped[int] = mapped_column(BigInteger, index=True)
    action: Mapped[str] = mapped_column(String(100))
    previous: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    current: Mapped[dict[str, Any]] = mapped_column(JSON, default=dict)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), default=now, index=True)
