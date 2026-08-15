ALTER TABLE program_signals
ADD COLUMN predecessor_review_id TEXT REFERENCES program_reviews(review_id);

CREATE UNIQUE INDEX program_signals_one_root_per_program
ON program_signals(program_id)
WHERE predecessor_review_id IS NULL;

CREATE UNIQUE INDEX program_signals_one_continuation_per_review
ON program_signals(predecessor_review_id)
WHERE predecessor_review_id IS NOT NULL;

CREATE TRIGGER program_signal_predecessor_same_program_insert
BEFORE INSERT ON program_signals
WHEN NEW.predecessor_review_id IS NOT NULL
  AND NOT EXISTS (
    SELECT 1
    FROM program_reviews
    WHERE review_id = NEW.predecessor_review_id
      AND program_id = NEW.program_id
  )
BEGIN
  SELECT RAISE(ABORT, 'program Signal predecessor must be a Review in the same Program');
END;

CREATE TRIGGER program_signal_predecessor_same_program_update
BEFORE UPDATE OF program_id, predecessor_review_id ON program_signals
WHEN NEW.predecessor_review_id IS NOT NULL
  AND NOT EXISTS (
    SELECT 1
    FROM program_reviews
    WHERE review_id = NEW.predecessor_review_id
      AND program_id = NEW.program_id
  )
BEGIN
  SELECT RAISE(ABORT, 'program Signal predecessor must be a Review in the same Program');
END;
