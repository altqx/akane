-- Create videos table
CREATE TABLE IF NOT EXISTS videos (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    available_resolutions TEXT NOT NULL,
    duration INTEGER NOT NULL,
    thumbnail_key TEXT NOT NULL,
    entrypoint TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Create index for querying by creation date
CREATE INDEX idx_videos_created_at ON videos(created_at);
