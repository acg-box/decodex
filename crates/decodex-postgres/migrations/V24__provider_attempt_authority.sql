-- XY-1401 generic ProviderAttempt authority.
-- ProviderAttemptService is the only product caller of these commands. Runtime receives no
-- direct relation authority. Timeout, missing events or rows, exhausted lists, process death,
-- kqueue exit, boot change, EOF, restart, and negative search have no transition path to
-- not_submitted or any other terminal state.

CREATE TYPE decodex.provider_attempt_state AS ENUM (
	'prepared',
	'canceled',
	'dispatch_authorized',
	'succeeded',
	'failed_definitive',
	'not_submitted',
	'unknown'
);
CREATE TYPE decodex.provider_attempt_consumer_kind AS ENUM (
	'conversation_turn', 'managed_run_execution'
);
CREATE TYPE decodex.provider_attempt_unknown_reason AS ENUM (
	'supervision_lost', 'dispatch_outcome_unavailable', 'restore_projection'
);
CREATE TYPE decodex.provider_attempt_evidence_source AS ENUM (
	'provider_receipt',
	'positive_idempotency_lookup',
	'exact_turn_readback',
	'exact_thread_readback',
	'positive_non_submission_receipt'
);
CREATE TYPE decodex.provider_attempt_terminal_outcome AS ENUM (
	'succeeded', 'failed_definitive', 'not_submitted'
);

