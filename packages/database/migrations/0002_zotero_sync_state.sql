-- Where each library's incremental sync got to.
--
-- Separate from zotero_collection and zotero_item on purpose: this is *our*
-- bookkeeping about a library, not mirrored Zotero data. Mixing the two would
-- mean a full re-mirror could not clear our cursor, or clearing our cursor
-- could disturb the mirror.
CREATE TABLE zotero_sync_state (
  -- "users/12345" or "groups/98765". Includes the kind, because the same
  -- numeric id can exist as both and their versions are unrelated.
  library_key TEXT PRIMARY KEY,

  -- Zotero's library version as of the last COMPLETED sync. 0 means never.
  --
  -- Written only when a sync reaches its final page. A partial sync leaves
  -- this alone, so the next run re-requests the same page rather than
  -- skipping data it never wrote.
  last_version INTEGER NOT NULL DEFAULT 0,

  last_synced_at TEXT,
  updated_at TEXT NOT NULL
);
