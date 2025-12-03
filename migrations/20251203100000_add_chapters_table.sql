-- Chapters table for storing video chapter metadata
CREATE TABLE IF NOT EXISTS chapters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    video_id TEXT NOT NULL,
    chapter_index INTEGER NOT NULL,
    start_time REAL NOT NULL,  -- Start time in seconds (float for precision)
    end_time REAL NOT NULL,    -- End time in seconds (float for precision)
    title TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(video_id) REFERENCES videos(id) ON DELETE CASCADE,
    UNIQUE(video_id, chapter_index)
);

-- Create index for faster lookups
CREATE INDEX IF NOT EXISTS idx_chapters_video_id ON chapters(video_id);
