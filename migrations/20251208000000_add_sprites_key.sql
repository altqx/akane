-- Add sprites_key column to store preview sprite sheets separately from thumbnails
ALTER TABLE videos ADD COLUMN sprites_key TEXT;

-- Backfill existing rows so sprites remain available after column addition
UPDATE videos SET sprites_key = thumbnail_key WHERE sprites_key IS NULL;
