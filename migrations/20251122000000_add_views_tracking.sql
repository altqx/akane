-- Add view_count to videos table
ALTER TABLE videos ADD COLUMN view_count INTEGER NOT NULL DEFAULT 0;

-- Create views table for historical tracking
CREATE TABLE views (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    video_id TEXT NOT NULL,
    ip_address TEXT,
    user_agent TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(video_id) REFERENCES videos(id) ON DELETE CASCADE
);

-- Create index for faster queries
CREATE INDEX idx_views_video_id ON views(video_id);
CREATE INDEX idx_views_created_at ON views(created_at);
