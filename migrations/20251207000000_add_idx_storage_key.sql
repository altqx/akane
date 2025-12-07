-- Add idx_storage_key for VobSub subtitle .idx files
ALTER TABLE subtitles ADD COLUMN idx_storage_key TEXT;
