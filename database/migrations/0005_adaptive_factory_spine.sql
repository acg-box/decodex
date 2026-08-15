CREATE TABLE programs (
  program_id TEXT PRIMARY KEY CHECK (length(program_id) = 36),
  name TEXT NOT NULL CHECK (length(CAST(name AS BLOB)) BETWEEN 1 AND 256),
  purpose TEXT NOT NULL CHECK (length(CAST(purpose AS BLOB)) BETWEEN 1 AND 4096),
  non_goals_json TEXT NOT NULL CHECK (
    json_valid(non_goals_json) AND json_type(non_goals_json) = 'array' AND
    length(CAST(non_goals_json AS BLOB)) BETWEEN 3 AND 131072
  ),
  review_policy TEXT NOT NULL CHECK (
    length(CAST(review_policy AS BLOB)) BETWEEN 1 AND 4096
  ),
  state TEXT NOT NULL CHECK (state IN ('active', 'paused', 'retired')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros)
) STRICT;

CREATE TABLE program_entities (
  entity_id TEXT PRIMARY KEY CHECK (length(entity_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  kind TEXT NOT NULL CHECK (
    kind IN ('program', 'signal', 'claim', 'proposal', 'objective', 'work_item', 'evidence', 'review')
  ),
  UNIQUE (program_id, entity_id, kind)
) STRICT;

CREATE TABLE program_signals (
  signal_id TEXT PRIMARY KEY CHECK (length(signal_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  source TEXT NOT NULL CHECK (length(CAST(source AS BLOB)) BETWEEN 1 AND 4096),
  summary TEXT NOT NULL CHECK (length(CAST(summary AS BLOB)) BETWEEN 1 AND 4096),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros >= observed_at_micros)
) STRICT;

CREATE TABLE program_claims (
  claim_id TEXT PRIMARY KEY CHECK (length(claim_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  signal_id TEXT NOT NULL REFERENCES program_signals(signal_id),
  statement TEXT NOT NULL CHECK (length(CAST(statement AS BLOB)) BETWEEN 1 AND 4096),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  UNIQUE (program_id, signal_id)
) STRICT;

CREATE TABLE program_proposals (
  proposal_id TEXT PRIMARY KEY CHECK (length(proposal_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  claim_id TEXT NOT NULL REFERENCES program_claims(claim_id),
  summary TEXT NOT NULL CHECK (length(CAST(summary AS BLOB)) BETWEEN 1 AND 4096),
  expected_effect TEXT NOT NULL CHECK (
    length(CAST(expected_effect AS BLOB)) BETWEEN 1 AND 4096
  ),
  risk TEXT NOT NULL CHECK (length(CAST(risk AS BLOB)) BETWEEN 1 AND 4096),
  evidence_need TEXT NOT NULL CHECK (
    length(CAST(evidence_need AS BLOB)) BETWEEN 1 AND 4096
  ),
  executable INTEGER NOT NULL CHECK (executable = 0),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  UNIQUE (program_id, claim_id)
) STRICT;

CREATE TABLE program_objectives (
  objective_id TEXT PRIMARY KEY CHECK (length(objective_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  proposal_id TEXT NOT NULL REFERENCES program_proposals(proposal_id),
  outcome TEXT NOT NULL CHECK (length(CAST(outcome AS BLOB)) BETWEEN 1 AND 4096),
  acceptance_criteria_json TEXT NOT NULL CHECK (
    json_valid(acceptance_criteria_json) AND
    json_type(acceptance_criteria_json) = 'array' AND
    length(CAST(acceptance_criteria_json AS BLOB)) BETWEEN 3 AND 131072
  ),
  validation_criteria_json TEXT NOT NULL CHECK (
    json_valid(validation_criteria_json) AND
    json_type(validation_criteria_json) = 'array' AND
    length(CAST(validation_criteria_json AS BLOB)) BETWEEN 3 AND 131072
  ),
  state TEXT NOT NULL CHECK (state IN ('active', 'achieved', 'abandoned')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  UNIQUE (program_id, proposal_id)
) STRICT;

CREATE TABLE program_work_items (
  work_item_id TEXT PRIMARY KEY CHECK (length(work_item_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  objective_id TEXT NOT NULL REFERENCES program_objectives(objective_id),
  title TEXT NOT NULL CHECK (length(CAST(title AS BLOB)) BETWEEN 1 AND 256),
  instructions TEXT NOT NULL CHECK (length(CAST(instructions AS BLOB)) BETWEEN 1 AND 16384),
  working_directory TEXT NOT NULL CHECK (
    length(CAST(working_directory AS BLOB)) BETWEEN 1 AND 4096
  ),
  state TEXT NOT NULL CHECK (state IN ('ready', 'running', 'done')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
  UNIQUE (program_id, objective_id)
) STRICT;

CREATE TABLE program_work_item_executions (
  work_item_id TEXT PRIMARY KEY REFERENCES program_work_items(work_item_id),
  conversation_id TEXT NOT NULL UNIQUE REFERENCES conversations(conversation_id),
  bound_at_micros INTEGER NOT NULL CHECK (bound_at_micros > 0)
) STRICT;

CREATE TABLE program_evidence (
  evidence_id TEXT PRIMARY KEY CHECK (length(evidence_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  work_item_id TEXT NOT NULL REFERENCES program_work_items(work_item_id),
  kind TEXT NOT NULL CHECK (kind IN ('deterministic_validation', 'external')),
  source TEXT NOT NULL CHECK (length(CAST(source AS BLOB)) BETWEEN 1 AND 4096),
  summary TEXT NOT NULL CHECK (length(CAST(summary AS BLOB)) BETWEEN 1 AND 4096),
  observed_at_micros INTEGER NOT NULL CHECK (observed_at_micros > 0),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros >= observed_at_micros),
  UNIQUE (work_item_id, kind)
) STRICT;

CREATE TABLE program_reviews (
  review_id TEXT PRIMARY KEY CHECK (length(review_id) = 36),
  program_id TEXT NOT NULL REFERENCES programs(program_id),
  work_item_id TEXT NOT NULL UNIQUE REFERENCES program_work_items(work_item_id),
  deterministic_evidence_id TEXT NOT NULL UNIQUE REFERENCES program_evidence(evidence_id),
  external_evidence_id TEXT NOT NULL UNIQUE REFERENCES program_evidence(evidence_id),
  classification TEXT NOT NULL CHECK (
    classification IN (
      'outcome_progress',
      'knowledge_progress',
      'capability_progress',
      'no_material_change',
      'regression',
      'unknown'
    )
  ),
  rationale TEXT NOT NULL CHECK (length(CAST(rationale AS BLOB)) BETWEEN 1 AND 4096),
  created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0),
  CHECK (deterministic_evidence_id <> external_evidence_id)
) STRICT;

CREATE INDEX programs_recent ON programs(updated_at_micros DESC, program_id);
CREATE INDEX program_entities_by_program ON program_entities(program_id, kind, entity_id);
CREATE INDEX program_signals_by_program ON program_signals(program_id, created_at_micros, signal_id);
CREATE INDEX program_claims_by_program ON program_claims(program_id, created_at_micros, claim_id);
CREATE INDEX program_proposals_by_program ON program_proposals(program_id, created_at_micros, proposal_id);
CREATE INDEX program_objectives_by_program ON program_objectives(program_id, created_at_micros, objective_id);
CREATE INDEX program_work_items_by_program ON program_work_items(program_id, created_at_micros, work_item_id);
CREATE INDEX program_evidence_by_program ON program_evidence(program_id, created_at_micros, evidence_id);
CREATE INDEX program_reviews_by_program ON program_reviews(program_id, created_at_micros, review_id);
