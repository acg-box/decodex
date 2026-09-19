CREATE TABLE process_generation_death_evidence_new (
  evidence_id TEXT PRIMARY KEY CHECK (length(evidence_id) = 36),
  generation_id TEXT NOT NULL UNIQUE REFERENCES process_generations(generation_id),
  kind TEXT NOT NULL CHECK (
    kind IN (
      'spawn_not_created',
      'owned_child_exit',
      'linux_pidfd_exit',
      'macos_kqueue_exit_and_group_quiescence',
      'macos_kernel_confirmed_gone',
      'exact_termination_exit',
      'prior_boot_ended'
    )
  ),
  observed_boot_id TEXT NOT NULL CHECK (
    length(CAST(observed_boot_id AS BLOB)) BETWEEN 1 AND 256
  ),
  bound_boot_id TEXT,
  process_id INTEGER CHECK (process_id > 0),
  process_start_id TEXT,
  process_group_id INTEGER CHECK (process_group_id > 0),
  session_id INTEGER CHECK (session_id > 0),
  witness_sha256 TEXT NOT NULL CHECK (length(witness_sha256) = 64),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  CHECK (
    (bound_boot_id IS NULL AND process_id IS NULL AND process_start_id IS NULL AND
      process_group_id IS NULL AND session_id IS NULL) OR
    (bound_boot_id IS NOT NULL AND process_id IS NOT NULL AND process_start_id IS NOT NULL AND
      process_group_id = process_id AND session_id = process_id)
  )
) STRICT;

INSERT INTO process_generation_death_evidence_new SELECT * FROM process_generation_death_evidence;
DROP TABLE process_generation_death_evidence;
ALTER TABLE process_generation_death_evidence_new RENAME TO process_generation_death_evidence;
