-- Marginalia initial schema.
-- See docs/architecture/SQLITE_SCHEMA.md for the annotated version.

CREATE TABLE zotero_item (
  id TEXT PRIMARY KEY,
  zotero_key TEXT NOT NULL UNIQUE,
  zotero_version INTEGER NOT NULL,
  library_id TEXT NOT NULL,
  item_type TEXT NOT NULL,
  title TEXT,
  creators TEXT,
  publication TEXT,
  year INTEGER,
  doi TEXT,
  isbn TEXT,
  url TEXT,
  abstract TEXT,
  date_added TEXT,
  date_modified TEXT,
  raw TEXT NOT NULL,
  deleted_remote INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_zitem_version ON zotero_item(zotero_version);
CREATE INDEX idx_zitem_year ON zotero_item(year);

CREATE TABLE zotero_attachment (
  id TEXT PRIMARY KEY,
  zotero_item_id TEXT NOT NULL REFERENCES zotero_item(id) ON DELETE CASCADE,
  zotero_key TEXT NOT NULL UNIQUE,
  link_mode TEXT NOT NULL,
  content_type TEXT,
  filename TEXT,
  local_path TEXT,
  file_size_bytes INTEGER,
  availability TEXT NOT NULL
    CHECK (availability IN ('UNKNOWN','NOT_PRESENT','AVAILABLE_LOCAL','UNREADABLE')),
  checksum_sha256 TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE zotero_collection (
  id TEXT PRIMARY KEY,
  zotero_key TEXT NOT NULL UNIQUE,
  zotero_version INTEGER NOT NULL,
  name TEXT NOT NULL,
  parent_collection_id TEXT REFERENCES zotero_collection(id),
  library_id TEXT NOT NULL
);

CREATE TABLE zotero_item_collection (
  zotero_item_id TEXT NOT NULL REFERENCES zotero_item(id) ON DELETE CASCADE,
  zotero_collection_id TEXT NOT NULL REFERENCES zotero_collection(id) ON DELETE CASCADE,
  PRIMARY KEY (zotero_item_id, zotero_collection_id)
);

CREATE TABLE document (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('ZOTERO','LOCAL_FILE','DEVICE_ONLY')),
  page_count INTEGER,
  state TEXT NOT NULL CHECK (state IN (
    'METADATA_ONLY','ATTACHMENT_AVAILABLE','TRANSFER_PENDING','ON_REMARKABLE',
    'ANNOTATED','CHANGES_PENDING','SYNCED','CONFLICT','TRANSFER_FAILED',
    'REMOVED_FROM_DEVICE')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE device (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  serial_hash TEXT UNIQUE,
  display_name TEXT NOT NULL,
  firmware_version TEXT,
  firmware_known INTEGER NOT NULL DEFAULT 0,
  connection TEXT NOT NULL DEFAULT 'UNKNOWN',
  last_seen_at TEXT,
  storage_total_bytes INTEGER,
  storage_free_bytes INTEGER,
  -- Safe Mode is ON for every device, always, until a human turns it off.
  safe_mode INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE document_mapping (
  id TEXT PRIMARY KEY,
  zotero_item_key TEXT,
  zotero_attachment_key TEXT,
  local_document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  remarkable_document_id TEXT,
  original_filename TEXT NOT NULL,
  original_checksum TEXT NOT NULL,
  working_checksum TEXT,
  device_checksum TEXT,
  device_state TEXT NOT NULL,
  transferred_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_synced_at TEXT
);
CREATE UNIQUE INDEX idx_mapping_rm_uuid
  ON document_mapping(remarkable_document_id) WHERE remarkable_document_id IS NOT NULL;
CREATE UNIQUE INDEX idx_mapping_attachment
  ON document_mapping(zotero_attachment_key) WHERE zotero_attachment_key IS NOT NULL;

-- INV-3: the checksum of a user's original file is written once and never
-- changes. If the source file really did change, that is a new mapping
-- generation and a conflict for the user to resolve -- not a silent overwrite.
CREATE TRIGGER trg_original_checksum_immutable
BEFORE UPDATE OF original_checksum ON document_mapping
WHEN OLD.original_checksum IS NOT NULL AND NEW.original_checksum <> OLD.original_checksum
BEGIN
  SELECT RAISE(ABORT, 'original_checksum is immutable');
END;

CREATE TABLE reading_state (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  device_id TEXT REFERENCES device(id),
  current_page INTEGER,
  total_pages INTEGER,
  progress_percent REAL,
  last_opened_at TEXT,
  last_annotation_at TEXT,
  status TEXT NOT NULL CHECK (status IN ('UNREAD','READING','COMPLETED','ARCHIVED'))
);

CREATE TABLE highlight (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  page_number INTEGER NOT NULL,
  bounding_boxes TEXT NOT NULL,
  selected_text TEXT,
  context_before TEXT,
  context_after TEXT,
  color TEXT,
  type TEXT NOT NULL DEFAULT 'PLAIN'
    CHECK (type IN ('PLAIN','IMPORTANT','CITATION','QUESTION','IDEA','REFERENCE')),
  source TEXT NOT NULL CHECK (source IN ('REMARKABLE','DESKTOP','ZOTERO','IMPORTED')),
  source_ref TEXT,
  extraction_version INTEGER NOT NULL,
  confidence REAL,
  zotero_annotation_key TEXT,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL
);
CREATE INDEX idx_highlight_doc_page ON highlight(document_id, page_number);

CREATE TABLE side_note (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  page_number INTEGER NOT NULL,
  anchor_x REAL,
  anchor_y REAL,
  highlight_id TEXT REFERENCES highlight(id) ON DELETE SET NULL,
  content TEXT NOT NULL,
  content_type TEXT NOT NULL DEFAULT 'MARKDOWN' CHECK (content_type IN ('PLAIN','MARKDOWN')),
  source TEXT NOT NULL,
  source_ref TEXT,
  extraction_version INTEGER NOT NULL,
  zotero_note_key TEXT,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL
);

CREATE TABLE sticky_note (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES document(id) ON DELETE CASCADE,
  page_number INTEGER NOT NULL,
  x REAL NOT NULL,
  y REAL NOT NULL,
  anchor_width REAL,
  anchor_height REAL,
  content TEXT NOT NULL,
  source TEXT NOT NULL,
  source_ref TEXT,
  extraction_version INTEGER NOT NULL,
  zotero_annotation_key TEXT,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL
);

CREATE VIEW annotation AS
  SELECT id, 'HIGHLIGHT' AS kind, document_id, page_number, selected_text AS text,
         type, source, created_at, modified_at FROM highlight
  UNION ALL
  SELECT id, 'SIDE_NOTE', document_id, page_number, content,
         'PLAIN', source, created_at, modified_at FROM side_note
  UNION ALL
  SELECT id, 'STICKY_NOTE', document_id, page_number, content,
         'PLAIN', source, created_at, modified_at FROM sticky_note;

CREATE TABLE tag (
  id TEXT PRIMARY KEY,
  namespace TEXT NOT NULL CHECK (namespace IN ('ZOTERO','REMARKABLE','MARGINALIA')),
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL,
  UNIQUE (namespace, name)
);

CREATE TABLE tag_mapping (
  id TEXT PRIMARY KEY,
  zotero_tag TEXT NOT NULL,
  remarkable_tag TEXT NOT NULL,
  direction TEXT NOT NULL CHECK (direction IN ('ZOTERO_TO_RM','RM_TO_ZOTERO','BIDIRECTIONAL')),
  -- An unconfirmed mapping is a suggestion. It is never applied.
  confirmed_by_user INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (zotero_tag, remarkable_tag)
);

CREATE TABLE device_capability (
  id TEXT PRIMARY KEY,
  device_id TEXT NOT NULL REFERENCES device(id) ON DELETE CASCADE,
  capability TEXT NOT NULL,
  status TEXT NOT NULL
    CHECK (status IN ('SUPPORTED','READ_ONLY','EXPERIMENTAL','UNSUPPORTED','UNKNOWN')),
  source TEXT NOT NULL CHECK (source IN ('MATRIX','PROBED','USER_OVERRIDE')),
  tested_at TEXT,
  notes TEXT,
  UNIQUE (device_id, capability)
);

CREATE TABLE safety_snapshot (
  id TEXT PRIMARY KEY,
  device_id TEXT NOT NULL REFERENCES device(id),
  created_at TEXT NOT NULL,
  operation TEXT NOT NULL,
  affected_documents TEXT NOT NULL,
  checksums TEXT NOT NULL,
  storage_before TEXT,
  status TEXT NOT NULL
    CHECK (status IN ('PENDING','VERIFIED','FAILED','CONSUMED','RESTORED'))
);

CREATE TABLE sync_job (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN
    ('ZOTERO_METADATA','DEVICE_SCAN','ANNOTATION_INGEST','TRANSFER','REMOVAL',
     'ZOTERO_EXPORT','TAG_BRIDGE')),
  state TEXT NOT NULL,
  triggered_by TEXT NOT NULL CHECK (triggered_by IN ('USER','SCHEDULE','STARTUP')),
  started_at TEXT NOT NULL,
  finished_at TEXT,
  counters TEXT,
  error TEXT,
  -- INV-2, third independent guard: a schedule can never start a job that
  -- writes to the device or pushes data outward. The primary guards are the
  -- Rust type split and the state machine.
  CHECK (kind NOT IN ('TRANSFER','REMOVAL','ZOTERO_EXPORT') OR triggered_by = 'USER')
);

CREATE TABLE sync_operation (
  id TEXT PRIMARY KEY,
  sync_job_id TEXT NOT NULL REFERENCES sync_job(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  target_ref TEXT,
  state TEXT NOT NULL,
  detail TEXT,
  -- Makes a duplicate Send a no-op instead of a second copy on the device.
  idempotency_key TEXT NOT NULL UNIQUE,
  attempted_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE TABLE safety_log (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  level TEXT NOT NULL,
  event TEXT NOT NULL,
  outcome TEXT,
  reason TEXT,
  user_message TEXT,
  device_id TEXT,
  document_id TEXT,
  operation TEXT,
  detail TEXT
);
CREATE INDEX idx_safety_log_created ON safety_log(created_at);