CREATE TABLE decodex.provider_attempts (
	attempt_id uuid PRIMARY KEY,
	consumer_kind decodex.provider_attempt_consumer_kind NOT NULL,
	conversation_id uuid,
	turn_id uuid,
	managed_run_id uuid,
	managed_run_revision bigint,
	managed_execution_id uuid,
	continuation_plan_id uuid NOT NULL UNIQUE
		REFERENCES decodex.continuation_plans(plan_id),
	routing_decision_id uuid NOT NULL
		REFERENCES decodex.routing_decisions(decision_id),
	accepted_runtime_session_id uuid NOT NULL,
	accepted_runtime_session_revision bigint NOT NULL CHECK (
		accepted_runtime_session_revision > 0
	),
	selected_account_id uuid NOT NULL REFERENCES decodex.accounts(account_id),
	process_generation_id uuid NOT NULL,
	process_generation_revision bigint NOT NULL CHECK (process_generation_revision > 0),
	process_execution_epoch_id uuid NOT NULL
		REFERENCES decodex.process_generation_execution_epochs(execution_epoch_id),
	request_id uuid NOT NULL UNIQUE,
	request_digest text NOT NULL CHECK (
		request_digest COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
	),
	provider_idempotency_key text,
	provider_correlation_key text,
	predecessor_attempt_id uuid REFERENCES decodex.provider_attempts(attempt_id),
	duplicate_risk_ack_digest text,
	state decodex.provider_attempt_state NOT NULL,
	unknown_reason decodex.provider_attempt_unknown_reason,
	terminal_evidence_id uuid,
	revision bigint NOT NULL CHECK (revision > 0),
	created_at timestamptz NOT NULL,
	updated_at timestamptz NOT NULL,
	CONSTRAINT provider_attempt_id_canonical CHECK (
		attempt_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND request_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND (
			managed_execution_id IS NULL
			OR managed_execution_id::text COLLATE pg_catalog."C" ~
				'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		)
	),
	CONSTRAINT provider_attempt_consumer_shape CHECK (
		(
			consumer_kind = 'conversation_turn'
			AND conversation_id IS NOT NULL
			AND turn_id IS NOT NULL
			AND managed_run_id IS NULL
			AND managed_run_revision IS NULL
			AND managed_execution_id IS NULL
		) OR (
			consumer_kind = 'managed_run_execution'
			AND conversation_id IS NULL
			AND turn_id IS NULL
			AND managed_run_id IS NOT NULL
			AND managed_run_revision IS NOT NULL
			AND managed_run_revision > 0
			AND managed_execution_id IS NOT NULL
		)
	),
	CONSTRAINT provider_attempt_request_keys CHECK (
		(
			provider_idempotency_key IS NOT NULL
			AND pg_catalog.octet_length(provider_idempotency_key) BETWEEN 1 AND 512
			AND provider_idempotency_key COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		) OR provider_idempotency_key IS NULL
	),
	CONSTRAINT provider_attempt_correlation_key CHECK (
		(
			provider_correlation_key IS NOT NULL
			AND pg_catalog.octet_length(provider_correlation_key) BETWEEN 1 AND 512
			AND provider_correlation_key COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		) OR provider_correlation_key IS NULL
	),
	CONSTRAINT provider_attempt_has_request_key CHECK (
		provider_idempotency_key IS NOT NULL OR provider_correlation_key IS NOT NULL
	),
	CONSTRAINT provider_attempt_duplicate_risk_shape CHECK (
		(predecessor_attempt_id IS NULL AND duplicate_risk_ack_digest IS NULL)
		OR (
			predecessor_attempt_id IS NOT NULL
			AND predecessor_attempt_id <> attempt_id
			AND duplicate_risk_ack_digest IS NOT NULL
			AND duplicate_risk_ack_digest COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
		)
	),
	CONSTRAINT provider_attempt_state_shape CHECK (
		(
			state IN ('prepared', 'canceled', 'dispatch_authorized')
			AND unknown_reason IS NULL
			AND terminal_evidence_id IS NULL
		) OR (
			state = 'unknown'
			AND unknown_reason IS NOT NULL
			AND terminal_evidence_id IS NULL
		) OR (
			state IN ('succeeded', 'failed_definitive', 'not_submitted')
			AND unknown_reason IS NULL
			AND terminal_evidence_id IS NOT NULL
		)
	),
	CONSTRAINT provider_attempt_finite_timestamps CHECK (
		pg_catalog.isfinite(created_at)
		AND pg_catalog.isfinite(updated_at)
		AND updated_at >= created_at
	),
	CONSTRAINT provider_attempt_runtime_session_fk FOREIGN KEY (
		accepted_runtime_session_id
	) REFERENCES decodex.runtime_sessions(runtime_session_id),
	CONSTRAINT provider_attempt_generation_revision_fk FOREIGN KEY (
		process_generation_id, process_generation_revision
	) REFERENCES decodex.process_generation_transitions(generation_id, revision),
	CONSTRAINT provider_attempt_conversation_fk FOREIGN KEY (
		conversation_id
	) REFERENCES decodex.conversations(conversation_id),
	CONSTRAINT provider_attempt_request_scope UNIQUE (
		attempt_id, request_id, selected_account_id
	)
);

CREATE UNIQUE INDEX provider_attempt_conversation_turn_unique
	ON decodex.provider_attempts(turn_id)
	WHERE consumer_kind = 'conversation_turn';
CREATE UNIQUE INDEX provider_attempt_managed_execution_unique
	ON decodex.provider_attempts(managed_execution_id)
	WHERE consumer_kind = 'managed_run_execution';
CREATE UNIQUE INDEX provider_attempt_account_idempotency_unique
	ON decodex.provider_attempts(selected_account_id, provider_idempotency_key)
	WHERE provider_idempotency_key IS NOT NULL;
CREATE INDEX provider_attempt_account_correlation_lookup
	ON decodex.provider_attempts(selected_account_id, provider_correlation_key)
	WHERE provider_correlation_key IS NOT NULL;
CREATE INDEX provider_attempt_reconciliation_order
	ON decodex.provider_attempts(state, attempt_id)
	WHERE state IN ('dispatch_authorized', 'unknown');
CREATE INDEX provider_attempt_account_state_order
	ON decodex.provider_attempts(selected_account_id, state, attempt_id);
CREATE INDEX provider_attempt_unknown_conversation_scope
	ON decodex.provider_attempts(conversation_id, attempt_id)
	WHERE state = 'unknown' AND consumer_kind = 'conversation_turn';
CREATE INDEX provider_attempt_unknown_managed_run_scope
	ON decodex.provider_attempts(managed_run_id, attempt_id)
	WHERE state = 'unknown' AND consumer_kind = 'managed_run_execution';

CREATE TABLE decodex.provider_attempt_positive_evidence (
	evidence_id uuid PRIMARY KEY,
	attempt_id uuid NOT NULL,
	attempt_revision bigint NOT NULL CHECK (attempt_revision > 0),
	request_id uuid NOT NULL,
	selected_account_id uuid NOT NULL,
	source decodex.provider_attempt_evidence_source NOT NULL,
	outcome decodex.provider_attempt_terminal_outcome NOT NULL,
	provider_key text NOT NULL CHECK (
		pg_catalog.octet_length(provider_key) >= 1
		AND pg_catalog.octet_length(provider_key) <= 512
		AND provider_key COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	),
	provider_receipt_id text,
	provider_thread_id text,
	provider_turn_id text,
	witness_digest text NOT NULL CHECK (
		witness_digest COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
	),
	observed_at timestamptz NOT NULL CHECK (pg_catalog.isfinite(observed_at)),
	CONSTRAINT provider_attempt_positive_evidence_id_canonical CHECK (
		evidence_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
		AND request_id::text COLLATE pg_catalog."C" ~
			'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
	),
	CONSTRAINT provider_attempt_positive_evidence_identities CHECK (
		(
			provider_receipt_id IS NULL
			OR (
				pg_catalog.octet_length(provider_receipt_id) >= 1
				AND pg_catalog.octet_length(provider_receipt_id) <= 512
				AND provider_receipt_id COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
			)
		) AND (
			provider_thread_id IS NULL
			OR (
				pg_catalog.octet_length(provider_thread_id) >= 1
				AND pg_catalog.octet_length(provider_thread_id) <= 512
				AND provider_thread_id COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
			)
		) AND (
			provider_turn_id IS NULL
			OR (
				pg_catalog.octet_length(provider_turn_id) >= 1
				AND pg_catalog.octet_length(provider_turn_id) <= 512
				AND provider_turn_id COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
			)
		)
	),
	CONSTRAINT provider_attempt_positive_evidence_shape CHECK (
		(
			source = 'provider_receipt'
			AND outcome IN ('succeeded', 'failed_definitive')
			AND provider_receipt_id IS NOT NULL
		) OR (
			source = 'positive_idempotency_lookup'
		) OR (
			source = 'exact_turn_readback'
			AND outcome IN ('succeeded', 'failed_definitive')
			AND provider_receipt_id IS NULL
			AND provider_thread_id IS NULL
			AND provider_turn_id IS NOT NULL
		) OR (
			source = 'exact_thread_readback'
			AND outcome IN ('succeeded', 'failed_definitive')
			AND provider_receipt_id IS NULL
			AND provider_thread_id IS NOT NULL
			AND provider_turn_id IS NOT NULL
		) OR (
			source = 'positive_non_submission_receipt'
			AND outcome = 'not_submitted'
			AND provider_receipt_id IS NOT NULL
			AND provider_turn_id IS NULL
		)
	),
	CONSTRAINT provider_attempt_positive_evidence_request_fk FOREIGN KEY (
		attempt_id, request_id, selected_account_id
	) REFERENCES decodex.provider_attempts(
		attempt_id, request_id, selected_account_id
	),
	CONSTRAINT provider_attempt_positive_evidence_scope
		UNIQUE (evidence_id, attempt_id)
);

CREATE UNIQUE INDEX provider_attempt_positive_receipt_unique
	ON decodex.provider_attempt_positive_evidence(
		selected_account_id, provider_receipt_id
	)
	WHERE provider_receipt_id IS NOT NULL;
CREATE UNIQUE INDEX provider_attempt_positive_turn_unique
	ON decodex.provider_attempt_positive_evidence(
		selected_account_id, provider_turn_id
	)
	WHERE provider_turn_id IS NOT NULL;

ALTER TABLE decodex.provider_attempts
	ADD CONSTRAINT provider_attempt_terminal_evidence_fk
	FOREIGN KEY (terminal_evidence_id, attempt_id)
	REFERENCES decodex.provider_attempt_positive_evidence(evidence_id, attempt_id)
	DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE decodex.provider_attempt_transitions (
	attempt_id uuid NOT NULL REFERENCES decodex.provider_attempts(attempt_id),
	revision bigint NOT NULL CHECK (revision > 0),
	previous_state decodex.provider_attempt_state,
	state decodex.provider_attempt_state NOT NULL,
	unknown_reason decodex.provider_attempt_unknown_reason,
	terminal_evidence_id uuid,
	transitioned_at timestamptz NOT NULL CHECK (pg_catalog.isfinite(transitioned_at)),
	PRIMARY KEY (attempt_id, revision),
	CONSTRAINT provider_attempt_transition_first CHECK (
		(revision = 1) = (previous_state IS NULL)
	),
	CONSTRAINT provider_attempt_transition_state_shape CHECK (
		(
			state IN ('prepared', 'canceled', 'dispatch_authorized')
			AND unknown_reason IS NULL
			AND terminal_evidence_id IS NULL
		) OR (
			state = 'unknown'
			AND unknown_reason IS NOT NULL
			AND terminal_evidence_id IS NULL
		) OR (
			state IN ('succeeded', 'failed_definitive', 'not_submitted')
			AND unknown_reason IS NULL
			AND terminal_evidence_id IS NOT NULL
		)
	),
	CONSTRAINT provider_attempt_transition_evidence_fk FOREIGN KEY (
		terminal_evidence_id, attempt_id
	) REFERENCES decodex.provider_attempt_positive_evidence(evidence_id, attempt_id)
	DEFERRABLE INITIALLY DEFERRED
);

ALTER TABLE decodex.provider_attempt_positive_evidence
	ADD CONSTRAINT provider_attempt_positive_evidence_revision_fk
	FOREIGN KEY (attempt_id, attempt_revision)
	REFERENCES decodex.provider_attempt_transitions(attempt_id, revision)
	DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION decodex.enforce_provider_attempt_transition()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF TG_OP = 'INSERT' THEN
		IF NEW.revision <> 1 OR NEW.state <> 'prepared'
			OR NEW.unknown_reason IS NOT NULL
			OR NEW.terminal_evidence_id IS NOT NULL
		THEN
			RAISE EXCEPTION 'provider attempt must begin as revision-one prepared'
				USING ERRCODE='23514',
					CONSTRAINT='provider_attempt_initial_state';
		END IF;
		RETURN NEW;
	END IF;

	IF (
		NEW.attempt_id,
		NEW.consumer_kind,
		NEW.conversation_id,
		NEW.turn_id,
		NEW.managed_run_id,
		NEW.managed_run_revision,
		NEW.managed_execution_id,
		NEW.continuation_plan_id,
		NEW.routing_decision_id,
		NEW.accepted_runtime_session_id,
		NEW.accepted_runtime_session_revision,
		NEW.selected_account_id,
		NEW.process_generation_id,
		NEW.process_generation_revision,
		NEW.process_execution_epoch_id,
		NEW.request_id,
		NEW.request_digest,
		NEW.provider_idempotency_key,
		NEW.provider_correlation_key,
		NEW.predecessor_attempt_id,
		NEW.duplicate_risk_ack_digest,
		NEW.created_at
	) IS DISTINCT FROM (
		OLD.attempt_id,
		OLD.consumer_kind,
		OLD.conversation_id,
		OLD.turn_id,
		OLD.managed_run_id,
		OLD.managed_run_revision,
		OLD.managed_execution_id,
		OLD.continuation_plan_id,
		OLD.routing_decision_id,
		OLD.accepted_runtime_session_id,
		OLD.accepted_runtime_session_revision,
		OLD.selected_account_id,
		OLD.process_generation_id,
		OLD.process_generation_revision,
		OLD.process_execution_epoch_id,
		OLD.request_id,
		OLD.request_digest,
		OLD.provider_idempotency_key,
		OLD.provider_correlation_key,
		OLD.predecessor_attempt_id,
		OLD.duplicate_risk_ack_digest,
		OLD.created_at
	)
		OR NEW.revision <> OLD.revision + 1
		OR NEW.updated_at < OLD.updated_at
	THEN
		RAISE EXCEPTION 'provider attempt immutable authority or revision changed'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_immutable_authority';
	END IF;

	IF NOT (
		(OLD.state = 'prepared' AND NEW.state IN ('canceled', 'dispatch_authorized'))
		OR (
			OLD.state = 'prepared'
			AND NEW.state = 'unknown'
			AND NEW.unknown_reason = 'restore_projection'
		)
		OR (
			OLD.state = 'dispatch_authorized'
			AND NEW.state IN ('succeeded', 'failed_definitive', 'not_submitted', 'unknown')
		)
		OR (
			OLD.state = 'unknown'
			AND NEW.state IN ('succeeded', 'failed_definitive', 'not_submitted')
		)
	) THEN
		RAISE EXCEPTION 'illegal provider attempt transition'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_legal_transition';
	END IF;

	IF NEW.state IN ('succeeded', 'failed_definitive', 'not_submitted')
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.provider_attempt_positive_evidence AS evidence
			WHERE (evidence.evidence_id, evidence.attempt_id, evidence.outcome::text) =
				(NEW.terminal_evidence_id, NEW.attempt_id, NEW.state::text)
		)
	THEN
		RAISE EXCEPTION 'terminal provider attempt lacks exact positive evidence'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_positive_terminal_evidence';
	END IF;

	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.enforce_provider_attempt_binding()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1
		FROM decodex.continuation_plans AS plan
		JOIN decodex.routing_decisions AS decision
			ON decision.decision_id = plan.routing_decision_id
		JOIN decodex.runtime_sessions AS session
			ON (
				session.runtime_session_id,
				session.revision
			) = (
				NEW.accepted_runtime_session_id,
				NEW.accepted_runtime_session_revision
			)
		JOIN decodex.account_snapshots AS account
			ON account.account_snapshot_id = session.account_snapshot_id
		JOIN decodex.process_generations AS generation
			ON generation.generation_id = NEW.process_generation_id
		JOIN decodex.process_generation_transitions AS generation_transition
			ON (
				generation_transition.generation_id,
				generation_transition.revision
			) = (
				NEW.process_generation_id,
				NEW.process_generation_revision
			)
		JOIN decodex.process_generation_execution_epochs AS epoch
			ON epoch.execution_epoch_id = generation.execution_epoch_id
		WHERE plan.plan_id = NEW.continuation_plan_id
			AND plan.routing_decision_id = NEW.routing_decision_id
			AND decision.kind = 'selected'
			AND decision.selected_account_id = NEW.selected_account_id
			AND plan.selected_account_id = NEW.selected_account_id
			AND NOT plan.dispatch_enabled
			AND NOT plan.replay_permitted
			AND (
				(
					plan.kind = 'same_thread'
					AND NEW.accepted_runtime_session_id =
						plan.source_runtime_session_id
					AND NEW.accepted_runtime_session_revision =
						plan.source_runtime_session_revision
					AND session.state = 'active'
				) OR (
					plan.kind = 'context_pack_fallback'
					AND NEW.accepted_runtime_session_id =
						plan.fallback_runtime_session_id
					AND NEW.accepted_runtime_session_revision = 1
					AND session.state = 'starting'
				)
			)
			AND account.source_account_id = NEW.selected_account_id
			AND generation.account_id = NEW.selected_account_id
			AND generation.execution_epoch_id = NEW.process_execution_epoch_id
			AND generation.revision = NEW.process_generation_revision
			AND generation.state = 'ready'
			AND generation.process_id IS NOT NULL
			AND generation_transition.state = 'ready'
			AND epoch.retired_at IS NULL
	) THEN
		RAISE EXCEPTION 'provider attempt has forged V16, V17, or ProcessGeneration lineage'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_authority_complete';
	END IF;

	IF NEW.consumer_kind = 'conversation_turn' THEN
		IF NOT EXISTS (
			SELECT 1
			FROM decodex.conversations AS conversation
			JOIN decodex.continuation_plans AS plan
				ON plan.plan_id = NEW.continuation_plan_id
			WHERE conversation.conversation_id = NEW.conversation_id
				AND conversation.status = 'open'
				AND plan.conversation_id = NEW.conversation_id
		) OR EXISTS (
			SELECT 1
			FROM decodex.turns AS turn
			WHERE turn.turn_id = NEW.turn_id
				AND (
					turn.conversation_id <> NEW.conversation_id
					OR turn.runtime_session_id <>
						NEW.accepted_runtime_session_id
					OR turn.status <> 'active'
				)
		) THEN
			RAISE EXCEPTION 'provider attempt Conversation reserved-turn binding is incomplete'
				USING ERRCODE='23514',
					CONSTRAINT='provider_attempt_consumer_complete';
		END IF;
	ELSE
		IF NOT EXISTS (
			SELECT 1
			FROM decodex.managed_runs AS run
			JOIN decodex.continuation_plans AS plan
				ON plan.plan_id = NEW.continuation_plan_id
			WHERE (run.managed_run_id, run.revision) =
				(NEW.managed_run_id, NEW.managed_run_revision)
				AND (plan.managed_run_id, plan.managed_run_revision) =
					(NEW.managed_run_id, NEW.managed_run_revision)
		) THEN
			RAISE EXCEPTION 'provider attempt ManagedRun execution binding is incomplete'
				USING ERRCODE='23514',
					CONSTRAINT='provider_attempt_consumer_complete';
		END IF;
	END IF;

	IF NEW.predecessor_attempt_id IS NULL THEN
		IF EXISTS (
			SELECT 1
			FROM decodex.provider_attempts AS predecessor
			WHERE predecessor.state = 'unknown'
				AND predecessor.attempt_id <> NEW.attempt_id
				AND predecessor.consumer_kind = NEW.consumer_kind
				AND (
					(
						NEW.consumer_kind = 'conversation_turn'
						AND predecessor.conversation_id = NEW.conversation_id
					) OR (
						NEW.consumer_kind = 'managed_run_execution'
						AND predecessor.managed_run_id = NEW.managed_run_id
					)
				)
		) THEN
			RAISE EXCEPTION 'new intent requires exact duplicate-risk acknowledgement'
				USING ERRCODE='23514',
					CONSTRAINT='provider_attempt_duplicate_risk_ack_required';
		END IF;
	ELSIF NOT EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS predecessor
		WHERE predecessor.attempt_id = NEW.predecessor_attempt_id
			AND predecessor.state = 'unknown'
			AND predecessor.consumer_kind = NEW.consumer_kind
			AND predecessor.request_id <> NEW.request_id
			AND (
				(
					NEW.consumer_kind = 'conversation_turn'
					AND predecessor.conversation_id = NEW.conversation_id
					AND predecessor.turn_id <> NEW.turn_id
				) OR (
					NEW.consumer_kind = 'managed_run_execution'
					AND predecessor.managed_run_id = NEW.managed_run_id
					AND predecessor.managed_execution_id <> NEW.managed_execution_id
				)
			)
	) OR EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS unresolved
		WHERE unresolved.state = 'unknown'
			AND unresolved.attempt_id NOT IN (
				NEW.attempt_id, NEW.predecessor_attempt_id
			)
			AND unresolved.consumer_kind = NEW.consumer_kind
			AND (
				(
					NEW.consumer_kind = 'conversation_turn'
					AND unresolved.conversation_id = NEW.conversation_id
				) OR (
					NEW.consumer_kind = 'managed_run_execution'
					AND unresolved.managed_run_id = NEW.managed_run_id
				)
			)
	) THEN
		RAISE EXCEPTION 'duplicate-risk acknowledgement does not bind one exact unknown attempt'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_duplicate_risk_ack_invalid';
	END IF;

	RETURN NULL;
