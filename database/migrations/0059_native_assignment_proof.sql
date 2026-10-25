-- Keep visible-prompt verification distinct from an encrypted native assignment.
ALTER TABLE native_work_deliveries ADD COLUMN assignment_proof TEXT NOT NULL
 DEFAULT 'plaintext' CHECK(assignment_proof IN('plaintext','native_task_name'));
