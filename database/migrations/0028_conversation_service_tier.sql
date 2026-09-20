-- NULL preserves the meaning of the existing immutable Fast flag.
ALTER TABLE quick_task_requests
ADD COLUMN service_tier TEXT
CHECK (service_tier IS NULL OR (
    length(CAST(service_tier AS BLOB)) BETWEEN 1 AND 64
    AND service_tier NOT GLOB '*[^a-zA-Z0-9_.-]*'
));
