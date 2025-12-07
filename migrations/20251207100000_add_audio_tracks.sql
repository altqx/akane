-- Create audio_tracks table for multi-audio support
CREATE TABLE IF NOT EXISTS audio_tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    video_id TEXT NOT NULL,
    track_index INTEGER NOT NULL,
    language TEXT,
    title TEXT,
    codec TEXT NOT NULL,
    channels INTEGER,
    sample_rate INTEGER,
    bit_rate INTEGER,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (video_id) REFERENCES videos(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_audio_tracks_video_id ON audio_tracks(video_id);
