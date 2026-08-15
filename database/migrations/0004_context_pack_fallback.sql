CREATE TABLE context_packs (
  context_pack_id TEXT PRIMARY KEY CHECK (length(context_pack_id) = 36),
  conversation_id TEXT NOT NULL REFERENCES conversations(conversation_id),
  pack_revision INTEGER NOT NULL CHECK (pack_revision = 1),
  possible_side_effects TEXT NOT NULL CHECK (
    possible_side_effects IN ('none', 'possible', 'unknown')
  ),
  policy_max_bytes INTEGER NOT NULL CHECK (policy_max_bytes BETWEEN 1024 AND 262144),
  policy_recent_item_limit INTEGER NOT NULL CHECK (policy_recent_item_limit BETWEEN 1 AND 256),
  manifest_json TEXT NOT NULL CHECK (
    length(CAST(manifest_json AS BLOB)) BETWEEN 2 AND 1048576
  ),
  manifest_sha256 TEXT NOT NULL CHECK (length(manifest_sha256) = 64),
  compiled_sha256 TEXT NOT NULL CHECK (length(compiled_sha256) = 64),
  byte_length INTEGER NOT NULL CHECK (byte_length BETWEEN 1 AND 262144),
  truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
  omitted_source_count INTEGER NOT NULL CHECK (omitted_source_count BETWEEN 0 AND 512),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE INDEX context_pack_by_conversation
  ON context_packs(conversation_id, created_at_micros, context_pack_id);
