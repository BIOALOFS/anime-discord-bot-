-- The day (UTC, YYYY-MM-DD) birthdays were last posted, so a restart never posts them twice.
ALTER TABLE feed_state ADD COLUMN last_birthday_day TEXT NOT NULL DEFAULT '';
