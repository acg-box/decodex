CREATE TABLE chief_work_items (
	id TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 512),
	parent_goal_id TEXT REFERENCES chief_work_items(id),
	kind TEXT NOT NULL CHECK (kind IN ('goal', 'task')),
	title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 1024),
	instructions TEXT NOT NULL CHECK (length(instructions) BETWEEN 1 AND 65536),
	codex_thread_id TEXT UNIQUE CHECK (length(CAST(codex_thread_id AS BLOB)) BETWEEN 1 AND 512),
	dispatch_state TEXT NOT NULL DEFAULT 'idle' CHECK (dispatch_state IN ('idle', 'dispatching', 'running', 'unknown')),
	active_turn_id TEXT CHECK (length(CAST(active_turn_id AS BLOB)) BETWEEN 1 AND 512),
	status TEXT NOT NULL CHECK (status IN ('open', 'resolved', 'follow_up', 'wait', 'user_decision')),
	next_check_at_micros INTEGER CHECK (next_check_at_micros >= 0),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros >= 0),
	updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros >= created_at_micros),
	CHECK (parent_goal_id IS NULL OR parent_goal_id <> id),
	CHECK ((dispatch_state = 'running' AND active_turn_id IS NOT NULL)
		OR dispatch_state = 'unknown'
		OR (dispatch_state IN ('idle', 'dispatching') AND active_turn_id IS NULL))
) STRICT;

CREATE INDEX chief_work_due ON chief_work_items(next_check_at_micros)
WHERE next_check_at_micros IS NOT NULL;

CREATE TRIGGER chief_parent_goal BEFORE INSERT ON chief_work_items
WHEN NEW.parent_goal_id IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'parent must be a goal')
	WHERE NOT EXISTS (SELECT 1 FROM chief_work_items WHERE id = NEW.parent_goal_id AND kind = 'goal');
END;

CREATE TRIGGER chief_work_identity_immutable
BEFORE UPDATE OF id, parent_goal_id, kind ON chief_work_items
BEGIN
	SELECT RAISE(ABORT, 'work identity is immutable');
END;

CREATE TABLE chief_dependencies (
	work_item_id TEXT NOT NULL REFERENCES chief_work_items(id),
	depends_on_id TEXT NOT NULL REFERENCES chief_work_items(id),
	PRIMARY KEY (work_item_id, depends_on_id),
	CHECK (work_item_id <> depends_on_id)
) STRICT;

CREATE TRIGGER chief_dependency_acyclic BEFORE INSERT ON chief_dependencies
BEGIN
	SELECT RAISE(ABORT, 'dependency cycle') WHERE EXISTS (
		WITH RECURSIVE ancestors(id) AS (
			SELECT NEW.depends_on_id
			UNION
			SELECT depends_on_id FROM chief_dependencies JOIN ancestors ON work_item_id = ancestors.id
		)
		SELECT 1 FROM ancestors WHERE id = NEW.work_item_id
	);
END;

CREATE TRIGGER chief_dependency_immutable BEFORE UPDATE ON chief_dependencies
BEGIN
	SELECT RAISE(ABORT, 'dependency is immutable');
END;

CREATE TABLE chief_inbox_events (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	source_event_id TEXT NOT NULL UNIQUE CHECK (length(source_event_id) BETWEEN 1 AND 2048),
	work_item_id TEXT NOT NULL REFERENCES chief_work_items(id),
	event_kind TEXT NOT NULL CHECK (length(event_kind) BETWEEN 1 AND 128),
	payload TEXT NOT NULL CHECK (length(payload) <= 65536),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros >= 0),
	disposition TEXT CHECK (disposition IN ('resolved', 'follow_up', 'wait', 'user_decision')),
	disposition_note TEXT CHECK (length(disposition_note) BETWEEN 1 AND 65536),
	disposed_at_micros INTEGER CHECK (disposed_at_micros >= created_at_micros),
	delivery_work_item_id TEXT REFERENCES chief_work_items(id),
	delivered_turn_id TEXT,
	CHECK ((delivery_work_item_id IS NULL AND delivered_turn_id IS NULL)
		OR (delivery_work_item_id IS NOT NULL AND delivered_turn_id IS NOT NULL)),
	CHECK ((disposition IS NULL AND disposition_note IS NULL AND disposed_at_micros IS NULL)
		OR (disposition IS NOT NULL AND disposition_note IS NOT NULL AND disposed_at_micros IS NOT NULL))
) STRICT;

CREATE INDEX chief_inbox_pending ON chief_inbox_events(id) WHERE disposition IS NULL;

CREATE TRIGGER chief_inbox_source_immutable
BEFORE UPDATE OF source_event_id, work_item_id, event_kind, payload, created_at_micros ON chief_inbox_events
BEGIN
	SELECT RAISE(ABORT, 'event source is immutable');
END;

CREATE TRIGGER chief_inbox_disposition_once BEFORE UPDATE ON chief_inbox_events
WHEN OLD.disposition IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'event already disposed');
END;

CREATE TABLE chief_process_bindings (
	operation_key TEXT PRIMARY KEY CHECK (length(CAST(operation_key AS BLOB)) BETWEEN 1 AND 256),
	root_id TEXT NOT NULL REFERENCES chief_work_items(id),
	account_id TEXT NOT NULL REFERENCES accounts(account_id),
	generation_id TEXT NOT NULL UNIQUE REFERENCES process_generations(generation_id),
	request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE INDEX chief_process_root ON chief_process_bindings(root_id, created_at_micros);

CREATE TRIGGER chief_process_binding_authority BEFORE INSERT ON chief_process_bindings
BEGIN
	SELECT RAISE(ABORT, 'Chief root must be a root goal') WHERE NOT EXISTS (
		SELECT 1 FROM chief_work_items WHERE id = NEW.root_id AND kind = 'goal' AND parent_goal_id IS NULL
	);
	SELECT RAISE(ABORT, 'Chief account affinity is immutable') WHERE EXISTS (
		SELECT 1 FROM chief_process_bindings WHERE root_id = NEW.root_id AND account_id <> NEW.account_id
	);
	SELECT RAISE(ABORT, 'Chief generation ownership differs') WHERE NOT EXISTS (
		SELECT 1 FROM process_generations WHERE generation_id = NEW.generation_id
			AND account_id = NEW.account_id AND runtime_session_id IS NULL AND quick_task_admission_key IS NULL
	);
END;

CREATE TABLE chief_root_settings (
	root_id TEXT PRIMARY KEY REFERENCES chief_work_items(id),
	config_json TEXT NOT NULL CHECK (length(CAST(config_json AS BLOB)) BETWEEN 2 AND 16384),
	created_at_micros INTEGER NOT NULL CHECK (created_at_micros > 0)
) STRICT;

CREATE TRIGGER chief_root_settings_authority BEFORE INSERT ON chief_root_settings
BEGIN
	SELECT RAISE(ABORT, 'Chief settings require a root goal') WHERE NOT EXISTS (
		SELECT 1 FROM chief_work_items WHERE id = NEW.root_id AND kind = 'goal' AND parent_goal_id IS NULL
	);
END;

CREATE TRIGGER chief_root_settings_immutable BEFORE UPDATE ON chief_root_settings
BEGIN
	SELECT RAISE(ABORT, 'Chief root settings are immutable');
END;

CREATE TRIGGER chief_process_binding_immutable BEFORE UPDATE ON chief_process_bindings
BEGIN
	SELECT RAISE(ABORT, 'Chief process admission is immutable');
END;
