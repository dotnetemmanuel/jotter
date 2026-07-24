CREATE TABLE notes (
  id           INTEGER PRIMARY KEY,
  path         TEXT NOT NULL UNIQUE,
  title        TEXT NOT NULL,
  mtime        INTEGER NOT NULL,
  size         INTEGER NOT NULL,
  frontmatter  TEXT
);
CREATE TABLE tags (
  note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  tag      TEXT NOT NULL,
  PRIMARY KEY (note_id, tag)
);
CREATE TABLE links (
  src_note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  dst_path     TEXT NOT NULL,
  resolved     INTEGER NOT NULL,
  PRIMARY KEY (src_note_id, dst_path)
);
CREATE INDEX idx_links_dst ON links(dst_path);
CREATE INDEX idx_tags_tag ON tags(tag);
CREATE VIRTUAL TABLE notes_fts USING fts5(
  title, body,
  content='',
  contentless_delete=1,
  tokenize='unicode61 remove_diacritics 2'
);
