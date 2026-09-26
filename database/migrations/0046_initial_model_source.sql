-- Retain the source of initial execution choices across routing and restart.
-- NULL pairs preserve requests created before account-bound discovery.
ALTER TABLE quick_task_requests ADD COLUMN model_source_account_id TEXT;
ALTER TABLE quick_task_requests ADD COLUMN model_source_account_revision INTEGER
CHECK (
  (model_source_account_id IS NULL AND model_source_account_revision IS NULL)
  OR (model_source_account_id IS NOT NULL AND model_source_account_revision IS NOT NULL
      AND model_source_account_revision > 0)
);

ALTER TABLE quick_task_requests ADD COLUMN model_source_review_required INTEGER NOT NULL DEFAULT 0
CHECK (model_source_review_required IN (0, 1));