END;
$$;

CREATE FUNCTION decodex.record_provider_attempt_transition()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	INSERT INTO decodex.provider_attempt_transitions(
		attempt_id,
		revision,
		previous_state,
		state,
		unknown_reason,
		terminal_evidence_id,
		transitioned_at
	) VALUES (
		NEW.attempt_id,
		NEW.revision,
		CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE OLD.state END,
		NEW.state,
		NEW.unknown_reason,
		NEW.terminal_evidence_id,
		NEW.updated_at
	);
	RETURN NULL;
END;
$$;

-- Runtime owns direct Turn writes but has no ProviderAttempt relation privilege. Run this
-- trigger as the migration owner so unrelated Turns can pass the reservation lookup while a
-- conflicting reserved identity still fails closed. Direct execution is revoked below.
CREATE FUNCTION decodex.enforce_provider_attempt_turn_materialization()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS attempt
		WHERE attempt.consumer_kind = 'conversation_turn'
			AND attempt.turn_id = NEW.turn_id
			AND (
				attempt.conversation_id <> NEW.conversation_id
				OR attempt.accepted_runtime_session_id <> NEW.runtime_session_id
			)
	) THEN
		RAISE EXCEPTION 'Turn conflicts with one reserved ProviderAttempt identity'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_turn_materialization';
	END IF;
	RETURN NEW;
