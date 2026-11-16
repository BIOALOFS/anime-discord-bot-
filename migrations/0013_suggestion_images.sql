-- Cover pictures of both works, so a suggestion can show them first.
ALTER TABLE suggestions ADD COLUMN first_image TEXT;
ALTER TABLE suggestions ADD COLUMN second_image TEXT;
