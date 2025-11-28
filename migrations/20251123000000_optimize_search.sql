-- Create FTS table for videos
CREATE VIRTUAL TABLE IF NOT EXISTS videos_fts USING fts5(id, name, tags);

-- Populate FTS table with existing data
INSERT INTO videos_fts(id, name, tags) SELECT id, name, tags FROM videos;

-- Triggers to keep FTS table in sync
CREATE TRIGGER videos_ai AFTER INSERT ON videos BEGIN
  INSERT INTO videos_fts(id, name, tags) VALUES (new.id, new.name, new.tags);
END;

CREATE TRIGGER videos_ad AFTER DELETE ON videos BEGIN
  DELETE FROM videos_fts WHERE id = old.id;
END;

CREATE TRIGGER videos_au AFTER UPDATE ON videos BEGIN
  UPDATE videos_fts SET name = new.name, tags = new.tags WHERE id = new.id;
END;