END;
$$;

CREATE FUNCTION decodex.forbid_provider_attempt_history_mutation()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	RAISE EXCEPTION 'provider attempt authority history is append-only'
		USING ERRCODE='23514',
			CONSTRAINT='provider_attempt_history_immutable';
END;
$$;

CREATE TRIGGER provider_attempt_transition_guard
	BEFORE INSERT OR UPDATE
	ON decodex.provider_attempts
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_provider_attempt_transition();
CREATE CONSTRAINT TRIGGER provider_attempt_binding_complete
	AFTER INSERT ON decodex.provider_attempts
	DEFERRABLE INITIALLY DEFERRED
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_provider_attempt_binding();
CREATE TRIGGER provider_attempt_delete_immutable
	BEFORE DELETE OR TRUNCATE
	ON decodex.provider_attempts
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_provider_attempt_history_mutation();
CREATE TRIGGER provider_attempt_transition_record
	AFTER INSERT OR UPDATE
	ON decodex.provider_attempts
	FOR EACH ROW EXECUTE FUNCTION decodex.record_provider_attempt_transition();
CREATE TRIGGER turns_provider_attempt_materialization
	BEFORE INSERT OR UPDATE
	ON decodex.turns
	FOR EACH ROW EXECUTE FUNCTION decodex.enforce_provider_attempt_turn_materialization();
CREATE TRIGGER provider_attempt_positive_evidence_immutable
	BEFORE UPDATE OR DELETE OR TRUNCATE
	ON decodex.provider_attempt_positive_evidence
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_provider_attempt_history_mutation();
CREATE TRIGGER provider_attempt_transitions_immutable
	BEFORE UPDATE OR DELETE OR TRUNCATE
	ON decodex.provider_attempt_transitions
	FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_provider_attempt_history_mutation();

