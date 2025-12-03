-- Subtitles table for storing subtitle track metadata
CREATE TABLE IF NOT EXISTS subtitles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    video_id TEXT NOT NULL,
    track_index INTEGER NOT NULL,
    language TEXT,
    title TEXT,
    codec TEXT NOT NULL,  -- 'ass', 'srt', 'subrip', etc.
    storage_key TEXT NOT NULL,  -- R2 path like '{video_id}/subtitles/track_0.ass'
    is_default INTEGER DEFAULT 0,
    is_forced INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(video_id) REFERENCES videos(id) ON DELETE CASCADE,
    UNIQUE(video_id, track_index)
);

-- Attachments table for storing font files and other attachments from MKV
CREATE TABLE IF NOT EXISTS attachments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    video_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    mimetype TEXT NOT NULL,  -- 'font/ttf', 'font/otf', 'application/x-truetype-font', etc.
    storage_key TEXT NOT NULL,  -- R2 path like '{video_id}/fonts/font.ttf'
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(video_id) REFERENCES videos(id) ON DELETE CASCADE,
    UNIQUE(video_id, filename)
);

-- Create indexes for faster lookups
CREATE INDEX IF NOT EXISTS idx_subtitles_video_id ON subtitles(video_id);
CREATE INDEX IF NOT EXISTS idx_attachments_video_id ON attachments(video_id);
