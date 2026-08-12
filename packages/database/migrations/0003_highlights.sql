-- Your reading, kept — reshaped around what a device actually stores.
--
-- The Phase 0 schema already had a `highlight` table. It was designed for a
-- desktop pipeline that no longer exists, and hardware contradicted three of
-- its assumptions:
--
--   * `page_number INTEGER NOT NULL` — a page number can be genuinely unknown,
--     when a document's `.content` uses a layout we have not verified. The
--     extractor returns the text with no page rather than a guessed one, and a
--     NOT NULL column has nowhere to put that.
--   * `document_id REFERENCES document(id)` — highlights are read straight off
--     the device and keyed by *its* uuid. Requiring a Marginalia document row
--     first would mean inventing one for every book someone has ever opened.
--   * `color TEXT` — the device writes an integer, and often writes nothing at
--     all. 10 of 26 highlighted documents on the reference device had no colour
--     field whatsoever.
--
-- Nothing ever wrote to the old table: no code referenced it, on any path. So
-- this replaces it rather than migrating rows, and says so plainly instead of
-- carrying a copy step that would silently do nothing.
--
-- `side_note` and `sticky_note` go with it. Both belonged to the overlay and
-- sticky-note features removed from the roadmap on 2026-08-13, for the same
-- reason as everything else in that group: they needed a screen this project
-- has decided never to draw on.

DROP VIEW IF EXISTS annotation;
DROP TABLE IF EXISTS sticky_note;
DROP TABLE IF EXISTS side_note;
DROP TABLE IF EXISTS highlight;

CREATE TABLE highlight (
  -- Derived from the highlight's own content and position, not generated.
  --
  -- WHY: extraction runs repeatedly over the same files. A random id would
  -- insert 2,624 duplicate rows on the second run. A deterministic one makes
  -- re-extraction idempotent, which is also what lets first_seen_at mean
  -- something — it is the first run that saw *this* passage, not the first run
  -- that happened to write a row.
  id TEXT PRIMARY KEY,

  -- The reMarkable's own uuid. Deliberately not a foreign key: the device owns
  -- these and may delete one at any time, and losing someone's notes because
  -- the device forgot the document would be exactly backwards.
  document_uuid TEXT NOT NULL,

  -- The document's name when last seen. Denormalised on purpose: a renamed
  -- document should not orphan the quotation, and the old name is often how
  -- someone remembers it.
  document_name TEXT NOT NULL,
  file_type TEXT NOT NULL,

  page_id TEXT NOT NULL,
  -- NULL when the .content layout could not be read. Never a guess.
  page_number INTEGER,

  start_offset INTEGER NOT NULL,
  length INTEGER NOT NULL,
  text TEXT NOT NULL,

  -- NULL means the device recorded no colour, which older highlights do not.
  -- Distinct from "colour zero", which is why it is nullable rather than 0.
  color INTEGER,

  -- Which version of the extractor produced this row. A format correction
  -- bumps it, and rows behind the current version can be re-extracted rather
  -- than silently disagreeing with newer ones.
  extraction_version INTEGER NOT NULL,

  first_seen_at TEXT NOT NULL,
  last_seen_at TEXT NOT NULL,

  -- Set when a run no longer finds it on the device. The row is kept: a
  -- highlight deleted on the device is still something someone read, and
  -- destroying that record on a device's say-so is not this table's job.
  -- Nothing deletes from this table.
  gone_from_device_at TEXT
);

CREATE INDEX idx_highlight_document ON highlight (document_uuid, page_number);
CREATE INDEX idx_highlight_first_seen ON highlight (first_seen_at);
CREATE INDEX idx_highlight_extraction_version ON highlight (extraction_version);

-- One row per extraction, so "what is new" has something to be new since.
CREATE TABLE extraction_run (
  id TEXT PRIMARY KEY,
  ran_at TEXT NOT NULL,
  extraction_version INTEGER NOT NULL,

  documents_seen INTEGER NOT NULL,
  highlights_seen INTEGER NOT NULL,
  highlights_new INTEGER NOT NULL,

  -- Documents whose files could not be read. Recorded rather than dropped: a
  -- run that quietly skipped ten documents must not look like a clean one.
  documents_unreadable INTEGER NOT NULL
);

CREATE INDEX idx_extraction_run_ran_at ON extraction_run (ran_at);

-- The `annotation` view survives with one member. It is kept rather than
-- dropped because it is the seam a future kind of annotation — handwritten
-- margin strokes — arrives through, and a one-member union is a cheap way to
-- keep the callers written against the seam rather than the table.
CREATE VIEW annotation AS
  SELECT id, 'HIGHLIGHT' AS kind, document_uuid, document_name, page_number,
         text, first_seen_at, last_seen_at
  FROM highlight;
