-- Preserve historical admissions while allowing fenced account rotation.
DROP TRIGGER chief_process_binding_authority;

CREATE TRIGGER chief_process_binding_authority BEFORE INSERT ON chief_process_bindings
BEGIN
	SELECT RAISE(ABORT, 'Chief root must be a root goal') WHERE NOT EXISTS (
		SELECT 1 FROM chief_work_items WHERE id = NEW.root_id AND kind = 'goal' AND parent_goal_id IS NULL
	);
	SELECT RAISE(ABORT, 'Previous Chief process must be dead before account rotation') WHERE EXISTS (
        SELECT 1 FROM chief_process_bindings b JOIN process_generations g ON g.generation_id = b.generation_id
        WHERE b.root_id = NEW.root_id AND b.account_id <> NEW.account_id AND g.state <> 'dead'
    );
    SELECT RAISE(ABORT, 'Chief work must be idle before account rotation') WHERE
        coalesce((SELECT account_id <> NEW.account_id FROM chief_process_bindings
            WHERE root_id = NEW.root_id ORDER BY created_at_micros DESC, rowid DESC LIMIT 1), 0)
        AND EXISTS (
            WITH RECURSIVE family(id) AS (
                SELECT NEW.root_id
                UNION SELECT w.id FROM chief_work_items w JOIN family f ON w.parent_goal_id = f.id
            )
            SELECT 1 FROM chief_work_items WHERE id IN (SELECT id FROM family) AND dispatch_state <> 'idle'
        );
	SELECT RAISE(ABORT, 'Chief generation ownership differs') WHERE NOT EXISTS (
		SELECT 1 FROM process_generations WHERE generation_id = NEW.generation_id
			AND account_id = NEW.account_id AND runtime_session_id IS NULL AND quick_task_admission_key IS NULL
	);
END;