CREATE FUNCTION decodex.prepare_provider_attempt_exact(
	p_attempt_id uuid,
	p_consumer_kind decodex.provider_attempt_consumer_kind,
	p_conversation_id uuid,
	p_turn_id uuid,
	p_managed_run_id uuid,
	p_managed_run_revision bigint,
	p_managed_execution_id uuid,
	p_continuation_plan_id uuid,
	p_process_generation_id uuid,
	p_process_generation_revision bigint,
	p_request_id uuid,
	p_request_digest text,
	p_provider_idempotency_key text,
	p_provider_correlation_key text,
	p_predecessor_attempt_id uuid,
	p_duplicate_risk_ack_digest text
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.provider_attempt_state,
	created_at_micros bigint,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE existing decodex.provider_attempts%ROWTYPE;
DECLARE plan decodex.continuation_plans%ROWTYPE;
DECLARE generation decodex.process_generations%ROWTYPE;
DECLARE accepted_session_id uuid;
DECLARE accepted_session_revision bigint;
DECLARE now_value timestamptz;
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock_shared(1400);
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1401,
		pg_catalog.hashtext(
			COALESCE(p_consumer_kind::text, 'invalid') || ':' ||
			COALESCE(p_conversation_id::text, p_managed_run_id::text, 'invalid')
		)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1402, pg_catalog.hashtext(p_attempt_id::text)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1403, pg_catalog.hashtext(p_request_id::text)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1404, pg_catalog.hashtext(p_continuation_plan_id::text)
	);

	SELECT * INTO existing
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id
	FOR UPDATE;
	IF FOUND THEN
		IF (
			existing.consumer_kind,
			existing.conversation_id,
			existing.turn_id,
			existing.managed_run_id,
			existing.managed_run_revision,
			existing.managed_execution_id,
			existing.continuation_plan_id,
			existing.process_generation_id,
			existing.process_generation_revision,
			existing.request_id,
			existing.request_digest,
			existing.provider_idempotency_key,
			existing.provider_correlation_key,
			existing.predecessor_attempt_id,
			existing.duplicate_risk_ack_digest
		) IS DISTINCT FROM (
			p_consumer_kind,
			p_conversation_id,
			p_turn_id,
			p_managed_run_id,
			p_managed_run_revision,
			p_managed_execution_id,
			p_continuation_plan_id,
			p_process_generation_id,
			p_process_generation_revision,
			p_request_id,
			p_request_digest,
			p_provider_idempotency_key,
			p_provider_correlation_key,
			p_predecessor_attempt_id,
			p_duplicate_risk_ack_digest
		) THEN
			RETURN QUERY SELECT
				'identity_conflict',
				existing.revision,
				existing.state,
				(extract(epoch FROM existing.created_at)*1000000)::bigint,
				(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		ELSE
			RETURN QUERY SELECT
				'replayed',
				existing.revision,
				existing.state,
				(extract(epoch FROM existing.created_at)*1000000)::bigint,
				(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;

	IF p_attempt_id IS NULL
		OR p_continuation_plan_id IS NULL
		OR p_process_generation_id IS NULL
		OR p_process_generation_revision IS NULL
		OR p_process_generation_revision <= 0
		OR p_request_id IS NULL
		OR p_request_digest IS NULL
		OR p_request_digest COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
		OR (p_provider_idempotency_key IS NULL AND p_provider_correlation_key IS NULL)
		OR (
			p_provider_idempotency_key IS NOT NULL
			AND (
				pg_catalog.octet_length(p_provider_idempotency_key) NOT BETWEEN 1 AND 512
				OR p_provider_idempotency_key COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			)
		)
		OR (
			p_provider_correlation_key IS NOT NULL
			AND (
				pg_catalog.octet_length(p_provider_correlation_key) NOT BETWEEN 1 AND 512
				OR p_provider_correlation_key COLLATE pg_catalog."C" ~ '[[:cntrl:]]'
			)
		)
		OR NOT COALESCE((
			(
				p_consumer_kind = 'conversation_turn'
				AND p_conversation_id IS NOT NULL
				AND p_turn_id IS NOT NULL
				AND p_managed_run_id IS NULL
				AND p_managed_run_revision IS NULL
				AND p_managed_execution_id IS NULL
			) OR (
				p_consumer_kind = 'managed_run_execution'
				AND p_conversation_id IS NULL
				AND p_turn_id IS NULL
				AND p_managed_run_id IS NOT NULL
				AND p_managed_run_revision IS NOT NULL
				AND p_managed_run_revision > 0
				AND p_managed_execution_id IS NOT NULL
			)
		), false)
		OR (
			(p_predecessor_attempt_id IS NULL) <>
				(p_duplicate_risk_ack_digest IS NULL)
		)
		OR (
			p_duplicate_risk_ack_digest IS NOT NULL
			AND p_duplicate_risk_ack_digest COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
		)
	THEN
		RETURN QUERY SELECT 'invalid_input', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;

	SELECT * INTO plan
	FROM decodex.continuation_plans
	WHERE plan_id = p_continuation_plan_id
	FOR SHARE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'authority_unavailable', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;
	IF plan.kind = 'same_thread' THEN
		accepted_session_id := plan.source_runtime_session_id;
		accepted_session_revision := plan.source_runtime_session_revision;
	ELSE
		accepted_session_id := plan.fallback_runtime_session_id;
		accepted_session_revision := 1;
	END IF;

	SELECT * INTO generation
	FROM decodex.process_generations
	WHERE generation_id = p_process_generation_id
	FOR SHARE;
	IF NOT FOUND
		OR generation.revision <> p_process_generation_revision
		OR generation.state <> 'ready'
		OR generation.account_id <> plan.selected_account_id
		OR generation.process_id IS NULL
		OR NOT EXISTS (
			SELECT 1
			FROM decodex.process_generation_execution_epochs AS epoch
			WHERE epoch.execution_epoch_id = generation.execution_epoch_id
				AND epoch.retired_at IS NULL
		)
	THEN
		RETURN QUERY SELECT 'generation_unavailable', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;

	IF NOT EXISTS (
		SELECT 1
		FROM decodex.routing_decisions AS decision
		JOIN decodex.runtime_sessions AS session
			ON (
				session.runtime_session_id,
				session.revision
			) = (
				accepted_session_id,
				accepted_session_revision
			)
		JOIN decodex.account_snapshots AS account
			ON account.account_snapshot_id = session.account_snapshot_id
		WHERE decision.decision_id = plan.routing_decision_id
			AND decision.kind = 'selected'
			AND decision.selected_account_id = plan.selected_account_id
			AND account.source_account_id = plan.selected_account_id
			AND (
				(plan.kind = 'same_thread' AND session.state = 'active')
				OR (
					plan.kind = 'context_pack_fallback'
					AND session.state = 'starting'
				)
			)
		FOR SHARE OF decision, session, account
	) THEN
		RETURN QUERY SELECT 'authority_unavailable', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;

	IF (
		p_consumer_kind = 'conversation_turn'
		AND (
			NOT EXISTS (
				SELECT 1
				FROM decodex.conversations AS conversation
				WHERE conversation.conversation_id = p_conversation_id
					AND conversation.status = 'open'
					AND conversation.conversation_id = plan.conversation_id
				FOR SHARE
			) OR EXISTS (
				SELECT 1
				FROM decodex.turns AS turn
				WHERE turn.turn_id = p_turn_id
					AND (
						turn.conversation_id <> p_conversation_id
						OR turn.runtime_session_id <> accepted_session_id
						OR turn.status <> 'active'
					)
			)
		)
	) OR (
		p_consumer_kind = 'managed_run_execution'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.managed_runs AS run
			WHERE (run.managed_run_id, run.revision) =
				(p_managed_run_id, p_managed_run_revision)
				AND (plan.managed_run_id, plan.managed_run_revision) =
					(p_managed_run_id, p_managed_run_revision)
			FOR SHARE
		)
	) THEN
		RETURN QUERY SELECT 'consumer_unavailable', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;

	PERFORM pg_catalog.pg_advisory_xact_lock(
		1405,
		pg_catalog.hashtext(
			plan.selected_account_id::text || ':' ||
			COALESCE(p_provider_idempotency_key, '')
		)
	);
	IF EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS assigned
		WHERE assigned.request_id = p_request_id
			OR assigned.continuation_plan_id = p_continuation_plan_id
			OR (
				p_provider_idempotency_key IS NOT NULL
				AND assigned.selected_account_id = plan.selected_account_id
				AND assigned.provider_idempotency_key = p_provider_idempotency_key
			)
	) THEN
		RETURN QUERY SELECT 'identity_conflict', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint, 0::bigint;
		RETURN;
	END IF;

	now_value := pg_catalog.clock_timestamp();
	INSERT INTO decodex.provider_attempts(
		attempt_id,
		consumer_kind,
		conversation_id,
		turn_id,
		managed_run_id,
		managed_run_revision,
		managed_execution_id,
		continuation_plan_id,
		routing_decision_id,
		accepted_runtime_session_id,
		accepted_runtime_session_revision,
		selected_account_id,
		process_generation_id,
		process_generation_revision,
		process_execution_epoch_id,
		request_id,
		request_digest,
		provider_idempotency_key,
		provider_correlation_key,
		predecessor_attempt_id,
		duplicate_risk_ack_digest,
		state,
		revision,
		created_at,
		updated_at
	) VALUES (
		p_attempt_id,
		p_consumer_kind,
		p_conversation_id,
		p_turn_id,
		p_managed_run_id,
		p_managed_run_revision,
		p_managed_execution_id,
		p_continuation_plan_id,
		plan.routing_decision_id,
		accepted_session_id,
		accepted_session_revision,
		plan.selected_account_id,
		p_process_generation_id,
		p_process_generation_revision,
		generation.execution_epoch_id,
		p_request_id,
		p_request_digest,
		p_provider_idempotency_key,
		p_provider_correlation_key,
		p_predecessor_attempt_id,
		p_duplicate_risk_ack_digest,
		'prepared',
		1,
		now_value,
		now_value
	);
	RETURN QUERY SELECT 'prepared', 1::bigint,
		'prepared'::decodex.provider_attempt_state,
		(extract(epoch FROM now_value)*1000000)::bigint,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.authorize_provider_attempt_dispatch_exact(
	p_attempt_id uuid,
	p_expected_revision bigint,
	p_process_generation_id uuid,
	p_process_generation_revision bigint
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.provider_attempt_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE attempt decodex.provider_attempts%ROWTYPE;
DECLARE scope_kind decodex.provider_attempt_consumer_kind;
DECLARE scope_conversation_id uuid;
DECLARE scope_managed_run_id uuid;
DECLARE now_value timestamptz;
BEGIN
	PERFORM pg_catalog.pg_advisory_xact_lock_shared(1400);
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	SELECT consumer_kind, conversation_id, managed_run_id
	INTO scope_kind, scope_conversation_id, scope_managed_run_id
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'attempt_missing', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint;
		RETURN;
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1401,
		pg_catalog.hashtext(
			scope_kind::text || ':' ||
			COALESCE(scope_conversation_id::text, scope_managed_run_id::text)
		)
	);
	SELECT * INTO attempt
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id
	FOR UPDATE;
	IF p_expected_revision IS NULL
		OR p_expected_revision <= 0
		OR p_process_generation_id IS NULL
		OR p_process_generation_revision IS NULL
		OR p_process_generation_revision <= 0
	THEN
		RETURN QUERY SELECT 'invalid_input', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF attempt.state = 'dispatch_authorized' THEN
		RETURN QUERY SELECT
			CASE
				WHEN p_expected_revision = attempt.revision - 1
					AND (
						p_process_generation_id,
						p_process_generation_revision
					) = (
						attempt.process_generation_id,
						attempt.process_generation_revision
					)
				THEN 'replayed'
				ELSE 'stale_attempt'
			END,
			attempt.revision,
			attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF attempt.state <> 'prepared'
		OR attempt.revision <> p_expected_revision
		OR (
			attempt.process_generation_id,
			attempt.process_generation_revision
		) <> (
			p_process_generation_id,
			p_process_generation_revision
		)
	THEN
		RETURN QUERY SELECT 'stale_attempt', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS unresolved
		WHERE unresolved.state = 'unknown'
			AND unresolved.attempt_id <> attempt.attempt_id
			AND unresolved.attempt_id IS DISTINCT FROM attempt.predecessor_attempt_id
			AND unresolved.consumer_kind = attempt.consumer_kind
			AND (
				(
					attempt.consumer_kind = 'conversation_turn'
					AND unresolved.conversation_id = attempt.conversation_id
				) OR (
					attempt.consumer_kind = 'managed_run_execution'
					AND unresolved.managed_run_id = attempt.managed_run_id
				)
			)
	) THEN
		RETURN QUERY SELECT 'authority_unavailable', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF NOT EXISTS (
		SELECT 1
		FROM decodex.continuation_plans AS plan
		JOIN decodex.routing_decisions AS decision
			ON decision.decision_id = plan.routing_decision_id
		JOIN decodex.runtime_sessions AS session
			ON (
				session.runtime_session_id,
				session.revision
			) = (
				attempt.accepted_runtime_session_id,
				attempt.accepted_runtime_session_revision
			)
		JOIN decodex.account_snapshots AS account
			ON account.account_snapshot_id = session.account_snapshot_id
		WHERE plan.plan_id = attempt.continuation_plan_id
			AND plan.routing_decision_id = attempt.routing_decision_id
			AND decision.kind = 'selected'
			AND decision.selected_account_id = attempt.selected_account_id
			AND plan.selected_account_id = attempt.selected_account_id
			AND NOT plan.dispatch_enabled
			AND NOT plan.replay_permitted
			AND (
				(
					plan.kind = 'same_thread'
					AND attempt.accepted_runtime_session_id =
						plan.source_runtime_session_id
					AND attempt.accepted_runtime_session_revision =
						plan.source_runtime_session_revision
					AND session.state = 'active'
				) OR (
					plan.kind = 'context_pack_fallback'
					AND attempt.accepted_runtime_session_id =
						plan.fallback_runtime_session_id
					AND attempt.accepted_runtime_session_revision = 1
					AND session.state = 'starting'
				)
			)
			AND account.source_account_id = attempt.selected_account_id
		FOR SHARE OF plan, decision, session, account
	) THEN
		RETURN QUERY SELECT 'authority_unavailable', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF (
		attempt.consumer_kind = 'conversation_turn'
		AND (
			NOT EXISTS (
				SELECT 1
				FROM decodex.conversations AS conversation
				JOIN decodex.continuation_plans AS plan
					ON plan.plan_id = attempt.continuation_plan_id
				WHERE conversation.conversation_id = attempt.conversation_id
					AND conversation.status = 'open'
					AND plan.conversation_id = attempt.conversation_id
				FOR SHARE OF conversation
			) OR EXISTS (
				SELECT 1
				FROM decodex.turns AS turn
				WHERE turn.turn_id = attempt.turn_id
					AND (
						turn.conversation_id <> attempt.conversation_id
						OR turn.runtime_session_id <>
							attempt.accepted_runtime_session_id
						OR turn.status <> 'active'
					)
			)
		)
	) OR (
		attempt.consumer_kind = 'managed_run_execution'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.managed_runs AS run
			JOIN decodex.continuation_plans AS plan
				ON plan.plan_id = attempt.continuation_plan_id
			WHERE (run.managed_run_id, run.revision) =
				(attempt.managed_run_id, attempt.managed_run_revision)
				AND (plan.managed_run_id, plan.managed_run_revision) =
					(attempt.managed_run_id, attempt.managed_run_revision)
			FOR SHARE OF run
		)
	) THEN
		RETURN QUERY SELECT 'consumer_unavailable', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF NOT EXISTS (
		SELECT 1
		FROM decodex.process_generations AS generation
		JOIN decodex.process_generation_execution_epochs AS epoch
			ON epoch.execution_epoch_id = generation.execution_epoch_id
		WHERE generation.generation_id = attempt.process_generation_id
			AND generation.revision = attempt.process_generation_revision
			AND generation.account_id = attempt.selected_account_id
			AND generation.execution_epoch_id = attempt.process_execution_epoch_id
			AND generation.state = 'ready'
			AND generation.process_id IS NOT NULL
			AND epoch.retired_at IS NULL
		FOR SHARE OF generation, epoch
	) THEN
		RETURN QUERY SELECT 'generation_unavailable', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;

	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.provider_attempts AS target
	SET state = 'dispatch_authorized',
		revision = target.revision + 1,
		updated_at = now_value
	WHERE target.attempt_id = p_attempt_id;
	RETURN QUERY SELECT 'dispatch_authorized', p_expected_revision + 1,
		'dispatch_authorized'::decodex.provider_attempt_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.cancel_provider_attempt_exact(
	p_attempt_id uuid,
	p_expected_revision bigint
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.provider_attempt_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE attempt decodex.provider_attempts%ROWTYPE;
DECLARE now_value timestamptz;
BEGIN
	SELECT * INTO attempt
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id
	FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'attempt_missing', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL OR p_expected_revision <= 0 THEN
		RETURN QUERY SELECT 'invalid_input', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF attempt.state = 'canceled' THEN
		RETURN QUERY SELECT
			CASE WHEN p_expected_revision = attempt.revision - 1
				THEN 'replayed' ELSE 'stale_attempt' END,
			attempt.revision,
			attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF attempt.state <> 'prepared' OR attempt.revision <> p_expected_revision THEN
		RETURN QUERY SELECT 'stale_attempt', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.provider_attempts AS target
	SET state = 'canceled',
		revision = target.revision + 1,
		updated_at = now_value
	WHERE target.attempt_id = p_attempt_id;
	RETURN QUERY SELECT 'canceled', p_expected_revision + 1,
		'canceled'::decodex.provider_attempt_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.mark_provider_attempt_unknown_exact(
	p_attempt_id uuid,
	p_expected_revision bigint,
	p_reason decodex.provider_attempt_unknown_reason
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.provider_attempt_state,
	updated_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE attempt decodex.provider_attempts%ROWTYPE;
DECLARE scope_kind decodex.provider_attempt_consumer_kind;
DECLARE scope_conversation_id uuid;
DECLARE scope_managed_run_id uuid;
DECLARE now_value timestamptz;
BEGIN
	SELECT consumer_kind, conversation_id, managed_run_id
	INTO scope_kind, scope_conversation_id, scope_managed_run_id
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'attempt_missing', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint;
		RETURN;
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1401,
		pg_catalog.hashtext(
			scope_kind::text || ':' ||
			COALESCE(scope_conversation_id::text, scope_managed_run_id::text)
		)
	);
	SELECT * INTO attempt
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id
	FOR UPDATE;
	IF p_expected_revision IS NULL
		OR p_expected_revision <= 0
		OR p_reason IS NULL
		OR p_reason = 'restore_projection'
	THEN
		RETURN QUERY SELECT 'invalid_input', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF attempt.state = 'unknown' THEN
		RETURN QUERY SELECT
			CASE
				WHEN p_expected_revision = attempt.revision - 1
					AND p_reason = attempt.unknown_reason
				THEN 'replayed'
				ELSE 'stale_attempt'
			END,
			attempt.revision,
			attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	IF attempt.state <> 'dispatch_authorized'
		OR attempt.revision <> p_expected_revision
	THEN
		RETURN QUERY SELECT 'stale_attempt', attempt.revision, attempt.state,
			(extract(epoch FROM attempt.updated_at)*1000000)::bigint;
		RETURN;
	END IF;
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.provider_attempts AS target
	SET state = 'unknown',
		unknown_reason = p_reason,
		revision = target.revision + 1,
		updated_at = now_value
	WHERE target.attempt_id = p_attempt_id;
	RETURN QUERY SELECT 'unknown', p_expected_revision + 1,
		'unknown'::decodex.provider_attempt_state,
		(extract(epoch FROM now_value)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.record_provider_attempt_positive_evidence_exact(
	p_attempt_id uuid,
	p_expected_revision bigint,
	p_evidence_id uuid,
	p_request_id uuid,
	p_source decodex.provider_attempt_evidence_source,
	p_outcome decodex.provider_attempt_terminal_outcome,
	p_provider_key text,
	p_provider_receipt_id text,
	p_provider_thread_id text,
	p_provider_turn_id text,
	p_witness_digest text
) RETURNS TABLE(
	result_code text,
	revision bigint,
	state decodex.provider_attempt_state,
	observed_at_micros bigint
) LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE attempt decodex.provider_attempts%ROWTYPE;
DECLARE evidence decodex.provider_attempt_positive_evidence%ROWTYPE;
DECLARE session_thread_id text;
DECLARE observed timestamptz;
BEGIN
	SELECT * INTO attempt
	FROM decodex.provider_attempts
	WHERE attempt_id = p_attempt_id
	FOR UPDATE;
	IF NOT FOUND THEN
		RETURN QUERY SELECT 'attempt_missing', 0::bigint,
			'prepared'::decodex.provider_attempt_state, 0::bigint;
		RETURN;
	END IF;
	IF p_expected_revision IS NULL OR p_expected_revision <= 0 THEN
		RETURN QUERY SELECT 'invalid_input', attempt.revision, attempt.state, 0::bigint;
		RETURN;
	END IF;

	SELECT * INTO evidence
	FROM decodex.provider_attempt_positive_evidence
	WHERE evidence_id = p_evidence_id;
	IF FOUND THEN
		IF (
			evidence.attempt_id,
			evidence.request_id,
			evidence.selected_account_id,
			evidence.source,
			evidence.outcome,
			evidence.provider_key,
			evidence.provider_receipt_id,
			evidence.provider_thread_id,
			evidence.provider_turn_id,
			evidence.witness_digest
		) IS NOT DISTINCT FROM (
			p_attempt_id,
			p_request_id,
			attempt.selected_account_id,
			p_source,
			p_outcome,
			p_provider_key,
			p_provider_receipt_id,
			p_provider_thread_id,
			p_provider_turn_id,
			p_witness_digest
		)
			AND p_expected_revision IN (
				evidence.attempt_revision, attempt.revision
			)
			AND attempt.terminal_evidence_id = p_evidence_id
			AND attempt.state::text = p_outcome::text
		THEN
			RETURN QUERY SELECT
				'replayed',
				attempt.revision,
				attempt.state,
				(extract(epoch FROM evidence.observed_at)*1000000)::bigint;
		ELSE
			RETURN QUERY SELECT
				'evidence_conflict',
				attempt.revision,
				attempt.state,
				(extract(epoch FROM evidence.observed_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;

	IF attempt.state NOT IN ('dispatch_authorized', 'unknown')
		OR attempt.revision <> p_expected_revision
	THEN
		RETURN QUERY SELECT 'stale_attempt', attempt.revision, attempt.state, 0::bigint;
		RETURN;
	END IF;
	IF p_request_id <> attempt.request_id
		OR (
			p_provider_key IS DISTINCT FROM attempt.provider_idempotency_key
			AND p_provider_key IS DISTINCT FROM attempt.provider_correlation_key
		)
		OR (
			p_source = 'positive_idempotency_lookup'
			AND p_provider_key IS DISTINCT FROM attempt.provider_idempotency_key
		)
		OR p_witness_digest IS NULL
		OR p_witness_digest COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
	THEN
		RETURN QUERY SELECT 'evidence_mismatch', attempt.revision, attempt.state, 0::bigint;
		RETURN;
	END IF;

	IF (
		p_source = 'provider_receipt'
		AND (
			p_outcome = 'not_submitted'
			OR p_provider_receipt_id IS NULL
		)
	) OR (
		p_source = 'exact_turn_readback'
		AND (
			p_outcome = 'not_submitted'
			OR p_provider_receipt_id IS NOT NULL
			OR p_provider_thread_id IS NOT NULL
			OR p_provider_turn_id IS NULL
		)
	) OR (
		p_source = 'exact_thread_readback'
		AND (
			p_outcome = 'not_submitted'
			OR p_provider_receipt_id IS NOT NULL
			OR p_provider_thread_id IS NULL
			OR p_provider_turn_id IS NULL
		)
	) OR (
		p_source = 'positive_non_submission_receipt'
		AND (
			p_outcome <> 'not_submitted'
			OR p_provider_receipt_id IS NULL
			OR p_provider_turn_id IS NOT NULL
		)
	) THEN
		RETURN QUERY SELECT 'invalid_evidence', attempt.revision, attempt.state, 0::bigint;
		RETURN;
	END IF;

	SELECT session.codex_thread_id INTO session_thread_id
	FROM decodex.runtime_sessions AS session
	WHERE session.runtime_session_id = attempt.accepted_runtime_session_id;
	IF p_source = 'exact_thread_readback'
		AND p_provider_thread_id IS DISTINCT FROM session_thread_id
	THEN
		RETURN QUERY SELECT 'evidence_mismatch', attempt.revision, attempt.state, 0::bigint;
		RETURN;
	END IF;

	observed := pg_catalog.clock_timestamp();
	INSERT INTO decodex.provider_attempt_positive_evidence(
		evidence_id,
		attempt_id,
		attempt_revision,
		request_id,
		selected_account_id,
		source,
		outcome,
		provider_key,
		provider_receipt_id,
		provider_thread_id,
		provider_turn_id,
		witness_digest,
		observed_at
	) VALUES (
		p_evidence_id,
		p_attempt_id,
		p_expected_revision,
		p_request_id,
		attempt.selected_account_id,
		p_source,
		p_outcome,
		p_provider_key,
		p_provider_receipt_id,
		p_provider_thread_id,
		p_provider_turn_id,
		p_witness_digest,
		observed
	);
	UPDATE decodex.provider_attempts AS target
	SET state = p_outcome::text::decodex.provider_attempt_state,
		unknown_reason = NULL,
		terminal_evidence_id = p_evidence_id,
		revision = target.revision + 1,
		updated_at = observed
	WHERE target.attempt_id = p_attempt_id;
	RETURN QUERY SELECT
		p_outcome::text,
		p_expected_revision + 1,
		p_outcome::text::decodex.provider_attempt_state,
		(extract(epoch FROM observed)*1000000)::bigint;
END;
$$;

CREATE FUNCTION decodex.project_provider_attempts_after_supervisor_loss_exact()
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE changed bigint;
DECLARE now_value timestamptz;
BEGIN
	-- The exclusive restore gate excludes both generation launch fencing and attempt
	-- preparation/dispatch authorization. A restored prepared row can be older than a later
	-- authorization, so restore projects it to unknown as an exceptional fail-closed view.
	PERFORM pg_catalog.pg_advisory_xact_lock(1400);
	now_value := pg_catalog.clock_timestamp();
	UPDATE decodex.provider_attempts
	SET state = 'unknown',
		unknown_reason = 'restore_projection',
		revision = revision + 1,
		updated_at = now_value
	WHERE state IN ('prepared', 'dispatch_authorized');
	GET DIAGNOSTICS changed = ROW_COUNT;
	RETURN changed;
END;
$$;

CREATE FUNCTION decodex.read_provider_attempts_exact(
	p_attempt_id uuid,
	p_account_id uuid,
	p_state decodex.provider_attempt_state,
	p_after_attempt_id uuid,
	p_limit bigint
) RETURNS TABLE(
	attempt_id uuid,
	consumer_kind decodex.provider_attempt_consumer_kind,
	conversation_id uuid,
	turn_id uuid,
	managed_run_id uuid,
	managed_run_revision bigint,
	managed_execution_id uuid,
	continuation_plan_id uuid,
	routing_decision_id uuid,
	accepted_runtime_session_id uuid,
	accepted_runtime_session_revision bigint,
	selected_account_id uuid,
	process_generation_id uuid,
	process_generation_revision bigint,
	process_execution_epoch_id uuid,
	request_id uuid,
	request_digest text,
	provider_idempotency_key text,
	provider_correlation_key text,
	predecessor_attempt_id uuid,
	duplicate_risk_ack_digest text,
	state decodex.provider_attempt_state,
	unknown_reason decodex.provider_attempt_unknown_reason,
	terminal_evidence_id uuid,
	revision bigint,
	created_at_micros bigint,
	updated_at_micros bigint
) LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF p_limit IS NULL OR p_limit < 1 OR p_limit > 256 THEN
		RAISE EXCEPTION 'provider attempt read limit is invalid'
			USING ERRCODE='22023';
	END IF;
	RETURN QUERY
	SELECT
		attempt.attempt_id,
		attempt.consumer_kind,
		attempt.conversation_id,
		attempt.turn_id,
		attempt.managed_run_id,
		attempt.managed_run_revision,
		attempt.managed_execution_id,
		attempt.continuation_plan_id,
		attempt.routing_decision_id,
		attempt.accepted_runtime_session_id,
		attempt.accepted_runtime_session_revision,
		attempt.selected_account_id,
		attempt.process_generation_id,
		attempt.process_generation_revision,
		attempt.process_execution_epoch_id,
		attempt.request_id,
		attempt.request_digest,
		attempt.provider_idempotency_key,
		attempt.provider_correlation_key,
		attempt.predecessor_attempt_id,
		attempt.duplicate_risk_ack_digest,
		attempt.state,
		attempt.unknown_reason,
		attempt.terminal_evidence_id,
		attempt.revision,
		(extract(epoch FROM attempt.created_at)*1000000)::bigint,
		(extract(epoch FROM attempt.updated_at)*1000000)::bigint
	FROM decodex.provider_attempts AS attempt
	WHERE (p_attempt_id IS NULL OR attempt.attempt_id = p_attempt_id)
		AND (p_account_id IS NULL OR attempt.selected_account_id = p_account_id)
		AND (p_state IS NULL OR attempt.state = p_state)
		AND (p_after_attempt_id IS NULL OR attempt.attempt_id > p_after_attempt_id)
	ORDER BY attempt.attempt_id
	LIMIT p_limit;
END;
$$;

REVOKE ALL ON TYPE
	decodex.provider_attempt_state,
	decodex.provider_attempt_consumer_kind,
	decodex.provider_attempt_unknown_reason,
	decodex.provider_attempt_evidence_source,
	decodex.provider_attempt_terminal_outcome
	FROM PUBLIC;
REVOKE ALL ON TABLE
	decodex.provider_attempts,
	decodex.provider_attempt_positive_evidence,
	decodex.provider_attempt_transitions
	FROM PUBLIC;
REVOKE ALL ON FUNCTION
	decodex.enforce_provider_attempt_transition(),
	decodex.enforce_provider_attempt_binding(),
	decodex.record_provider_attempt_transition(),
	decodex.enforce_provider_attempt_turn_materialization(),
	decodex.forbid_provider_attempt_history_mutation(),
	decodex.prepare_provider_attempt_exact(uuid,decodex.provider_attempt_consumer_kind,uuid,uuid,uuid,bigint,uuid,uuid,uuid,bigint,uuid,text,text,text,uuid,text),
	decodex.authorize_provider_attempt_dispatch_exact(uuid,bigint,uuid,bigint),
	decodex.cancel_provider_attempt_exact(uuid,bigint),
	decodex.mark_provider_attempt_unknown_exact(uuid,bigint,decodex.provider_attempt_unknown_reason),
	decodex.record_provider_attempt_positive_evidence_exact(uuid,bigint,uuid,uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,text,text,text,text,text),
	decodex.project_provider_attempts_after_supervisor_loss_exact(),
	decodex.read_provider_attempts_exact(uuid,uuid,decodex.provider_attempt_state,uuid,bigint)
	FROM PUBLIC;

-- Derive the one configured runtime principal from the existing migration-owned anchor.
-- Runtime receives enum USAGE and only the seven ProviderAttemptService entrypoints.
DO $$
DECLARE anchor_oid pg_catalog.oid;
DECLARE migration_role_oid pg_catalog.oid;
DECLARE owner_execute_count pg_catalog.int8;
DECLARE runtime_role_count pg_catalog.int8;
DECLARE invalid_execute_count pg_catalog.int8;
DECLARE runtime_role pg_catalog.name;
BEGIN
	SELECT role.oid INTO migration_role_oid
	FROM pg_catalog.pg_roles AS role
	WHERE role.rolname = current_user;
	anchor_oid := pg_catalog.to_regprocedure(
		'decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)'
	);
	IF anchor_oid IS NULL OR NOT EXISTS (
		SELECT 1
		FROM pg_catalog.pg_proc AS procedure
		WHERE procedure.oid = anchor_oid
			AND procedure.proowner = migration_role_oid
	) THEN
		RAISE EXCEPTION 'V24 runtime principal anchor is missing or not migration-owned'
			USING ERRCODE='42501';
	END IF;
	SELECT
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee = migration_role_oid
		),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee <> migration_role_oid
				AND role.oid IS NOT NULL
		),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee = 0
				OR privilege.grantor <> migration_role_oid
				OR (
					privilege.grantee <> migration_role_oid
					AND (privilege.is_grantable OR role.oid IS NULL)
				)
		),
		pg_catalog.min(role.rolname) FILTER (
			WHERE privilege.grantee <> migration_role_oid
				AND role.oid IS NOT NULL
		)
	INTO
		owner_execute_count,
		runtime_role_count,
		invalid_execute_count,
		runtime_role
	FROM pg_catalog.pg_proc AS procedure
	CROSS JOIN LATERAL pg_catalog.aclexplode(
		COALESCE(
			procedure.proacl,
			pg_catalog.acldefault('f', procedure.proowner)
		)
	) AS privilege
	LEFT JOIN pg_catalog.pg_roles AS role
		ON role.oid = privilege.grantee
	WHERE procedure.oid = anchor_oid
		AND privilege.privilege_type = 'EXECUTE';
	IF owner_execute_count <> 1
		OR runtime_role_count > 1
		OR invalid_execute_count <> 0
	THEN
		RAISE EXCEPTION 'V24 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count = 1 THEN
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON TABLE decodex.provider_attempts, '
			|| 'decodex.provider_attempt_positive_evidence, '
			|| 'decodex.provider_attempt_transitions FROM %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON TYPE decodex.provider_attempt_state, '
			|| 'decodex.provider_attempt_consumer_kind, '
			|| 'decodex.provider_attempt_unknown_reason, '
			|| 'decodex.provider_attempt_evidence_source, '
			|| 'decodex.provider_attempt_terminal_outcome FROM %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'REVOKE ALL ON FUNCTION '
			|| 'decodex.enforce_provider_attempt_transition(), '
			|| 'decodex.enforce_provider_attempt_binding(), '
			|| 'decodex.record_provider_attempt_transition(), '
			|| 'decodex.enforce_provider_attempt_turn_materialization(), '
			|| 'decodex.forbid_provider_attempt_history_mutation(), '
			|| 'decodex.prepare_provider_attempt_exact(uuid,decodex.provider_attempt_consumer_kind,uuid,uuid,uuid,bigint,uuid,uuid,uuid,bigint,uuid,text,text,text,uuid,text), '
			|| 'decodex.authorize_provider_attempt_dispatch_exact(uuid,bigint,uuid,bigint), '
			|| 'decodex.cancel_provider_attempt_exact(uuid,bigint), '
			|| 'decodex.mark_provider_attempt_unknown_exact(uuid,bigint,decodex.provider_attempt_unknown_reason), '
			|| 'decodex.record_provider_attempt_positive_evidence_exact(uuid,bigint,uuid,uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,text,text,text,text,text), '
			|| 'decodex.project_provider_attempts_after_supervisor_loss_exact(), '
			|| 'decodex.read_provider_attempts_exact(uuid,uuid,decodex.provider_attempt_state,uuid,bigint) '
			|| 'FROM %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT USAGE ON TYPE decodex.provider_attempt_state, '
			|| 'decodex.provider_attempt_consumer_kind, '
			|| 'decodex.provider_attempt_unknown_reason, '
			|| 'decodex.provider_attempt_evidence_source, '
			|| 'decodex.provider_attempt_terminal_outcome TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.prepare_provider_attempt_exact(uuid,decodex.provider_attempt_consumer_kind,uuid,uuid,uuid,bigint,uuid,uuid,uuid,bigint,uuid,text,text,text,uuid,text) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.authorize_provider_attempt_dispatch_exact(uuid,bigint,uuid,bigint) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.cancel_provider_attempt_exact(uuid,bigint) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.mark_provider_attempt_unknown_exact(uuid,bigint,decodex.provider_attempt_unknown_reason) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.record_provider_attempt_positive_evidence_exact(uuid,bigint,uuid,uuid,decodex.provider_attempt_evidence_source,decodex.provider_attempt_terminal_outcome,text,text,text,text,text) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.project_provider_attempts_after_supervisor_loss_exact() TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_provider_attempts_exact(uuid,uuid,decodex.provider_attempt_state,uuid,bigint) TO %I',
			runtime_role
		);
	END IF;
END;
$$;
