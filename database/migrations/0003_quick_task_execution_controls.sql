ALTER TABLE quick_task_requests
ADD COLUMN model TEXT NOT NULL DEFAULT 'gpt-5.6-sol'
CHECK (length(CAST(model AS BLOB)) BETWEEN 1 AND 128);

ALTER TABLE quick_task_requests
ADD COLUMN reasoning_effort TEXT NOT NULL DEFAULT 'high'
CHECK (reasoning_effort IN ('low', 'medium', 'high', 'xhigh', 'max', 'ultra'));

ALTER TABLE quick_task_requests
ADD COLUMN fast INTEGER NOT NULL DEFAULT 0 CHECK (fast IN (0, 1));
