-- Keep the target as written alongside where it resolved to, so re-resolution
-- never narrows a stem into the path it happened to match first.
CREATE TABLE links_new (
  src_note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  target       TEXT NOT NULL,          -- link target exactly as written in the note
  dst_path     TEXT NOT NULL,          -- resolved relative path, or the target if unresolved
  resolved     INTEGER NOT NULL,       -- 0 or 1
  PRIMARY KEY (src_note_id, target)
);

INSERT OR IGNORE INTO links_new (src_note_id, target, dst_path, resolved)
  SELECT src_note_id, dst_path, dst_path, resolved FROM links;

DROP TABLE links;
ALTER TABLE links_new RENAME TO links;

CREATE INDEX idx_links_dst ON links(dst_path);
