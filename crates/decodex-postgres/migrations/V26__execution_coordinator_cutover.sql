-- XY-1402 V26 stateless execution-coordinator cutover.
--
-- V16 remains the complete account decision writer. V17 remains the RuntimeSession reuse or
-- atomic fallback writer. ProcessGeneration and ProviderAttempt retain their sole writers.
-- Conversation and ManagedRun retain their domain tables. This migration creates no coordinator
-- relation, retry ledger, dispatch gate, provider gateway, credential switch, or remote transport.

LOCK TABLE decodex.routing_snapshots, decodex.routing_decisions,
	decodex.routing_decision_blocker_refs, decodex.continuation_plans,
	decodex.provider_attempts, decodex.managed_runs,
	decodex.managed_run_effect_barriers, decodex.managed_run_effects,
	decodex.managed_run_submitted_turn_receipts, decodex.managed_run_safety_inputs,
	decodex.activity, decodex.outbox, decodex.exact_command_receipts
	IN ACCESS EXCLUSIVE MODE;

-- The retired V12 authority has no lossless mapping to ProviderAttempt. Require a coordinated
-- drained cutover rather than create a compatibility ledger or infer non-submission.
DO $$
BEGIN
	IF EXISTS (SELECT 1 FROM decodex.managed_run_effect_barriers)
		OR EXISTS (SELECT 1 FROM decodex.managed_run_effects)
		OR EXISTS (SELECT 1 FROM decodex.managed_run_submitted_turn_receipts)
		OR EXISTS (SELECT 1 FROM decodex.managed_run_safety_inputs)
		OR EXISTS (
			SELECT 1 FROM decodex.exact_command_receipts
			WHERE request_envelope->>'operation'='apply_managed_run_safety_input'
		)
	THEN
		RAISE EXCEPTION 'V26 requires drained V12 ManagedRun turn-effect authority'
			USING ERRCODE='55000',
				CONSTRAINT='execution_coordinator_v12_drained_cutover';
	END IF;
	IF EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS attempt
		WHERE attempt.consumer_kind::text='conversation_turn'
	) THEN
		RAISE EXCEPTION 'V26 cannot convert a historical ordinary ProviderAttempt whose V17 lineage is ManagedRun-local'
			USING ERRCODE='55000',
				CONSTRAINT='execution_coordinator_historical_conversation_cross_link';
	END IF;
	IF EXISTS (
		SELECT decision.snapshot_id
		FROM decodex.routing_decisions AS decision
		JOIN decodex.continuation_plans AS plan
			ON plan.routing_decision_id=decision.decision_id
		JOIN decodex.provider_attempts AS attempt
			ON attempt.continuation_plan_id=plan.plan_id
		WHERE attempt.consumer_kind::text='managed_run_execution'
		GROUP BY decision.snapshot_id
		HAVING pg_catalog.count(DISTINCT attempt.managed_execution_id)>1
	) THEN
		RAISE EXCEPTION 'V26 cannot compose one historical snapshot with distinct accepted ProviderAttempt intents'
			USING ERRCODE='55000',
				CONSTRAINT='execution_coordinator_historical_intent_ambiguous';
	END IF;
END
$$;

CREATE FUNCTION decodex.read_execution_decision_exact(p_decision_id uuid)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
BEGIN
	RETURN (
	SELECT pg_catalog.jsonb_build_object(
		'decision_id',decision.decision_id,
		'consumer_kind',decision.consumer_kind,
		'conversation_id',decision.conversation_id,
		'conversation_revision',decision.conversation_revision,
		'source_runtime_session_id',CASE
			WHEN decision.consumer_kind::text='conversation_turn'
				THEN snapshot.runtime_session_id
		END,
		'source_runtime_session_revision',CASE
			WHEN decision.consumer_kind::text='conversation_turn'
				THEN snapshot.runtime_session_revision
		END,
		'turn_id',decision.turn_id,
		'managed_run_id',decision.managed_run_id,
		'managed_run_revision',decision.managed_run_revision,
		'managed_execution_id',decision.managed_execution_id,
		'kind',decision.kind,
		'selected_account_id',decision.selected_account_id,
		'waiting_ready_at_micros',decision.waiting_ready_at_micros,
		'no_route_reason',decision.no_route_reason,
		'causes',CASE WHEN decision.kind::text='selected' THEN '[]'::jsonb
			ELSE (
				SELECT COALESCE(
					pg_catalog.jsonb_agg(
						pg_catalog.jsonb_build_object(
							'account_id',blocker.account_id,
							'blocker',blocker.blocker
						)
						ORDER BY member.position,blocker.position
					),
					'[]'::jsonb
				)
				FROM decodex.routing_decision_blocker_refs AS blocker
				JOIN decodex.routing_snapshot_members AS member
					ON member.snapshot_id=blocker.snapshot_id
					AND member.account_id=blocker.account_id
				WHERE blocker.decision_id=decision.decision_id
					AND (
						decision.kind::text='no_route'
						OR member.disposition='included'
					)
			) END,
		'quota_exclusions',(
			SELECT COALESCE(
				pg_catalog.jsonb_agg(
					pg_catalog.jsonb_build_object(
						'account_id',exclusion.account_id,
						'window_class',exclusion.window_class,
						'duration_minutes',exclusion.duration_minutes,
						'observation_revision',
							exclusion.observation_revision,
						'resets_at_micros',exclusion.resets_at_micros
					)
					ORDER BY exclusion.member_position,
						exclusion.window_class
				),
				'[]'::jsonb
			)
			FROM decodex.routing_decision_exclusions AS exclusion
			WHERE exclusion.decision_id=decision.decision_id
		)
	)
	FROM decodex.routing_decisions AS decision
	JOIN decodex.routing_snapshots AS snapshot
		ON snapshot.snapshot_id=decision.snapshot_id
	WHERE decision.decision_id=p_decision_id
	);
END
$$;

CREATE FUNCTION decodex.read_managed_run_execution_exact(
	p_managed_run_id uuid,p_project_id uuid,p_expected_revision bigint
) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
	SELECT pg_catalog.jsonb_build_object(
		'managed_run_id',run.managed_run_id,'project_id',run.project_id,
		'work_item_id',run.work_item_id,'runtime_session_id',run.runtime_session_id,
		'runtime_session_revision',run.runtime_session_revision,
		'runtime_session_state',session.state,'lifecycle',run.lifecycle,'phase',run.phase,
		'wait_reason',run.wait_reason,'revision',run.revision,'diverged',run.diverged,
		'blocked',run.blocked,'created_at',run.created_at,'updated_at',run.updated_at,
		'assignments',COALESCE((
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.jsonb_build_object(
					'runtime_session_id',assignment.runtime_session_id,
					'role',assignment.role
				)
				ORDER BY assignment.role
			)
			FROM decodex.managed_run_assignments AS assignment
			WHERE assignment.managed_run_id=run.managed_run_id
		),'[]'::jsonb),
		'provider_attempts',COALESCE((
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.jsonb_build_object(
					'execution_id',attempt.managed_execution_id,
					'attempt_id',attempt.attempt_id,
					'process_generation_id',attempt.process_generation_id,
					'state',attempt.state,
					'revision',attempt.revision,
					'terminal_evidence_id',attempt.terminal_evidence_id,
					'unknown_reason',attempt.unknown_reason
				)
				ORDER BY attempt.managed_execution_id,attempt.attempt_id
			)
			FROM decodex.provider_attempts AS attempt
			WHERE attempt.consumer_kind='managed_run_execution'
				AND attempt.managed_run_id=run.managed_run_id
		),'[]'::jsonb)
	)
	FROM decodex.managed_runs AS run
	JOIN decodex.runtime_sessions AS session
		ON session.runtime_session_id=run.runtime_session_id
		AND session.revision=run.runtime_session_revision
	WHERE run.managed_run_id=p_managed_run_id
		AND run.project_id=p_project_id
		AND run.revision=p_expected_revision
$$;

CREATE OR REPLACE FUNCTION decodex.enforce_provider_attempt_binding()
RETURNS trigger LANGUAGE plpgsql
SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1
		FROM decodex.continuation_plans AS plan
		JOIN decodex.routing_decisions AS decision
			ON decision.decision_id=plan.routing_decision_id
		JOIN decodex.runtime_sessions AS session
			ON (
				session.runtime_session_id,
				session.revision
			)=(
				NEW.accepted_runtime_session_id,
				NEW.accepted_runtime_session_revision
			)
		JOIN decodex.account_snapshots AS account
			ON account.account_snapshot_id=session.account_snapshot_id
		JOIN decodex.process_generations AS generation
			ON generation.generation_id=NEW.process_generation_id
		JOIN decodex.process_generation_transitions AS generation_transition
			ON (
				generation_transition.generation_id,
				generation_transition.revision
			)=(
				NEW.process_generation_id,
				NEW.process_generation_revision
			)
		JOIN decodex.process_generation_execution_epochs AS epoch
			ON epoch.execution_epoch_id=generation.execution_epoch_id
		WHERE plan.plan_id=NEW.continuation_plan_id
			AND plan.routing_decision_id=NEW.routing_decision_id
			AND decision.kind='selected'
			AND decision.selected_account_id=NEW.selected_account_id
			AND plan.selected_account_id=NEW.selected_account_id
			AND NOT plan.dispatch_enabled
			AND NOT plan.replay_permitted
			AND (
				plan.consumer_kind,
				plan.consumer_conversation_id,
				plan.turn_id,
				plan.managed_run_id,
				plan.managed_run_revision,
				plan.managed_execution_id
			) IS NOT DISTINCT FROM (
				NEW.consumer_kind,
				NEW.conversation_id,
				NEW.turn_id,
				NEW.managed_run_id,
				NEW.managed_run_revision,
				NEW.managed_execution_id
			)
			AND (
				decision.consumer_kind,
				decision.conversation_id,
				decision.turn_id,
				decision.managed_run_id,
				decision.managed_run_revision,
				decision.managed_execution_id
			) IS NOT DISTINCT FROM (
				NEW.consumer_kind,
				NEW.conversation_id,
				NEW.turn_id,
				NEW.managed_run_id,
				NEW.managed_run_revision,
				NEW.managed_execution_id
			)
			AND (
				(
					plan.kind='same_thread'
					AND NEW.accepted_runtime_session_id=
						plan.source_runtime_session_id
					AND NEW.accepted_runtime_session_revision=
						plan.source_runtime_session_revision
					AND session.state='active'
				) OR (
					plan.kind='context_pack_fallback'
					AND NEW.accepted_runtime_session_id=
						plan.fallback_runtime_session_id
					AND NEW.accepted_runtime_session_revision=1
					AND session.state='starting'
				)
			)
			AND account.source_account_id=NEW.selected_account_id
			AND generation.account_id=NEW.selected_account_id
			AND generation.execution_epoch_id=
				NEW.process_execution_epoch_id
			AND generation.revision=NEW.process_generation_revision
			AND generation.state='ready'
			AND generation.process_id IS NOT NULL
			AND generation_transition.state='ready'
			AND epoch.retired_at IS NULL
	) THEN
		RAISE EXCEPTION 'provider attempt has forged V16, V17, or ProcessGeneration lineage'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_authority_complete';
	END IF;

	IF NEW.consumer_kind='conversation_turn' THEN
		IF NOT EXISTS (
			SELECT 1
			FROM decodex.conversations AS conversation
			JOIN decodex.continuation_plans AS plan
				ON plan.plan_id=NEW.continuation_plan_id
			WHERE conversation.conversation_id=NEW.conversation_id
				AND conversation.revision=plan.conversation_revision
				AND conversation.status='open'
				AND plan.consumer_conversation_id=NEW.conversation_id
				AND plan.turn_id=NEW.turn_id
		) OR EXISTS (
			SELECT 1
			FROM decodex.turns AS turn
			WHERE turn.turn_id=NEW.turn_id
				AND (
					turn.conversation_id<>NEW.conversation_id
					OR turn.runtime_session_id<>
						NEW.accepted_runtime_session_id
					OR turn.status<>'active'
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
				ON plan.plan_id=NEW.continuation_plan_id
			WHERE (run.managed_run_id,run.revision)=
					(NEW.managed_run_id,NEW.managed_run_revision)
				AND (
					plan.managed_run_id,
					plan.managed_run_revision,
					plan.managed_execution_id
				)=(
					NEW.managed_run_id,
					NEW.managed_run_revision,
					NEW.managed_execution_id
				)
		) THEN
			RAISE EXCEPTION 'provider attempt ManagedRun execution binding is incomplete'
				USING ERRCODE='23514',
					CONSTRAINT='provider_attempt_consumer_complete';
		END IF;
	END IF;

	-- An unknown attempt blocks only its exact immutable intent. The unique Turn and
	-- ManagedRun-execution indexes reject replay of that intent. Other intents remain eligible.
	IF NEW.predecessor_attempt_id IS NOT NULL
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.provider_attempts AS predecessor
			WHERE predecessor.attempt_id=NEW.predecessor_attempt_id
				AND predecessor.state='unknown'
				AND predecessor.consumer_kind=NEW.consumer_kind
				AND predecessor.request_id<>NEW.request_id
				AND (
					(
						NEW.consumer_kind='conversation_turn'
						AND predecessor.conversation_id=
							NEW.conversation_id
						AND predecessor.turn_id<>NEW.turn_id
					) OR (
						NEW.consumer_kind='managed_run_execution'
						AND predecessor.managed_run_id=
							NEW.managed_run_id
						AND predecessor.managed_execution_id<>
							NEW.managed_execution_id
					)
				)
		)
	THEN
		RAISE EXCEPTION 'duplicate-risk acknowledgement does not bind one exact unknown attempt'
			USING ERRCODE='23514',
				CONSTRAINT='provider_attempt_duplicate_risk_ack_invalid';
	END IF;
	RETURN NULL;
END
$$;

CREATE OR REPLACE FUNCTION decodex.enforce_continuation_plan_completeness()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
BEGIN
	IF NOT EXISTS (
		SELECT 1
		FROM decodex.routing_decisions AS decision
		JOIN decodex.routing_snapshots AS snapshot
			ON snapshot.snapshot_id=decision.snapshot_id
		JOIN decodex.runtime_sessions AS session
			ON (session.runtime_session_id,session.revision)=
				(snapshot.runtime_session_id,snapshot.runtime_session_revision)
		WHERE decision.decision_id=NEW.routing_decision_id
			AND decision.kind='selected'
			AND decision.selected_account_id=NEW.selected_account_id
			AND NOT EXISTS (
				SELECT 1
				FROM decodex.routing_decision_blocker_refs AS blocker
				WHERE blocker.decision_id=decision.decision_id
					AND blocker.account_id=decision.selected_account_id
			)
			AND (
				decision.consumer_kind,
				decision.conversation_id,
				decision.conversation_revision,
				decision.turn_id,
				decision.managed_run_id,
				decision.managed_run_revision,
				decision.managed_execution_id
			) IS NOT DISTINCT FROM (
				NEW.consumer_kind,
				NEW.consumer_conversation_id,
				NEW.conversation_revision,
				NEW.turn_id,
				NEW.managed_run_id,
				NEW.managed_run_revision,
				NEW.managed_execution_id
			)
			AND (
				snapshot.consumer_kind,
				snapshot.conversation_id,
				snapshot.conversation_revision,
				snapshot.turn_id,
				snapshot.managed_run_id,
				snapshot.managed_run_revision,
				snapshot.managed_execution_id
			) IS NOT DISTINCT FROM (
				NEW.consumer_kind,
				NEW.consumer_conversation_id,
				NEW.conversation_revision,
				NEW.turn_id,
				NEW.managed_run_id,
				NEW.managed_run_revision,
				NEW.managed_execution_id
			)
			AND (snapshot.runtime_session_id,snapshot.runtime_session_revision)=
				(NEW.source_runtime_session_id,NEW.source_runtime_session_revision)
			AND session.conversation_id=NEW.conversation_id
			AND (
				(
					NEW.consumer_kind::text='conversation_turn'
					AND NEW.consumer_conversation_id=NEW.conversation_id
					AND EXISTS (
						SELECT 1 FROM decodex.conversations AS conversation
						WHERE conversation.conversation_id=
								NEW.consumer_conversation_id
							AND conversation.revision=NEW.conversation_revision
							AND conversation.status='open'
					)
				) OR (
					NEW.consumer_kind::text='managed_run_execution'
					AND EXISTS (
						SELECT 1 FROM decodex.managed_runs AS run
						WHERE (run.managed_run_id,run.revision)=
							(NEW.managed_run_id,NEW.managed_run_revision)
					)
				)
			)
	) THEN
		RAISE EXCEPTION 'continuation plan has forged V16 lineage'
			USING ERRCODE='23514',CONSTRAINT='continuation_plan_complete';
	END IF;

	IF NEW.kind='same_thread'
		AND NEW.consumer_kind::text='managed_run_execution'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.routing_decisions AS decision
			JOIN decodex.routing_snapshots AS snapshot
				ON snapshot.snapshot_id=decision.snapshot_id
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=snapshot.snapshot_id
				AND member.account_id=decision.selected_account_id
			JOIN decodex.managed_runs AS run
				ON run.managed_run_id=NEW.managed_run_id
			JOIN decodex.runtime_sessions AS source_session
				ON source_session.runtime_session_id=
					NEW.source_runtime_session_id
			JOIN decodex.routing_compatibility_evidence AS evidence
				ON evidence.evidence_id=member.evidence_id
			JOIN decodex.codex_experiments AS experiment
				ON experiment.experiment_id=NEW.codex_experiment_id
			JOIN decodex.codex_experiment_thread_bindings AS binding
				ON binding.experiment_id=experiment.experiment_id
			JOIN decodex.codex_experiment_observations AS observation
				ON observation.observation_id=NEW.codex_observation_id
			WHERE decision.decision_id=NEW.routing_decision_id
				AND (
					evidence.evidence_id,
					evidence.evidence_revision,
					evidence.schema_fingerprint
				)=(
					NEW.routing_evidence_id,
					NEW.routing_evidence_revision,
					NEW.schema_fingerprint
				)
				AND (
					experiment.managed_run_id,
					experiment.managed_run_revision,
					experiment.routing_snapshot_id,
					experiment.account_id,
					experiment.account_revision,
					experiment.role_profile_revision,
					experiment.build_id,
					experiment.revision,
					experiment.state
				)=(
					NEW.managed_run_id,
					NEW.managed_run_revision,
					snapshot.snapshot_id,
					decision.selected_account_id,
					member.account_revision,
					snapshot.required_role_profile_revision,
					snapshot.required_build_id,
					3,
					'thread_bound'
				)
				AND binding.thread_id=NEW.codex_thread_id::text
				AND observation.experiment_id=experiment.experiment_id
				AND observation.experiment_revision=3
				AND observation.thread_id=binding.thread_id
				AND observation.kind='thread_read_item'
				AND evidence.account_id=NEW.selected_account_id
				AND evidence.account_revision=member.account_revision
				AND evidence.role=snapshot.required_role
				AND evidence.role_profile_revision=
					snapshot.required_role_profile_revision
				AND evidence.build_id=snapshot.required_build_id
				AND evidence.process_account_id=NEW.selected_account_id
				AND member.disposition='included'
				AND source_session.state='active'
				AND NOT run.diverged
				AND evidence.ingested_at<=NEW.planned_at
				AND NEW.planned_at-evidence.ingested_at<=INTERVAL '300 seconds'
				AND experiment.updated_at<=NEW.planned_at
				AND NEW.planned_at-experiment.updated_at<=INTERVAL '300 seconds'
				AND observation.observed_at<=NEW.planned_at
				AND NEW.planned_at-observation.observed_at<=
					INTERVAL '300 seconds'
				AND 4=(
					SELECT pg_catalog.count(DISTINCT capability.capability)
					FROM decodex.routing_capability_evidence AS capability
					WHERE capability.evidence_id=evidence.evidence_id
						AND capability.capability IN (
							'initialize','account_read',
							'thread_read','paginated_history'
						)
						AND capability.state='supported'
				)
		) THEN
		RAISE EXCEPTION 'same-thread ManagedRun plan has forged positive evidence'
			USING ERRCODE='23514',CONSTRAINT='continuation_plan_complete';
	ELSIF NEW.kind='same_thread'
		AND NEW.consumer_kind::text='conversation_turn'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.provider_attempts AS attempt
			JOIN decodex.provider_attempt_positive_evidence AS evidence
				ON evidence.evidence_id=attempt.terminal_evidence_id
				AND evidence.attempt_id=attempt.attempt_id
			JOIN decodex.runtime_sessions AS source_session
				ON source_session.runtime_session_id=
					NEW.source_runtime_session_id
			WHERE attempt.attempt_id=NEW.same_thread_provider_attempt_id
				AND attempt.revision=
					NEW.same_thread_provider_attempt_revision
				AND evidence.evidence_id=
					NEW.same_thread_provider_evidence_id
				AND attempt.consumer_kind::text='conversation_turn'
				AND attempt.conversation_id=
					NEW.consumer_conversation_id
				AND attempt.turn_id<>NEW.turn_id
				AND attempt.accepted_runtime_session_id=
					NEW.source_runtime_session_id
				AND attempt.accepted_runtime_session_revision=
					NEW.source_runtime_session_revision
				AND attempt.selected_account_id=NEW.selected_account_id
				AND attempt.state IN ('succeeded','failed_definitive')
				AND evidence.outcome::text=attempt.state::text
				AND evidence.source='exact_thread_readback'
				AND evidence.provider_thread_id=NEW.codex_thread_id::text
				AND source_session.codex_thread_id=NEW.codex_thread_id
				AND source_session.state='active'
				AND evidence.observed_at<=NEW.planned_at
				AND NEW.planned_at-evidence.observed_at<=
					INTERVAL '300 seconds'
		) THEN
		RAISE EXCEPTION 'same-thread Conversation plan has forged positive evidence'
			USING ERRCODE='23514',CONSTRAINT='continuation_plan_complete';
	ELSIF NEW.kind='context_pack_fallback'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.context_packs AS pack
			JOIN decodex.routing_decisions AS decision
				ON decision.decision_id=NEW.routing_decision_id
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=decision.snapshot_id
				AND member.account_id=NEW.selected_account_id
			JOIN decodex.runtime_sessions AS source_session
				ON source_session.runtime_session_id=
					NEW.source_runtime_session_id
			JOIN decodex.runtime_sessions AS session
				ON session.runtime_session_id=
					NEW.fallback_runtime_session_id
			JOIN decodex.account_snapshots AS account
				ON account.account_snapshot_id=session.account_snapshot_id
			WHERE pack.context_pack_id=NEW.fallback_context_pack_id
				AND pack.conversation_id=NEW.conversation_id
				AND session.conversation_id=NEW.conversation_id
				AND session.codex_thread_id IS NULL
				AND session.state='starting'
				AND session.revision=1
				AND account.source_account_id=NEW.selected_account_id
				AND (
					account.source_revision,
					account.display_label,
					account.observed_state
				)=(
					member.account_revision,
					member.display_label,
					member.account_state
				)
				AND session.profile_snapshot_id=
					source_session.profile_snapshot_id
		) THEN
		RAISE EXCEPTION 'fallback plan has incomplete Context Pack or RuntimeSession linkage'
			USING ERRCODE='23514',CONSTRAINT='continuation_plan_complete';
	END IF;
	RETURN NULL;
END
$$;

-- Remove the V12 submitted-turn/effect writer surface before installing the new composition.
DROP FUNCTION decodex.apply_managed_run_safety_input_exact(
	text,text,uuid,uuid,bigint,decodex.managed_run_safety_input_kind,uuid,uuid,uuid
);
DROP FUNCTION decodex.complete_exact_managed_run_safety_rejection(text,text,text,jsonb);
DROP FUNCTION decodex.reserve_exact_managed_run_safety_command(text,text,jsonb);
DROP TABLE decodex.managed_run_safety_inputs;
DROP TABLE decodex.managed_run_submitted_turn_receipts;
DROP TABLE decodex.managed_run_effects;
DROP TABLE decodex.managed_run_effect_barriers;
DROP FUNCTION decodex.enforce_effect_barrier_state();

-- V14 snapshot lineage becomes a closed ordinary-Conversation or ManagedRun union.
ALTER TABLE decodex.routing_snapshots
	ADD COLUMN consumer_kind decodex.provider_attempt_consumer_kind,
	ADD COLUMN conversation_id uuid,
	ADD COLUMN conversation_revision bigint,
	ADD COLUMN turn_id uuid,
	ADD COLUMN managed_execution_id uuid;

ALTER TABLE decodex.routing_snapshots
	DISABLE TRIGGER routing_snapshots_immutable;

UPDATE decodex.routing_snapshots SET
	consumer_kind='managed_run_execution',
	managed_execution_id=pg_catalog.gen_random_uuid();

-- Preserve an already accepted V24 ManagedRun execution intent. A synthetic identity is safe only
-- for a historical V16/V17 lineage that has no ProviderAttempt.
UPDATE decodex.routing_snapshots AS snapshot SET
	managed_execution_id=accepted.managed_execution_id
FROM (
	SELECT decision.snapshot_id,
		pg_catalog.min(attempt.managed_execution_id::text)::uuid
			AS managed_execution_id
	FROM decodex.routing_decisions AS decision
	JOIN decodex.continuation_plans AS plan
		ON plan.routing_decision_id=decision.decision_id
	JOIN decodex.provider_attempts AS attempt
		ON attempt.continuation_plan_id=plan.plan_id
	WHERE attempt.consumer_kind::text='managed_run_execution'
	GROUP BY decision.snapshot_id
) AS accepted
WHERE accepted.snapshot_id=snapshot.snapshot_id;

ALTER TABLE decodex.routing_snapshots
	ENABLE TRIGGER routing_snapshots_immutable;

ALTER TABLE decodex.routing_snapshots
	ALTER COLUMN consumer_kind SET NOT NULL,
	ALTER COLUMN managed_run_id DROP NOT NULL,
	ALTER COLUMN managed_run_revision DROP NOT NULL,
	ADD CONSTRAINT routing_snapshots_conversation_fk FOREIGN KEY (conversation_id)
		REFERENCES decodex.conversations(conversation_id),
	ADD CONSTRAINT routing_snapshots_consumer_shape CHECK (
		(
			consumer_kind::text='conversation_turn'
			AND conversation_id IS NOT NULL
			AND conversation_revision > 0
			AND turn_id IS NOT NULL
			AND managed_run_id IS NULL
			AND managed_run_revision IS NULL
			AND managed_execution_id IS NULL
		) OR (
			consumer_kind::text='managed_run_execution'
			AND conversation_id IS NULL
			AND conversation_revision IS NULL
			AND turn_id IS NULL
			AND managed_run_id IS NOT NULL
			AND managed_run_revision > 0
			AND managed_execution_id IS NOT NULL
		)
	);

-- V16 preserves the same closed consumer and its exact intent.
ALTER TABLE decodex.routing_decisions
	DROP CONSTRAINT routing_decisions_shape,
	ADD COLUMN consumer_kind decodex.provider_attempt_consumer_kind,
	ADD COLUMN conversation_id uuid,
	ADD COLUMN conversation_revision bigint,
	ADD COLUMN turn_id uuid,
	ADD COLUMN managed_execution_id uuid;

ALTER TABLE decodex.routing_decisions
	DISABLE TRIGGER routing_decisions_immutable;

UPDATE decodex.routing_decisions AS decision SET
	consumer_kind=snapshot.consumer_kind,
	conversation_id=snapshot.conversation_id,
	conversation_revision=snapshot.conversation_revision,
	turn_id=snapshot.turn_id,
	managed_execution_id=snapshot.managed_execution_id
FROM decodex.routing_snapshots AS snapshot
WHERE snapshot.snapshot_id=decision.snapshot_id;

ALTER TABLE decodex.routing_decisions
	ENABLE TRIGGER routing_decisions_immutable;

ALTER TABLE decodex.routing_decisions
	ALTER COLUMN consumer_kind SET NOT NULL,
	ALTER COLUMN managed_run_id DROP NOT NULL,
	ALTER COLUMN managed_run_revision DROP NOT NULL,
	ADD CONSTRAINT routing_decisions_snapshot_fk FOREIGN KEY (snapshot_id)
		REFERENCES decodex.routing_snapshots(snapshot_id),
	ADD CONSTRAINT routing_decisions_shape CHECK (
		(
			kind::text='selected'
			AND selected_account_id IS NOT NULL
			AND waiting_ready_at_micros IS NULL
			AND no_route_reason IS NULL
		) OR (
			kind::text='waiting_usage'
			AND selected_account_id IS NULL
			AND waiting_ready_at_micros BETWEEN 0 AND 253402300799999999
			AND no_route_reason IS NULL
		) OR (
			kind::text='waiting_reconciliation'
			AND selected_account_id IS NULL
			AND waiting_ready_at_micros IS NULL
			AND no_route_reason IS NULL
		) OR (
			kind::text='no_route'
			AND selected_account_id IS NULL
			AND waiting_ready_at_micros IS NULL
			AND no_route_reason::text='blocked_evidence'
		)
	),
	ADD CONSTRAINT routing_decisions_consumer_shape CHECK (
		(
			consumer_kind::text='conversation_turn'
			AND conversation_id IS NOT NULL
			AND conversation_revision > 0
			AND turn_id IS NOT NULL
			AND managed_run_id IS NULL
			AND managed_run_revision IS NULL
			AND managed_execution_id IS NULL
		) OR (
			consumer_kind::text='managed_run_execution'
			AND conversation_id IS NULL
			AND conversation_revision IS NULL
			AND turn_id IS NULL
			AND managed_run_id IS NOT NULL
			AND managed_run_revision > 0
			AND managed_execution_id IS NOT NULL
		)
	);

-- Decision blocker references now retain the complete V16 blocker projection, including
-- read-only ProcessGeneration, ProviderAttempt, and ManagedRun-domain causes.
ALTER TABLE decodex.routing_decision_blocker_refs
	DROP CONSTRAINT routing_decision_blocker_snapshot_fk;

-- V17 preserves consumer identity but no longer retains any V12 barrier or receipt fact.
ALTER TABLE decodex.continuation_plans
	DROP CONSTRAINT continuation_plans_run_revision_authority_fk,
	DROP CONSTRAINT continuation_plans_shape,
	DROP COLUMN effect_barrier_state,
	DROP COLUMN effect_barrier_revision,
	DROP COLUMN submitted_turn_receipt_count,
	ADD COLUMN consumer_kind decodex.provider_attempt_consumer_kind,
	ADD COLUMN consumer_conversation_id uuid,
	ADD COLUMN conversation_revision bigint,
	ADD COLUMN turn_id uuid,
	ADD COLUMN managed_execution_id uuid,
	ADD COLUMN same_thread_provider_attempt_id uuid,
	ADD COLUMN same_thread_provider_attempt_revision bigint,
	ADD COLUMN same_thread_provider_evidence_id uuid;

ALTER TABLE decodex.continuation_plans
	DISABLE TRIGGER continuation_plans_command_owner;

UPDATE decodex.continuation_plans AS plan SET
	consumer_kind=decision.consumer_kind,
	consumer_conversation_id=decision.conversation_id,
	conversation_revision=decision.conversation_revision,
	turn_id=decision.turn_id,
	managed_execution_id=decision.managed_execution_id
FROM decodex.routing_decisions AS decision
WHERE decision.decision_id=plan.routing_decision_id;

ALTER TABLE decodex.continuation_plans
	ENABLE TRIGGER continuation_plans_command_owner;

ALTER TABLE decodex.continuation_plans
	ALTER COLUMN consumer_kind SET NOT NULL,
	ALTER COLUMN managed_run_id DROP NOT NULL,
	ALTER COLUMN managed_run_revision DROP NOT NULL,
	ADD CONSTRAINT continuation_plans_decision_fk FOREIGN KEY (routing_decision_id)
		REFERENCES decodex.routing_decisions(decision_id),
	ADD CONSTRAINT continuation_plans_consumer_conversation_fk
		FOREIGN KEY (consumer_conversation_id)
		REFERENCES decodex.conversations(conversation_id),
	ADD CONSTRAINT continuation_plans_same_thread_attempt_fk
		FOREIGN KEY (same_thread_provider_attempt_id)
		REFERENCES decodex.provider_attempts(attempt_id),
	ADD CONSTRAINT continuation_plans_same_thread_evidence_fk
		FOREIGN KEY (same_thread_provider_evidence_id, same_thread_provider_attempt_id)
		REFERENCES decodex.provider_attempt_positive_evidence(evidence_id, attempt_id),
	ADD CONSTRAINT continuation_plans_consumer_shape CHECK (
		(
			consumer_kind::text='conversation_turn'
			AND consumer_conversation_id IS NOT NULL
			AND conversation_revision > 0
			AND turn_id IS NOT NULL
			AND managed_run_id IS NULL
			AND managed_run_revision IS NULL
			AND managed_execution_id IS NULL
		) OR (
			consumer_kind::text='managed_run_execution'
			AND consumer_conversation_id IS NULL
			AND conversation_revision IS NULL
			AND turn_id IS NULL
			AND managed_run_id IS NOT NULL
			AND managed_run_revision > 0
			AND managed_execution_id IS NOT NULL
		)
	),
	ADD CONSTRAINT continuation_plans_shape CHECK (
		(
			kind='same_thread'
			AND codex_thread_id IS NOT NULL
			AND fallback_context_pack_id IS NULL
			AND fallback_runtime_session_id IS NULL
			AND (
				(
					consumer_kind::text='managed_run_execution'
					AND routing_evidence_id IS NOT NULL
					AND routing_evidence_revision > 0
					AND schema_fingerprint COLLATE pg_catalog."C" ~ '^[0-9a-f]{64}$'
					AND codex_experiment_id IS NOT NULL
					AND codex_experiment_revision=3
					AND codex_observation_id IS NOT NULL
					AND same_thread_provider_attempt_id IS NULL
					AND same_thread_provider_attempt_revision IS NULL
					AND same_thread_provider_evidence_id IS NULL
				) OR (
					consumer_kind::text='conversation_turn'
					AND routing_evidence_id IS NULL
					AND routing_evidence_revision IS NULL
					AND schema_fingerprint IS NULL
					AND codex_experiment_id IS NULL
					AND codex_experiment_revision IS NULL
					AND codex_observation_id IS NULL
					AND same_thread_provider_attempt_id IS NOT NULL
					AND same_thread_provider_attempt_revision > 0
					AND same_thread_provider_evidence_id IS NOT NULL
				)
			)
		) OR (
			kind='context_pack_fallback'
			AND codex_thread_id IS NULL
			AND fallback_context_pack_id IS NOT NULL
			AND fallback_runtime_session_id IS NOT NULL
			AND routing_evidence_id IS NULL
			AND routing_evidence_revision IS NULL
			AND schema_fingerprint IS NULL
			AND codex_experiment_id IS NULL
			AND codex_experiment_revision IS NULL
			AND codex_observation_id IS NULL
			AND same_thread_provider_attempt_id IS NULL
			AND same_thread_provider_attempt_revision IS NULL
			AND same_thread_provider_evidence_id IS NULL
		)
	);

DROP TYPE decodex.managed_run_safety_input_kind;
DROP TYPE decodex.managed_run_effect_state;
DROP TYPE decodex.managed_run_effect_kind;
DROP TYPE decodex.effect_barrier_state;

DROP FUNCTION decodex.resolve_routing_snapshot_exact(text,text,uuid,bigint,uuid,bigint);
DROP FUNCTION decodex.route_account_exact(text,text,uuid,uuid,bigint,uuid,bigint);
DROP FUNCTION decodex.plan_continuation_exact(
	text,text,uuid,uuid,bigint,uuid,uuid,uuid,uuid,bytea,text,text,integer,integer,text,
	boolean,integer,text[],text[],bigint[],text[],bigint[],bigint[],text[],text[],text[],bigint[]
);

CREATE OR REPLACE FUNCTION decodex.enforce_routing_completeness()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
DECLARE member_count bigint; capability_count bigint; account_count bigint;
DECLARE quota_count bigint; matrix_count bigint; blocker_count bigint; blocker_array_count bigint;
DECLARE lineage_complete boolean;
BEGIN
	IF TG_TABLE_NAME='routing_policy_revisions' THEN
		SELECT pg_catalog.count(*) INTO member_count
		FROM decodex.routing_policy_members
		WHERE routing_policy_id=NEW.routing_policy_id
			AND routing_policy_revision=NEW.revision;
		SELECT pg_catalog.count(*) INTO account_count FROM decodex.accounts;
		IF member_count<>account_count OR EXISTS (
			SELECT 1 FROM decodex.routing_policy_members
			WHERE routing_policy_id=NEW.routing_policy_id
				AND routing_policy_revision=NEW.revision
			GROUP BY routing_policy_id,routing_policy_revision
			HAVING pg_catalog.min(position)<>1
				OR pg_catalog.max(position)<>pg_catalog.count(*)
		) OR EXISTS (
			SELECT account_id FROM decodex.accounts
			EXCEPT
			SELECT account_id FROM decodex.routing_policy_members
			WHERE routing_policy_id=NEW.routing_policy_id
				AND routing_policy_revision=NEW.revision
		) THEN
			RAISE EXCEPTION 'routing policy revision is not a complete account inventory'
				USING ERRCODE='23514',
					CONSTRAINT='routing_policy_complete_inventory';
		END IF;
		IF EXISTS (
			SELECT 1 FROM decodex.routing_policy_required_capabilities
			WHERE routing_policy_id=NEW.routing_policy_id
				AND routing_policy_revision=NEW.revision
			GROUP BY routing_policy_id,routing_policy_revision
			HAVING pg_catalog.min(position)<>1
				OR pg_catalog.max(position)<>pg_catalog.count(*)
				OR pg_catalog.array_agg(capability ORDER BY position)
					<>pg_catalog.array_agg(capability ORDER BY capability)
		) THEN
			RAISE EXCEPTION 'required capability positions are not contiguous'
				USING ERRCODE='23514',
					CONSTRAINT='routing_policy_capabilities_contiguous';
		END IF;
		RETURN NULL;
	END IF;

	IF TG_TABLE_NAME='routing_compatibility_evidence' THEN
		SELECT pg_catalog.count(*) INTO capability_count
		FROM decodex.routing_capability_evidence
		WHERE evidence_id=NEW.evidence_id;
		IF capability_count<>8 OR EXISTS (
			SELECT 1 FROM decodex.routing_capability_evidence
			WHERE evidence_id=NEW.evidence_id
			GROUP BY evidence_id
			HAVING pg_catalog.min(position)<>1
				OR pg_catalog.max(position)<>8
				OR pg_catalog.count(*)<>8
		) THEN
			RAISE EXCEPTION 'compatibility evidence lacks the closed capability projection'
				USING ERRCODE='23514',
					CONSTRAINT='routing_evidence_complete_capabilities';
		END IF;
		RETURN NULL;
	END IF;

	SELECT pg_catalog.count(*) INTO member_count
	FROM decodex.routing_snapshot_members WHERE snapshot_id=NEW.snapshot_id;
	SELECT pg_catalog.count(*) INTO account_count
	FROM decodex.routing_policy_members
	WHERE routing_policy_id=NEW.routing_policy_id
		AND routing_policy_revision=NEW.routing_policy_revision;
	SELECT pg_catalog.count(*) INTO quota_count
	FROM decodex.routing_snapshot_quota_facts WHERE snapshot_id=NEW.snapshot_id;
	SELECT pg_catalog.count(*) INTO matrix_count
	FROM decodex.routing_snapshot_capability_facts WHERE snapshot_id=NEW.snapshot_id;
	SELECT pg_catalog.count(*) INTO blocker_count
	FROM decodex.routing_snapshot_blockers WHERE snapshot_id=NEW.snapshot_id;
	SELECT COALESCE(pg_catalog.sum(pg_catalog.cardinality(blockers)),0)
	INTO blocker_array_count
	FROM decodex.routing_snapshot_members WHERE snapshot_id=NEW.snapshot_id;

	IF NEW.consumer_kind::text='conversation_turn' THEN
		SELECT EXISTS (
			SELECT 1
			FROM decodex.conversations AS conversation
			JOIN decodex.runtime_sessions AS session
				ON (session.runtime_session_id,session.revision)=
					(NEW.runtime_session_id,NEW.runtime_session_revision)
			JOIN decodex.account_snapshots AS account
				ON account.account_snapshot_id=NEW.account_snapshot_id
			JOIN decodex.profile_snapshots AS profile
				ON profile.profile_snapshot_id=NEW.profile_snapshot_id
			JOIN decodex.routing_snapshot_members AS sticky
				ON sticky.snapshot_id=NEW.snapshot_id AND sticky.sticky
			WHERE conversation.conversation_id=NEW.conversation_id
				AND conversation.revision=NEW.conversation_revision
				AND conversation.status='open'
				AND session.conversation_id=conversation.conversation_id
				AND (session.account_snapshot_id,session.profile_snapshot_id)=
					(account.account_snapshot_id,profile.profile_snapshot_id)
				AND (account.source_account_id,account.source_revision)=
					(sticky.account_id,NEW.account_snapshot_source_revision)
				AND (profile.role,profile.source_revision)=
					(NEW.required_role,NEW.profile_snapshot_source_revision)
				AND NEW.required_role_profile_revision=profile.source_revision
		) INTO lineage_complete;
	ELSE
		SELECT EXISTS (
			SELECT 1
			FROM decodex.managed_runs AS run
			JOIN decodex.runtime_sessions AS session
				ON (session.runtime_session_id,session.revision)=
					(NEW.runtime_session_id,NEW.runtime_session_revision)
			JOIN decodex.account_snapshots AS account
				ON account.account_snapshot_id=NEW.account_snapshot_id
			JOIN decodex.profile_snapshots AS profile
				ON profile.profile_snapshot_id=NEW.profile_snapshot_id
			JOIN decodex.routing_policy_revisions AS policy
				ON policy.routing_policy_id=NEW.routing_policy_id
				AND policy.revision=NEW.routing_policy_revision
			JOIN decodex.routing_snapshot_members AS sticky
				ON sticky.snapshot_id=NEW.snapshot_id AND sticky.sticky
			WHERE run.managed_run_id=NEW.managed_run_id
				AND run.revision=NEW.managed_run_revision
				AND run.project_id=policy.project_id
				AND (run.runtime_session_id,run.runtime_session_revision)=
					(session.runtime_session_id,session.revision)
				AND (session.account_snapshot_id,session.profile_snapshot_id)=
					(account.account_snapshot_id,profile.profile_snapshot_id)
				AND (account.source_account_id,account.source_revision)=
					(sticky.account_id,NEW.account_snapshot_source_revision)
				AND (profile.role,profile.source_revision)=
					(NEW.required_role,NEW.profile_snapshot_source_revision)
				AND NEW.required_role_profile_revision=profile.source_revision
		) INTO lineage_complete;
	END IF;

	IF member_count<>account_count
		OR quota_count<>member_count*2
		OR matrix_count<>member_count*8
		OR blocker_count<>blocker_array_count
		OR NOT lineage_complete
		OR (
			SELECT pg_catalog.count(*) FROM decodex.routing_snapshot_members
			WHERE snapshot_id=NEW.snapshot_id AND sticky
		)<>1
		OR EXISTS (
			SELECT 1 FROM decodex.routing_snapshot_members
			WHERE snapshot_id=NEW.snapshot_id
			GROUP BY snapshot_id
			HAVING pg_catalog.min(position)<>1
				OR pg_catalog.max(position)<>pg_catalog.count(*)
		)
		OR EXISTS (
			SELECT member.position,member.account_id,member.disposition
			FROM decodex.routing_snapshot_members AS member
			WHERE member.snapshot_id=NEW.snapshot_id
			EXCEPT
			SELECT policy.position,policy.account_id,policy.disposition
			FROM decodex.routing_policy_members AS policy
			WHERE policy.routing_policy_id=NEW.routing_policy_id
				AND policy.routing_policy_revision=NEW.routing_policy_revision
		)
		OR EXISTS (
			SELECT policy.position,policy.account_id,policy.disposition
			FROM decodex.routing_policy_members AS policy
			WHERE policy.routing_policy_id=NEW.routing_policy_id
				AND policy.routing_policy_revision=NEW.routing_policy_revision
			EXCEPT
			SELECT member.position,member.account_id,member.disposition
			FROM decodex.routing_snapshot_members AS member
			WHERE member.snapshot_id=NEW.snapshot_id
		)
		OR EXISTS (
			SELECT 1 FROM decodex.routing_snapshot_members AS member
			WHERE member.snapshot_id=NEW.snapshot_id
				AND pg_catalog.cardinality(member.blockers)
					<>(
						SELECT pg_catalog.count(*)
						FROM decodex.routing_snapshot_blockers AS blocker
						WHERE blocker.snapshot_id=member.snapshot_id
							AND blocker.account_id=member.account_id
					)
		)
	THEN
		RAISE EXCEPTION 'routing snapshot is incomplete or cross-linked'
			USING ERRCODE='23514',CONSTRAINT='routing_snapshot_complete';
	END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.resolve_routing_snapshot_exact(
	p_protocol text,
	p_idempotency_key text,
	p_routing_policy_id uuid,
	p_expected_routing_policy_revision bigint,
	p_consumer_kind decodex.provider_attempt_consumer_kind,
	p_conversation_id uuid,
	p_expected_conversation_revision bigint,
	p_source_runtime_session_id uuid,
	p_expected_source_runtime_session_revision bigint,
	p_turn_id uuid,
	p_managed_run_id uuid,
	p_expected_managed_run_revision bigint,
	p_managed_execution_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; policy_row record; run_row record;
DECLARE conversation_row record; session_row record; resolved timestamptz;
DECLARE new_snapshot_id uuid; member record; evidence record; quota record;
DECLARE blockers decodex.routing_blocker[]; sticky_account uuid;
DECLARE core jsonb; effect jsonb; response bytea;
DECLARE source_session_id uuid; source_session_revision bigint;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','resolve_routing_snapshot',
		'protocol',p_protocol,
		'routing_policy_id',p_routing_policy_id,
		'expected_routing_policy_revision',p_expected_routing_policy_revision,
		'consumer_kind',p_consumer_kind,
		'conversation_id',p_conversation_id,
		'conversation_revision',p_expected_conversation_revision,
		'source_runtime_session_id',p_source_runtime_session_id,
		'source_runtime_session_revision',p_expected_source_runtime_session_revision,
		'turn_id',p_turn_id,
		'managed_run_id',p_managed_run_id,
		'managed_run_revision',p_expected_managed_run_revision,
		'managed_execution_id',p_managed_execution_id
	);
	replay:=decodex.reserve_exact_routing_command(
		p_protocol,p_idempotency_key,request
	);
	IF replay IS NOT NULL THEN RETURN replay; END IF;

	IF p_routing_policy_id IS NULL
		OR p_expected_routing_policy_revision IS NULL
		OR p_expected_routing_policy_revision<=0
		OR NOT COALESCE((
			(
				p_consumer_kind::text='conversation_turn'
				AND p_conversation_id IS NOT NULL
				AND p_expected_conversation_revision>0
				AND p_source_runtime_session_id IS NOT NULL
				AND p_expected_source_runtime_session_revision>0
				AND p_turn_id IS NOT NULL
				AND p_managed_run_id IS NULL
				AND p_expected_managed_run_revision IS NULL
				AND p_managed_execution_id IS NULL
			) OR (
				p_consumer_kind::text='managed_run_execution'
				AND p_conversation_id IS NULL
				AND p_expected_conversation_revision IS NULL
				AND p_source_runtime_session_id IS NULL
				AND p_expected_source_runtime_session_revision IS NULL
				AND p_turn_id IS NULL
				AND p_managed_run_id IS NOT NULL
				AND p_expected_managed_run_revision>0
				AND p_managed_execution_id IS NOT NULL
			)
		),false)
	THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot','malformed_input'
		);
	END IF;

	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1338,
		pg_catalog.hashtext(COALESCE(p_conversation_id::text,p_managed_run_id::text))
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(1356);
	LOCK TABLE decodex.accounts,decodex.quota_windows,decodex.policies,
		decodex.policy_revisions,decodex.role_profiles,decodex.role_profile_revisions,
		decodex.profile_snapshots,decodex.account_snapshots,decodex.runtime_sessions,
		decodex.conversations,decodex.routing_policy_heads,
		decodex.routing_policy_revisions,decodex.routing_policy_members,
		decodex.routing_policy_required_capabilities,
		decodex.routing_compatibility_evidence,
		decodex.routing_capability_evidence IN SHARE MODE;

	SELECT revision.project_id,revision.accepted_policy_id,
		revision.accepted_policy_revision,revision.required_role,
		revision.required_role_profile_revision,revision.required_build_id
	INTO policy_row
	FROM decodex.routing_policy_heads AS head
	JOIN decodex.routing_policy_revisions AS revision
		ON revision.routing_policy_id=head.routing_policy_id
		AND revision.revision=head.current_revision
	WHERE head.routing_policy_id=p_routing_policy_id
		AND head.current_revision=p_expected_routing_policy_revision
	FOR SHARE OF head,revision;
	IF NOT FOUND OR NOT EXISTS (
		SELECT 1 FROM decodex.policies
		WHERE policy_id=policy_row.accepted_policy_id
			AND project_id=policy_row.project_id
			AND current_revision=policy_row.accepted_policy_revision
	) OR NOT EXISTS (
		SELECT 1 FROM decodex.role_profiles
		WHERE role=policy_row.required_role
			AND current_revision=policy_row.required_role_profile_revision
	) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot',
			'routing_authority_mismatch'
		);
	END IF;

	IF p_consumer_kind::text='conversation_turn' THEN
		SELECT * INTO conversation_row
		FROM decodex.conversations
		WHERE conversation_id=p_conversation_id
			AND revision=p_expected_conversation_revision
			AND status='open'
		FOR SHARE;
		IF NOT FOUND THEN
			RETURN decodex.complete_exact_routing_rejection(
				p_protocol,p_idempotency_key,'resolve_routing_snapshot',
				'conversation_mismatch'
			);
		END IF;
		source_session_id:=p_source_runtime_session_id;
		source_session_revision:=p_expected_source_runtime_session_revision;
	ELSE
		SELECT * INTO run_row
		FROM decodex.managed_runs
		WHERE managed_run_id=p_managed_run_id
			AND revision=p_expected_managed_run_revision
		FOR SHARE;
		IF NOT FOUND OR run_row.project_id<>policy_row.project_id THEN
			RETURN decodex.complete_exact_routing_rejection(
				p_protocol,p_idempotency_key,'resolve_routing_snapshot',
				'managed_run_mismatch'
			);
		END IF;
		source_session_id:=run_row.runtime_session_id;
		source_session_revision:=run_row.runtime_session_revision;
	END IF;

	SELECT session.runtime_session_id,session.revision,session.conversation_id,
		session.account_snapshot_id,session.profile_snapshot_id,
		account.source_account_id,
		account.source_revision AS account_source_revision,
		account.display_label AS account_snapshot_display_label,
		account.observed_state AS account_snapshot_state,
		profile.role,profile.source_revision AS profile_source_revision
	INTO session_row
	FROM decodex.runtime_sessions AS session
	JOIN decodex.account_snapshots AS account USING(account_snapshot_id)
	JOIN decodex.profile_snapshots AS profile USING(profile_snapshot_id)
	WHERE session.runtime_session_id=source_session_id
		AND session.revision=source_session_revision
	FOR SHARE OF session,account,profile;
	IF NOT FOUND
		OR (
			p_consumer_kind::text='conversation_turn'
			AND session_row.conversation_id<>p_conversation_id
		)
		OR session_row.role<>policy_row.required_role
		OR session_row.profile_source_revision<>policy_row.required_role_profile_revision
		OR NOT EXISTS (
			SELECT 1 FROM decodex.accounts AS account
			WHERE account.account_id=session_row.source_account_id
				AND account.revision=session_row.account_source_revision
				AND account.display_label=session_row.account_snapshot_display_label
				AND account.state=session_row.account_snapshot_state
		)
		OR NOT EXISTS (
			SELECT 1 FROM decodex.routing_policy_members AS policy_member
			WHERE policy_member.routing_policy_id=p_routing_policy_id
				AND policy_member.routing_policy_revision=
					p_expected_routing_policy_revision
				AND policy_member.account_id=session_row.source_account_id
				AND policy_member.account_revision=session_row.account_source_revision
		)
	THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot',
			'sticky_provenance_mismatch'
		);
	END IF;

	sticky_account:=session_row.source_account_id;
	IF EXISTS (
		SELECT account_id,revision FROM decodex.accounts
		EXCEPT
		SELECT policy_member.account_id,policy_member.account_revision
		FROM decodex.routing_policy_members AS policy_member
		WHERE policy_member.routing_policy_id=p_routing_policy_id
			AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
	) OR EXISTS (
		SELECT policy_member.account_id,policy_member.account_revision
		FROM decodex.routing_policy_members AS policy_member
		WHERE policy_member.routing_policy_id=p_routing_policy_id
			AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
		EXCEPT
		SELECT account_id,revision FROM decodex.accounts
	) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'resolve_routing_snapshot',
			'routing_authority_mismatch'
		);
	END IF;

	PERFORM 1
	FROM decodex.routing_policy_members AS policy_member
	JOIN decodex.accounts AS account USING(account_id)
	WHERE policy_member.routing_policy_id=p_routing_policy_id
		AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
	ORDER BY policy_member.position
	FOR SHARE OF policy_member,account;
	PERFORM 1 FROM decodex.routing_compatibility_evidence
	WHERE account_id IN (
		SELECT account_id FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
	) ORDER BY evidence_id FOR SHARE;
	PERFORM 1 FROM decodex.routing_capability_evidence
	WHERE evidence_id IN (
		SELECT evidence_id FROM decodex.routing_compatibility_evidence
		WHERE account_id IN (
			SELECT account_id FROM decodex.routing_policy_members
			WHERE routing_policy_id=p_routing_policy_id
				AND routing_policy_revision=p_expected_routing_policy_revision
		)
	) ORDER BY evidence_id,position FOR SHARE;
	PERFORM 1 FROM decodex.routing_policy_required_capabilities
	WHERE routing_policy_id=p_routing_policy_id
		AND routing_policy_revision=p_expected_routing_policy_revision
	ORDER BY position FOR SHARE;
	PERFORM 1 FROM decodex.quota_windows
	WHERE account_id IN (
		SELECT account_id FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
	) ORDER BY account_id,window_class,duration_minutes FOR SHARE;

	resolved:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.routing_snapshots(
		routing_policy_id,routing_policy_revision,accepted_policy_id,
		accepted_policy_revision,required_role,required_role_profile_revision,
		required_build_id,managed_run_id,managed_run_revision,
		runtime_session_id,runtime_session_revision,account_snapshot_id,
		account_snapshot_source_revision,profile_snapshot_id,
		profile_snapshot_source_revision,resolved_at,consumer_kind,
		conversation_id,conversation_revision,turn_id,managed_execution_id
	) VALUES (
		p_routing_policy_id,p_expected_routing_policy_revision,
		policy_row.accepted_policy_id,policy_row.accepted_policy_revision,
		policy_row.required_role,policy_row.required_role_profile_revision,
		policy_row.required_build_id,p_managed_run_id,p_expected_managed_run_revision,
		session_row.runtime_session_id,session_row.revision,
		session_row.account_snapshot_id,session_row.account_source_revision,
		session_row.profile_snapshot_id,session_row.profile_source_revision,resolved,
		p_consumer_kind,p_conversation_id,p_expected_conversation_revision,
		p_turn_id,p_managed_execution_id
	) RETURNING snapshot_id INTO new_snapshot_id;

	FOR member IN
		SELECT policy_member.position,policy_member.account_id,
			policy_member.account_revision,policy_member.disposition,
			account.display_label,account.state,account.observed_at,
			account.revision AS current_account_revision
		FROM decodex.routing_policy_members AS policy_member
		JOIN decodex.accounts AS account USING(account_id)
		WHERE policy_member.routing_policy_id=p_routing_policy_id
			AND policy_member.routing_policy_revision=p_expected_routing_policy_revision
		ORDER BY policy_member.position
	LOOP
		blockers:=ARRAY[]::decodex.routing_blocker[];
		IF member.disposition='excluded' THEN
			blockers:=pg_catalog.array_append(
				blockers,'excluded_by_policy'::decodex.routing_blocker
			);
		END IF;
		IF member.account_revision<>member.current_account_revision THEN
			blockers:=pg_catalog.array_append(
				blockers,'account_stale'::decodex.routing_blocker
			);
		END IF;
		IF member.observed_at>resolved THEN
			blockers:=pg_catalog.array_append(
				blockers,'account_from_future'::decodex.routing_blocker
			);
		ELSIF resolved-member.observed_at>INTERVAL '300 seconds' THEN
			blockers:=pg_catalog.array_append(
				blockers,'account_stale'::decodex.routing_blocker
			);
		END IF;
		blockers:=pg_catalog.array_append(blockers,CASE member.state
			WHEN 'unavailable' THEN 'account_unavailable'::decodex.routing_blocker
			WHEN 'unknown' THEN 'account_unknown'::decodex.routing_blocker
			WHEN 'depleted' THEN 'account_depleted'::decodex.routing_blocker
			WHEN 'auth_failed' THEN 'account_auth_failed'::decodex.routing_blocker
			WHEN 'plugin_unready' THEN 'account_plugin_unready'::decodex.routing_blocker
			WHEN 'disabled' THEN 'account_disabled'::decodex.routing_blocker
			ELSE NULL
		END);
		blockers:=pg_catalog.array_remove(blockers,NULL);

		SELECT candidate.* INTO evidence
		FROM decodex.routing_compatibility_evidence AS candidate
		WHERE candidate.account_id=member.account_id
		ORDER BY candidate.evidence_revision DESC LIMIT 1;
		IF NOT FOUND THEN
			blockers:=pg_catalog.array_append(
				blockers,'evidence_missing'::decodex.routing_blocker
			);
		ELSE
			IF evidence.ingested_at>resolved THEN
				blockers:=pg_catalog.array_append(
					blockers,'evidence_from_future'::decodex.routing_blocker
				);
			ELSIF resolved-evidence.ingested_at>INTERVAL '300 seconds' THEN
				blockers:=pg_catalog.array_append(
					blockers,'evidence_stale'::decodex.routing_blocker
				);
			END IF;
			IF evidence.account_revision<>member.current_account_revision THEN
				blockers:=pg_catalog.array_append(
					blockers,'evidence_account_mismatch'::decodex.routing_blocker
				);
			END IF;
			IF evidence.role<>policy_row.required_role
				OR evidence.role_profile_revision<>
					policy_row.required_role_profile_revision
			THEN
				blockers:=pg_catalog.array_append(
					blockers,'evidence_profile_mismatch'::decodex.routing_blocker
				);
			END IF;
			IF evidence.build_id<>policy_row.required_build_id THEN
				blockers:=pg_catalog.array_append(
					blockers,'evidence_build_mismatch'::decodex.routing_blocker
				);
			END IF;
		END IF;
		IF EXISTS (
			SELECT 1
			FROM decodex.routing_policy_required_capabilities AS required
			LEFT JOIN decodex.routing_capability_evidence AS actual
				ON actual.evidence_id=evidence.evidence_id
				AND actual.capability=required.capability
			WHERE required.routing_policy_id=p_routing_policy_id
				AND required.routing_policy_revision=p_expected_routing_policy_revision
				AND actual.state IS DISTINCT FROM 'supported'
		) THEN
			blockers:=pg_catalog.array_append(
				blockers,'required_capability_unsatisfied'::decodex.routing_blocker
			);
		END IF;

		FOR quota IN
			SELECT definition.window_class,definition.duration_minutes,
				quota_window.revision,quota_window.remaining_percent,
				quota_window.resets_at,quota_window.observed_at,
				quota_window.confidence
			FROM (
				VALUES
					('five_hour'::decodex.quota_window_class,300::smallint),
					('seven_day'::decodex.quota_window_class,10080::smallint)
			) AS definition(window_class,duration_minutes)
			LEFT JOIN decodex.quota_windows AS quota_window
				ON quota_window.account_id=member.account_id
				AND quota_window.window_class=definition.window_class
				AND quota_window.duration_minutes=definition.duration_minutes
		LOOP
			IF quota.revision IS NULL THEN
				blockers:=pg_catalog.array_append(blockers,CASE quota.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_missing'::decodex.routing_blocker
					ELSE 'quota_seven_day_missing'::decodex.routing_blocker
				END);
			ELSIF quota.observed_at>resolved THEN
				blockers:=pg_catalog.array_append(blockers,CASE quota.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_from_future'::decodex.routing_blocker
					ELSE 'quota_seven_day_from_future'::decodex.routing_blocker
				END);
			ELSIF resolved-quota.observed_at>INTERVAL '300 seconds' THEN
				blockers:=pg_catalog.array_append(blockers,CASE quota.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_stale'::decodex.routing_blocker
					ELSE 'quota_seven_day_stale'::decodex.routing_blocker
				END);
			ELSIF quota.remaining_percent IS NULL OR quota.confidence<>'high' THEN
				blockers:=pg_catalog.array_append(blockers,CASE quota.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_unknown'::decodex.routing_blocker
					ELSE 'quota_seven_day_unknown'::decodex.routing_blocker
				END);
			ELSIF quota.resets_at IS NULL OR quota.resets_at<=resolved THEN
				blockers:=pg_catalog.array_append(blockers,CASE quota.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_reset_elapsed'::decodex.routing_blocker
					ELSE 'quota_seven_day_reset_elapsed'::decodex.routing_blocker
				END);
			ELSIF quota.remaining_percent=0 THEN
				blockers:=pg_catalog.array_append(blockers,CASE quota.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_depleted'::decodex.routing_blocker
					ELSE 'quota_seven_day_depleted'::decodex.routing_blocker
				END);
			END IF;
		END LOOP;

		SELECT pg_catalog.array_agg(DISTINCT blocker ORDER BY blocker)
		INTO blockers
		FROM pg_catalog.unnest(blockers) AS item(blocker);
		blockers:=COALESCE(blockers,ARRAY[]::decodex.routing_blocker[]);
		INSERT INTO decodex.routing_snapshot_members VALUES (
			new_snapshot_id,member.position,member.account_id,member.disposition,
			member.current_account_revision,member.display_label,member.state,
			decodex.rfc3339_utc(member.observed_at),evidence.evidence_id,
			evidence.evidence_revision,evidence.account_revision,evidence.role,
			evidence.role_profile_revision,evidence.build_id,evidence.process_id,
			evidence.schema_fingerprint,member.account_id=sticky_account,blockers
		);
		INSERT INTO decodex.routing_snapshot_quota_facts
		SELECT new_snapshot_id,member.account_id,definition.position,
			definition.window_class,definition.duration_minutes,quota_window.revision,
			quota_window.remaining_percent,
			CASE WHEN quota_window.resets_at IS NULL THEN NULL
				ELSE (extract(epoch FROM quota_window.resets_at)*1000000)::bigint END,
			CASE WHEN quota_window.observed_at IS NULL THEN NULL
				ELSE (extract(epoch FROM quota_window.observed_at)*1000000)::bigint END,
			quota_window.confidence
		FROM (
			VALUES
				(1::smallint,'five_hour'::decodex.quota_window_class,300::smallint),
				(2::smallint,'seven_day'::decodex.quota_window_class,10080::smallint)
		) AS definition(position,window_class,duration_minutes)
		LEFT JOIN decodex.quota_windows AS quota_window
			ON quota_window.account_id=member.account_id
			AND quota_window.window_class=definition.window_class
			AND quota_window.duration_minutes=definition.duration_minutes;
		INSERT INTO decodex.routing_snapshot_capability_facts
		SELECT new_snapshot_id,member.account_id,definition.position,
			definition.capability,required.capability IS NOT NULL,actual.state
		FROM (
			VALUES
				(1::smallint,'initialize'::decodex.codex_capability),
				(2::smallint,'account_read'),(3::smallint,'thread_list'),
				(4::smallint,'thread_read'),(5::smallint,'thread_archive'),
				(6::smallint,'paginated_history'),
				(7::smallint,'native_collaboration'),(8::smallint,'thread_search')
		) AS definition(position,capability)
		LEFT JOIN decodex.routing_policy_required_capabilities AS required
			ON required.routing_policy_id=p_routing_policy_id
			AND required.routing_policy_revision=p_expected_routing_policy_revision
			AND required.capability=definition.capability
		LEFT JOIN decodex.routing_capability_evidence AS actual
			ON actual.evidence_id=evidence.evidence_id
			AND actual.capability=definition.capability;
		INSERT INTO decodex.routing_snapshot_blockers
		SELECT new_snapshot_id,member.account_id,ordinality::integer,blocker
		FROM pg_catalog.unnest(blockers) WITH ORDINALITY
			AS item(blocker,ordinality);
	END LOOP;

	core:=pg_catalog.jsonb_build_object(
		'operation','resolve_routing_snapshot',
		'snapshot_id',new_snapshot_id,
		'routing_policy_id',p_routing_policy_id,
		'routing_policy_revision',p_expected_routing_policy_revision,
		'accepted_policy_id',policy_row.accepted_policy_id,
		'accepted_policy_revision',policy_row.accepted_policy_revision,
		'required_role',policy_row.required_role,
		'required_role_profile_revision',policy_row.required_role_profile_revision,
		'required_build_id',policy_row.required_build_id,
		'consumer_kind',p_consumer_kind,
		'conversation_id',p_conversation_id,
		'conversation_revision',p_expected_conversation_revision,
		'turn_id',p_turn_id,
		'managed_run_id',p_managed_run_id,
		'managed_run_revision',p_expected_managed_run_revision,
		'managed_execution_id',p_managed_execution_id,
		'runtime_session_id',session_row.runtime_session_id,
		'runtime_session_revision',session_row.revision,
		'account_snapshot_id',session_row.account_snapshot_id,
		'account_snapshot_source_revision',session_row.account_source_revision,
		'profile_snapshot_id',session_row.profile_snapshot_id,
		'profile_snapshot_source_revision',session_row.profile_source_revision,
		'resolved_at_micros',(extract(epoch FROM resolved)*1000000)::bigint,
		'members',(
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.to_jsonb(member_row) ORDER BY position
			)
			FROM decodex.routing_snapshot_members AS member_row
			WHERE snapshot_id=new_snapshot_id
		),
		'quota_facts',(
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.to_jsonb(quota_row)
				ORDER BY snapshot_member.position,quota_row.position
			)
			FROM decodex.routing_snapshot_quota_facts AS quota_row
			JOIN decodex.routing_snapshot_members AS snapshot_member
				USING(snapshot_id,account_id)
			WHERE quota_row.snapshot_id=new_snapshot_id
		),
		'capability_facts',(
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.to_jsonb(capability_row)
				ORDER BY snapshot_member.position,capability_row.position
			)
			FROM decodex.routing_snapshot_capability_facts AS capability_row
			JOIN decodex.routing_snapshot_members AS snapshot_member
				USING(snapshot_id,account_id)
			WHERE capability_row.snapshot_id=new_snapshot_id
		),
		'blockers',(
			SELECT COALESCE(
				pg_catalog.jsonb_agg(
					pg_catalog.to_jsonb(blocker_row)
					ORDER BY snapshot_member.position,blocker_row.position
				),
				'[]'::jsonb
			)
			FROM decodex.routing_snapshot_blockers AS blocker_row
			JOIN decodex.routing_snapshot_members AS snapshot_member
				USING(snapshot_id,account_id)
			WHERE blocker_row.snapshot_id=new_snapshot_id
		)
	);
	effect:=core||pg_catalog.jsonb_build_object(
		'effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
			public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'
		)
	);
	response:=pg_catalog.convert_to(
		pg_catalog.jsonb_build_object(
			'classification','success','effect',effect
		)::text,
		'UTF8'
	);
	UPDATE decodex.exact_command_receipts
	SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol
		AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE OR REPLACE FUNCTION decodex.prepare_provider_attempt_exact(
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
			COALESCE(p_consumer_kind::text,'invalid')||':'||
			COALESCE(
				p_turn_id::text,p_managed_execution_id::text,'invalid'
			)
		)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1402,pg_catalog.hashtext(p_attempt_id::text)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1403,pg_catalog.hashtext(p_request_id::text)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1404,pg_catalog.hashtext(p_continuation_plan_id::text)
	);

	SELECT * INTO existing
	FROM decodex.provider_attempts
	WHERE attempt_id=p_attempt_id
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
			p_consumer_kind,p_conversation_id,p_turn_id,p_managed_run_id,
			p_managed_run_revision,p_managed_execution_id,
			p_continuation_plan_id,p_process_generation_id,
			p_process_generation_revision,p_request_id,p_request_digest,
			p_provider_idempotency_key,p_provider_correlation_key,
			p_predecessor_attempt_id,p_duplicate_risk_ack_digest
		) THEN
			RETURN QUERY SELECT
				'identity_conflict',existing.revision,existing.state,
				(extract(epoch FROM existing.created_at)*1000000)::bigint,
				(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		ELSE
			RETURN QUERY SELECT
				'replayed',existing.revision,existing.state,
				(extract(epoch FROM existing.created_at)*1000000)::bigint,
				(extract(epoch FROM existing.updated_at)*1000000)::bigint;
		END IF;
		RETURN;
	END IF;

	IF p_attempt_id IS NULL
		OR p_continuation_plan_id IS NULL
		OR p_process_generation_id IS NULL
		OR p_process_generation_revision IS NULL
		OR p_process_generation_revision<=0
		OR p_request_id IS NULL
		OR p_request_digest IS NULL
		OR p_request_digest COLLATE pg_catalog."C" !~ '^[0-9a-f]{64}$'
		OR (
			p_provider_idempotency_key IS NULL
			AND p_provider_correlation_key IS NULL
		)
		OR (
			p_provider_idempotency_key IS NOT NULL
			AND (
				pg_catalog.octet_length(p_provider_idempotency_key)
					NOT BETWEEN 1 AND 512
				OR p_provider_idempotency_key COLLATE pg_catalog."C"
					~ '[[:cntrl:]]'
			)
		)
		OR (
			p_provider_correlation_key IS NOT NULL
			AND (
				pg_catalog.octet_length(p_provider_correlation_key)
					NOT BETWEEN 1 AND 512
				OR p_provider_correlation_key COLLATE pg_catalog."C"
					~ '[[:cntrl:]]'
			)
		)
		OR NOT COALESCE((
			(
				p_consumer_kind::text='conversation_turn'
				AND p_conversation_id IS NOT NULL
				AND p_turn_id IS NOT NULL
				AND p_managed_run_id IS NULL
				AND p_managed_run_revision IS NULL
				AND p_managed_execution_id IS NULL
			) OR (
				p_consumer_kind::text='managed_run_execution'
				AND p_conversation_id IS NULL
				AND p_turn_id IS NULL
				AND p_managed_run_id IS NOT NULL
				AND p_managed_run_revision>0
				AND p_managed_execution_id IS NOT NULL
			)
		),false)
		OR (
			(p_predecessor_attempt_id IS NULL) <>
				(p_duplicate_risk_ack_digest IS NULL)
		)
		OR (
			p_duplicate_risk_ack_digest IS NOT NULL
			AND p_duplicate_risk_ack_digest COLLATE pg_catalog."C"
				!~ '^[0-9a-f]{64}$'
		)
	THEN
		RETURN QUERY SELECT
			'invalid_input',0::bigint,
			'prepared'::decodex.provider_attempt_state,
			0::bigint,0::bigint;
		RETURN;
	END IF;

	SELECT * INTO plan
	FROM decodex.continuation_plans
	WHERE plan_id=p_continuation_plan_id
	FOR SHARE;
	IF NOT FOUND OR (
		plan.consumer_kind,
		plan.consumer_conversation_id,
		plan.turn_id,
		plan.managed_run_id,
		plan.managed_run_revision,
		plan.managed_execution_id
	) IS DISTINCT FROM (
		p_consumer_kind,p_conversation_id,p_turn_id,p_managed_run_id,
		p_managed_run_revision,p_managed_execution_id
	) THEN
		RETURN QUERY SELECT
			'authority_unavailable',0::bigint,
			'prepared'::decodex.provider_attempt_state,
			0::bigint,0::bigint;
		RETURN;
	END IF;
	IF plan.kind='same_thread' THEN
		accepted_session_id:=plan.source_runtime_session_id;
		accepted_session_revision:=plan.source_runtime_session_revision;
	ELSE
		accepted_session_id:=plan.fallback_runtime_session_id;
		accepted_session_revision:=1;
	END IF;

	SELECT * INTO generation
	FROM decodex.process_generations
	WHERE generation_id=p_process_generation_id
	FOR SHARE;
	IF NOT FOUND
		OR generation.revision<>p_process_generation_revision
		OR generation.state<>'ready'
		OR generation.account_id<>plan.selected_account_id
		OR generation.process_id IS NULL
		OR NOT EXISTS (
			SELECT 1
			FROM decodex.process_generation_execution_epochs AS epoch
			WHERE epoch.execution_epoch_id=generation.execution_epoch_id
				AND epoch.retired_at IS NULL
		)
	THEN
		RETURN QUERY SELECT
			'generation_unavailable',0::bigint,
			'prepared'::decodex.provider_attempt_state,
			0::bigint,0::bigint;
		RETURN;
	END IF;

	IF NOT EXISTS (
		SELECT 1
		FROM decodex.routing_decisions AS decision
		JOIN decodex.runtime_sessions AS session
			ON (session.runtime_session_id,session.revision)=
				(accepted_session_id,accepted_session_revision)
		JOIN decodex.account_snapshots AS account
			ON account.account_snapshot_id=session.account_snapshot_id
		WHERE decision.decision_id=plan.routing_decision_id
			AND decision.kind='selected'
			AND decision.selected_account_id=plan.selected_account_id
			AND account.source_account_id=plan.selected_account_id
			AND (
				(plan.kind='same_thread' AND session.state='active')
				OR (
					plan.kind='context_pack_fallback'
					AND session.state='starting'
				)
			)
		FOR SHARE OF decision,session,account
	) THEN
		RETURN QUERY SELECT
			'authority_unavailable',0::bigint,
			'prepared'::decodex.provider_attempt_state,
			0::bigint,0::bigint;
		RETURN;
	END IF;

	IF (
		p_consumer_kind::text='conversation_turn'
		AND (
			NOT EXISTS (
				SELECT 1
				FROM decodex.conversations AS conversation
				WHERE conversation.conversation_id=p_conversation_id
					AND conversation.revision=plan.conversation_revision
					AND conversation.status='open'
				FOR SHARE
			) OR EXISTS (
				SELECT 1
				FROM decodex.turns AS turn
				WHERE turn.turn_id=p_turn_id
					AND (
						turn.conversation_id<>p_conversation_id
						OR turn.runtime_session_id<>
							accepted_session_id
						OR turn.status<>'active'
					)
			)
		)
	) OR (
		p_consumer_kind::text='managed_run_execution'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.managed_runs AS run
			WHERE (run.managed_run_id,run.revision)=
				(p_managed_run_id,p_managed_run_revision)
			FOR SHARE
		)
	) THEN
		RETURN QUERY SELECT
			'consumer_unavailable',0::bigint,
			'prepared'::decodex.provider_attempt_state,
			0::bigint,0::bigint;
		RETURN;
	END IF;

	PERFORM pg_catalog.pg_advisory_xact_lock(
		1405,
		pg_catalog.hashtext(
			plan.selected_account_id::text||':'||
			COALESCE(p_provider_idempotency_key,'')
		)
	);
	IF EXISTS (
		SELECT 1
		FROM decodex.provider_attempts AS assigned
		WHERE assigned.request_id=p_request_id
			OR assigned.continuation_plan_id=p_continuation_plan_id
			OR (
				p_provider_idempotency_key IS NOT NULL
				AND assigned.selected_account_id=plan.selected_account_id
				AND assigned.provider_idempotency_key=
					p_provider_idempotency_key
			)
	) THEN
		RETURN QUERY SELECT
			'identity_conflict',0::bigint,
			'prepared'::decodex.provider_attempt_state,
			0::bigint,0::bigint;
		RETURN;
	END IF;

	now_value:=pg_catalog.clock_timestamp();
	INSERT INTO decodex.provider_attempts(
		attempt_id,consumer_kind,conversation_id,turn_id,managed_run_id,
		managed_run_revision,managed_execution_id,continuation_plan_id,
		routing_decision_id,accepted_runtime_session_id,
		accepted_runtime_session_revision,selected_account_id,
		process_generation_id,process_generation_revision,
		process_execution_epoch_id,request_id,request_digest,
		provider_idempotency_key,provider_correlation_key,
		predecessor_attempt_id,duplicate_risk_ack_digest,state,revision,
		created_at,updated_at
	) VALUES (
		p_attempt_id,p_consumer_kind,p_conversation_id,p_turn_id,
		p_managed_run_id,p_managed_run_revision,p_managed_execution_id,
		p_continuation_plan_id,plan.routing_decision_id,
		accepted_session_id,accepted_session_revision,
		plan.selected_account_id,p_process_generation_id,
		p_process_generation_revision,generation.execution_epoch_id,
		p_request_id,p_request_digest,p_provider_idempotency_key,
		p_provider_correlation_key,p_predecessor_attempt_id,
		p_duplicate_risk_ack_digest,'prepared',1,now_value,now_value
	);
	RETURN QUERY SELECT
		'prepared',1::bigint,
		'prepared'::decodex.provider_attempt_state,
		(extract(epoch FROM now_value)*1000000)::bigint,
		(extract(epoch FROM now_value)*1000000)::bigint;
END
$$;

CREATE FUNCTION decodex.plan_continuation_exact(
	p_protocol text,p_idempotency_key text,p_operation_id uuid,
	p_decision_id uuid,p_expected_consumer_revision bigint,p_plan_id uuid,
	p_fallback_session_id uuid,p_account_snapshot_id uuid,p_context_pack_id uuid,
	p_compiled_bytes bytea,p_compiled_digest text,p_manifest_digest text,
	p_max_bytes integer,p_recent_item_limit integer,p_possible_side_effects text,
	p_truncated boolean,p_omitted_source_count integer,
	p_source_kinds text[],p_source_ids text[],p_source_revisions bigint[],
	p_content_digests text[],p_original_lengths bigint[],p_included_lengths bigint[],
	p_included_digests text[],p_dispositions text[],p_artifact_ids text[],
	p_artifact_revisions bigint[]
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; existing_plan record; decision_row record;
DECLARE snapshot_row record; run_row record; session_row record; profile_row record;
DECLARE conversation_row record; member_row record;
DECLARE evidence_count bigint; selected_evidence_id uuid:=NULL;
DECLARE selected_evidence_revision bigint:=NULL;
DECLARE selected_schema_fingerprint text:=NULL;
DECLARE selected_experiment_id uuid:=NULL;
DECLARE selected_experiment_revision bigint:=NULL;
DECLARE selected_observation_id uuid:=NULL;
DECLARE selected_provider_attempt_id uuid:=NULL;
DECLARE selected_provider_attempt_revision bigint:=NULL;
DECLARE selected_provider_evidence_id uuid:=NULL;
DECLARE planned timestamptz; pack_revision bigint; source_count integer;
DECLARE position integer; inline_value bytea; blob_value text;
DECLARE plan_kind decodex.continuation_plan_kind; thread_value uuid;
DECLARE same_thread_available boolean:=false;
DECLARE core jsonb; effect jsonb; response bytea;
DECLARE activity_sequence bigint; outbox_id bigint;
DECLARE activity_rows jsonb:='[]'::jsonb;
DECLARE outbox_rows jsonb:='[]'::jsonb; payload jsonb;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','plan_continuation',
		'protocol',p_protocol,
		'operation_id',p_operation_id,
		'decision_id',p_decision_id,
		'expected_consumer_revision',p_expected_consumer_revision,
		'plan_id',p_plan_id,
		'fallback_session_id',p_fallback_session_id,
		'account_snapshot_id',p_account_snapshot_id,
		'context_pack_id',p_context_pack_id,
		'compiled_digest',p_compiled_digest,
		'manifest_digest',p_manifest_digest,
		'byte_length',pg_catalog.octet_length(p_compiled_bytes),
		'max_bytes',p_max_bytes,
		'recent_item_limit',p_recent_item_limit,
		'possible_side_effects',p_possible_side_effects,
		'truncated',p_truncated,
		'omitted_source_count',p_omitted_source_count,
		'source_kinds',p_source_kinds,
		'source_ids',p_source_ids,
		'source_revisions',p_source_revisions,
		'content_digests',p_content_digests,
		'original_lengths',p_original_lengths,
		'included_lengths',p_included_lengths,
		'included_digests',p_included_digests,
		'dispositions',p_dispositions,
		'artifact_ids',p_artifact_ids,
		'artifact_revisions',p_artifact_revisions
	);
	replay:=decodex.reserve_exact_continuation_command(
		p_protocol,p_idempotency_key,request
	);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL
		OR p_decision_id IS NULL
		OR p_plan_id IS NULL
		OR p_fallback_session_id IS NULL
		OR p_account_snapshot_id IS NULL
		OR p_context_pack_id IS NULL
		OR p_expected_consumer_revision IS NULL
		OR p_expected_consumer_revision<=0
	THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'invalid_input'
		);
	END IF;

	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1360,pg_catalog.hashtext(p_decision_id::text)
	);
	SELECT * INTO decision_row
	FROM decodex.routing_decisions
	WHERE decision_id=p_decision_id
	FOR UPDATE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'missing_decision'
		);
	END IF;
	IF decision_row.kind<>'selected' THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'decision_not_selected'
		);
	END IF;
	SELECT * INTO existing_plan
	FROM decodex.continuation_plans
	WHERE routing_decision_id=p_decision_id
		OR operation_id=p_operation_id
		OR plan_id=p_plan_id
	FOR SHARE;
	IF FOUND THEN
		IF existing_plan.routing_decision_id<>p_decision_id
			OR existing_plan.operation_id<>p_operation_id
			OR existing_plan.plan_id<>p_plan_id
			OR existing_plan.request_envelope<>request
		THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'decision_already_consumed'
			);
		END IF;
		UPDATE decodex.exact_command_receipts
		SET receipt_state='completed_success',outcome_class='success',
			effect_envelope=existing_plan.effect_envelope,
			response_bytes=existing_plan.response_bytes,
			completed_at=pg_catalog.clock_timestamp()
		WHERE protocol_version=p_protocol
			AND idempotency_key=p_idempotency_key;
		RETURN existing_plan.response_bytes;
	END IF;

	SELECT * INTO snapshot_row
	FROM decodex.routing_snapshots
	WHERE snapshot_id=decision_row.snapshot_id
	FOR SHARE;
	IF snapshot_row.snapshot_id IS NULL
		OR (
			decision_row.consumer_kind,
			decision_row.conversation_id,
			decision_row.conversation_revision,
			decision_row.turn_id,
			decision_row.managed_run_id,
			decision_row.managed_run_revision,
			decision_row.managed_execution_id
		) IS DISTINCT FROM (
			snapshot_row.consumer_kind,
			snapshot_row.conversation_id,
			snapshot_row.conversation_revision,
			snapshot_row.turn_id,
			snapshot_row.managed_run_id,
			snapshot_row.managed_run_revision,
			snapshot_row.managed_execution_id
		)
	THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'stale_consumer_revision'
		);
	END IF;
	IF decision_row.consumer_kind::text='conversation_turn' THEN
		SELECT * INTO conversation_row
		FROM decodex.conversations
		WHERE conversation_id=decision_row.conversation_id
		FOR UPDATE;
		IF conversation_row.conversation_id IS NULL
			OR conversation_row.revision<>p_expected_consumer_revision
			OR decision_row.conversation_revision<>p_expected_consumer_revision
			OR conversation_row.status<>'open'
		THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'stale_consumer_revision'
			);
		END IF;
	ELSE
		SELECT * INTO run_row
		FROM decodex.managed_runs
		WHERE managed_run_id=decision_row.managed_run_id
		FOR UPDATE;
		IF run_row.managed_run_id IS NULL
			OR run_row.revision<>p_expected_consumer_revision
			OR decision_row.managed_run_revision<>p_expected_consumer_revision
		THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'stale_consumer_revision'
			);
		END IF;
	END IF;

	SELECT * INTO session_row
	FROM decodex.runtime_sessions
	WHERE (runtime_session_id,revision)=(
		snapshot_row.runtime_session_id,
		snapshot_row.runtime_session_revision
	)
	FOR SHARE;
	SELECT * INTO profile_row
	FROM decodex.profile_snapshots
	WHERE profile_snapshot_id=snapshot_row.profile_snapshot_id
	FOR SHARE;
	SELECT * INTO conversation_row
	FROM decodex.conversations
	WHERE conversation_id=session_row.conversation_id
	FOR SHARE;
	SELECT * INTO member_row
	FROM decodex.routing_snapshot_members
	WHERE snapshot_id=snapshot_row.snapshot_id
		AND account_id=decision_row.selected_account_id
	FOR SHARE;
	IF session_row.runtime_session_id IS NULL
		OR profile_row.profile_snapshot_id IS NULL
		OR conversation_row.conversation_id IS NULL
		OR conversation_row.status<>'open'
		OR member_row.account_id IS NULL
		OR member_row.disposition<>'included'
		OR EXISTS (
			SELECT 1
			FROM decodex.routing_decision_blocker_refs AS blocker
			WHERE blocker.decision_id=decision_row.decision_id
				AND blocker.account_id=decision_row.selected_account_id
		)
		OR (
			session_row.profile_snapshot_id,
			profile_row.role,
			profile_row.source_revision
		) IS DISTINCT FROM (
			snapshot_row.profile_snapshot_id,
			snapshot_row.required_role,
			snapshot_row.required_role_profile_revision
		)
		OR (
			decision_row.consumer_kind::text='conversation_turn'
			AND session_row.conversation_id<>decision_row.conversation_id
		)
	THEN
		RETURN decodex.complete_exact_continuation_rejection(
			p_protocol,p_idempotency_key,'stale_consumer_revision'
		);
	END IF;

	planned:=pg_catalog.clock_timestamp();
	IF decision_row.consumer_kind::text='managed_run_execution' THEN
		SELECT pg_catalog.count(*) INTO evidence_count
		FROM (
			SELECT experiment.experiment_id
			FROM decodex.routing_compatibility_evidence AS evidence
			JOIN decodex.routing_capability_evidence AS capability
				ON capability.evidence_id=evidence.evidence_id
			JOIN decodex.codex_experiments AS experiment
				ON (
					experiment.managed_run_id,
					experiment.managed_run_revision,
					experiment.routing_snapshot_id,
					experiment.account_id,
					experiment.account_revision,
					experiment.role_profile_revision,
					experiment.build_id,
					experiment.revision,
					experiment.state
				)=(
					decision_row.managed_run_id,
					decision_row.managed_run_revision,
					snapshot_row.snapshot_id,
					decision_row.selected_account_id,
					member_row.account_revision,
					snapshot_row.required_role_profile_revision,
					snapshot_row.required_build_id,
					3,
					'thread_bound'
				)
			JOIN decodex.codex_experiment_thread_bindings AS binding
				ON binding.experiment_id=experiment.experiment_id
			JOIN decodex.codex_experiment_observations AS observation
				ON observation.experiment_id=experiment.experiment_id
				AND observation.experiment_revision=3
				AND observation.thread_id=binding.thread_id
				AND observation.kind='thread_read_item'
			WHERE evidence.evidence_id=member_row.evidence_id
				AND evidence.evidence_revision=member_row.evidence_revision
				AND evidence.account_id=decision_row.selected_account_id
				AND evidence.account_revision=member_row.account_revision
				AND evidence.role=snapshot_row.required_role
				AND evidence.role_profile_revision=
					snapshot_row.required_role_profile_revision
				AND evidence.build_id=snapshot_row.required_build_id
				AND evidence.process_account_id=
					decision_row.selected_account_id
				AND session_row.codex_thread_id IS NOT NULL
				AND binding.thread_id=session_row.codex_thread_id::text
				AND session_row.state='active'
				AND NOT run_row.diverged
				AND evidence.ingested_at<=planned
				AND planned-evidence.ingested_at<=INTERVAL '300 seconds'
				AND experiment.updated_at<=planned
				AND planned-experiment.updated_at<=INTERVAL '300 seconds'
				AND observation.observed_at<=planned
				AND planned-observation.observed_at<=INTERVAL '300 seconds'
				AND capability.capability IN (
					'initialize','account_read','thread_read','paginated_history'
				)
				AND capability.state='supported'
			GROUP BY experiment.experiment_id
			HAVING pg_catalog.count(DISTINCT capability.capability)=4
		) AS canonical_experiment;
		IF evidence_count=1 THEN
			SELECT evidence.evidence_id,evidence.evidence_revision,
				evidence.schema_fingerprint,experiment.experiment_id,
				experiment.revision,observation.observation_id
			INTO selected_evidence_id,selected_evidence_revision,
				selected_schema_fingerprint,selected_experiment_id,
				selected_experiment_revision,selected_observation_id
			FROM decodex.routing_compatibility_evidence AS evidence
			JOIN decodex.codex_experiments AS experiment
				ON experiment.managed_run_id=decision_row.managed_run_id
				AND experiment.managed_run_revision=
					decision_row.managed_run_revision
				AND experiment.routing_snapshot_id=snapshot_row.snapshot_id
				AND experiment.account_id=decision_row.selected_account_id
				AND experiment.account_revision=member_row.account_revision
				AND experiment.role_profile_revision=
					snapshot_row.required_role_profile_revision
				AND experiment.build_id=snapshot_row.required_build_id
				AND experiment.revision=3
				AND experiment.state='thread_bound'
			JOIN decodex.codex_experiment_thread_bindings AS binding
				ON binding.experiment_id=experiment.experiment_id
			JOIN decodex.codex_experiment_observations AS observation
				ON observation.experiment_id=experiment.experiment_id
				AND observation.experiment_revision=3
				AND observation.thread_id=binding.thread_id
				AND observation.kind='thread_read_item'
			WHERE evidence.evidence_id=member_row.evidence_id
				AND evidence.evidence_revision=member_row.evidence_revision
				AND binding.thread_id=session_row.codex_thread_id::text
				AND evidence.ingested_at<=planned
				AND planned-evidence.ingested_at<=INTERVAL '300 seconds'
				AND experiment.updated_at<=planned
				AND planned-experiment.updated_at<=INTERVAL '300 seconds'
				AND observation.observed_at<=planned
				AND planned-observation.observed_at<=INTERVAL '300 seconds'
			ORDER BY observation.observed_at DESC,observation.observation_id
			LIMIT 1;
			same_thread_available:=true;
		END IF;
	ELSE
		SELECT attempt.attempt_id,attempt.revision,evidence.evidence_id
		INTO selected_provider_attempt_id,
			selected_provider_attempt_revision,
			selected_provider_evidence_id
		FROM decodex.provider_attempts AS attempt
		JOIN decodex.provider_attempt_positive_evidence AS evidence
			ON evidence.evidence_id=attempt.terminal_evidence_id
			AND evidence.attempt_id=attempt.attempt_id
		WHERE attempt.consumer_kind::text='conversation_turn'
			AND attempt.conversation_id=decision_row.conversation_id
			AND attempt.turn_id<>decision_row.turn_id
			AND attempt.accepted_runtime_session_id=
				session_row.runtime_session_id
			AND attempt.accepted_runtime_session_revision=session_row.revision
			AND attempt.selected_account_id=decision_row.selected_account_id
			AND attempt.state IN ('succeeded','failed_definitive')
			AND evidence.outcome::text=attempt.state::text
			AND evidence.source='exact_thread_readback'
			AND evidence.provider_thread_id=session_row.codex_thread_id::text
			AND session_row.codex_thread_id IS NOT NULL
			AND session_row.state='active'
			AND evidence.observed_at<=planned
			AND planned-evidence.observed_at<=INTERVAL '300 seconds'
		ORDER BY evidence.observed_at DESC,evidence.evidence_id
		LIMIT 1;
		same_thread_available:=FOUND;
	END IF;

	IF same_thread_available THEN
		plan_kind:='same_thread';
		thread_value:=session_row.codex_thread_id;
	ELSE
		plan_kind:='context_pack_fallback';
		thread_value:=NULL;
		IF NOT decodex.is_canonical_continuation_pack(
			session_row.conversation_id,p_compiled_bytes,p_compiled_digest,
			p_manifest_digest,p_max_bytes,p_recent_item_limit,
			p_possible_side_effects,p_truncated,p_omitted_source_count,
			p_source_kinds,p_source_ids,p_source_revisions,p_content_digests,
			p_original_lengths,p_included_lengths,p_included_digests,
			p_dispositions,p_artifact_ids,p_artifact_revisions
		) THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'invalid_context_pack'
			);
		END IF;
		IF EXISTS (
			SELECT 1 FROM decodex.runtime_sessions
			WHERE runtime_session_id=p_fallback_session_id
		) OR EXISTS (
			SELECT 1 FROM decodex.account_snapshots
			WHERE account_snapshot_id=p_account_snapshot_id
		) OR EXISTS (
			SELECT 1 FROM decodex.context_packs
			WHERE context_pack_id=p_context_pack_id
		) THEN
			RETURN decodex.complete_exact_continuation_rejection(
				p_protocol,p_idempotency_key,'fallback_identity_conflict'
			);
		END IF;
		SELECT COALESCE(pg_catalog.max(stored.pack_revision),0)+1
		INTO pack_revision
		FROM decodex.context_packs AS stored
		WHERE stored.conversation_id=session_row.conversation_id;
		source_count:=pg_catalog.cardinality(p_source_kinds);
		FOR position IN 1..source_count LOOP
			IF p_artifact_ids[position]<>''
				AND NOT EXISTS (
					SELECT 1
					FROM decodex.artifact_revisions AS artifact
					JOIN decodex.blob_objects AS blob
						ON blob.blob_hash=artifact.blob_hash
					WHERE artifact.artifact_id=
							p_artifact_ids[position]::uuid
						AND artifact.conversation_id=
							session_row.conversation_id
						AND artifact.revision=
							p_artifact_revisions[position]
						AND artifact.blob_hash=
							p_content_digests[position]
						AND blob.byte_length=
							p_original_lengths[position]
				)
			THEN
				RETURN decodex.complete_exact_continuation_rejection(
					p_protocol,p_idempotency_key,'invalid_context_pack'
				);
			END IF;
		END LOOP;
		IF pg_catalog.octet_length(p_compiled_bytes)>16384 THEN
			inline_value:=NULL;
			blob_value:=p_compiled_digest;
			INSERT INTO decodex.blob_objects(
				blob_hash,byte_length,verified_at,created_at
			) VALUES (
				blob_value,pg_catalog.octet_length(p_compiled_bytes),
				planned,planned
			) ON CONFLICT (blob_hash) DO NOTHING;
			IF NOT EXISTS (
				SELECT 1
				FROM decodex.blob_objects
				WHERE blob_hash=blob_value
					AND byte_length=
						pg_catalog.octet_length(p_compiled_bytes)
			) THEN
				RETURN decodex.complete_exact_continuation_rejection(
					p_protocol,p_idempotency_key,'invalid_context_pack'
				);
			END IF;
		ELSE
			inline_value:=p_compiled_bytes;
			blob_value:=NULL;
		END IF;
		FOR position IN 1..source_count LOOP
			INSERT INTO decodex.context_pack_sources(
				context_pack_id,conversation_id,position,kind,
				source_id,source_revision,content_digest,
				original_byte_length,included_byte_length,included_digest,
				disposition,artifact_id,artifact_revision
			) VALUES (
				p_context_pack_id,session_row.conversation_id,position-1,
				p_source_kinds[position]::decodex.context_source_kind,
				p_source_ids[position],p_source_revisions[position],
				p_content_digests[position],p_original_lengths[position],
				p_included_lengths[position],p_included_digests[position],
				p_dispositions[position]::decodex.context_source_disposition,
				NULLIF(p_artifact_ids[position],'')::uuid,
				NULLIF(p_artifact_revisions[position],0)
			);
		END LOOP;
		INSERT INTO decodex.context_packs(
			context_pack_id,conversation_id,pack_revision,compiled_digest,
			manifest_digest,inline_bytes,blob_hash,byte_length,max_bytes,
			recent_item_limit,possible_side_effects,truncated,
			omitted_source_count,source_count
		) VALUES (
			p_context_pack_id,session_row.conversation_id,pack_revision,
			p_compiled_digest,p_manifest_digest,inline_value,blob_value,
			pg_catalog.octet_length(p_compiled_bytes),p_max_bytes,
			p_recent_item_limit,
			p_possible_side_effects::decodex.side_effect_state,
			p_truncated,p_omitted_source_count,source_count
		);
		INSERT INTO decodex.account_snapshots(
			account_snapshot_id,source_account_id,display_label,
			observed_state,source_revision
		) VALUES (
			p_account_snapshot_id,member_row.account_id,
			member_row.display_label,member_row.account_state,
			member_row.account_revision
		);
		INSERT INTO decodex.runtime_sessions(
			runtime_session_id,conversation_id,profile_snapshot_id,
			account_snapshot_id,codex_thread_id,state,last_known_turn_id
		) VALUES (
			p_fallback_session_id,session_row.conversation_id,
			profile_row.profile_snapshot_id,p_account_snapshot_id,
			NULL,'starting',NULL
		);
	END IF;

	core:=pg_catalog.jsonb_build_object(
		'operation','plan_continuation',
		'plan_id',p_plan_id,
		'operation_id',p_operation_id,
		'routing_decision_id',p_decision_id,
		'consumer_kind',decision_row.consumer_kind,
		'consumer_conversation_id',decision_row.conversation_id,
		'conversation_revision',decision_row.conversation_revision,
		'turn_id',decision_row.turn_id,
		'managed_run_id',decision_row.managed_run_id,
		'managed_run_revision',decision_row.managed_run_revision,
		'managed_execution_id',decision_row.managed_execution_id,
		'conversation_id',session_row.conversation_id,
		'source_runtime_session_id',session_row.runtime_session_id,
		'source_runtime_session_revision',session_row.revision,
		'selected_account_id',decision_row.selected_account_id,
		'kind',plan_kind,
		'codex_thread_id',thread_value,
		'fallback_context_pack_id',CASE
			WHEN plan_kind='context_pack_fallback' THEN p_context_pack_id
		END,
		'fallback_context_pack_revision',CASE
			WHEN plan_kind='context_pack_fallback' THEN pack_revision
		END,
		'fallback_runtime_session_id',CASE
			WHEN plan_kind='context_pack_fallback' THEN p_fallback_session_id
		END,
		'routing_evidence_id',selected_evidence_id,
		'routing_evidence_revision',selected_evidence_revision,
		'schema_fingerprint',selected_schema_fingerprint,
		'codex_experiment_id',selected_experiment_id,
		'codex_experiment_revision',selected_experiment_revision,
		'codex_observation_id',selected_observation_id,
		'same_thread_provider_attempt_id',selected_provider_attempt_id,
		'same_thread_provider_attempt_revision',
			selected_provider_attempt_revision,
		'same_thread_provider_evidence_id',selected_provider_evidence_id,
		'replay_permitted',false,
		'dispatch_enabled',false,
		'planned_at_micros',
			(extract(epoch FROM planned)*1000000)::bigint
	);
	payload:=core||pg_catalog.jsonb_build_object(
		'continuation_plan_id',p_plan_id
	);
	INSERT INTO decodex.activity(
		aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload
	) VALUES (
		'continuation_plan',p_plan_id::text,1,
		'continuation_plan_created',p_idempotency_key,payload
	) RETURNING sequence INTO activity_sequence;
	activity_rows:=activity_rows||pg_catalog.jsonb_build_array(
		pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,
			'aggregate_kind','continuation_plan',
			'aggregate_id',p_plan_id,
			'revision',1,
			'event_kind','continuation_plan_created',
			'payload',payload
		)
	);
	INSERT INTO decodex.outbox(
		effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload
	) VALUES (
		'activity/'||activity_sequence::text,
		'continuation_plan',p_plan_id::text,1,
		pg_catalog.jsonb_build_object(
			'activity_sequence',activity_sequence,
			'event_kind','continuation_plan_created',
			'aggregate_kind','continuation_plan',
			'aggregate_id',p_plan_id,
			'revision',1,
			'payload',payload
		)
	) RETURNING id INTO outbox_id;
	outbox_rows:=outbox_rows||pg_catalog.jsonb_build_array(
		pg_catalog.jsonb_build_object(
			'id',outbox_id,
			'effect_key','activity/'||activity_sequence::text,
			'aggregate_kind','continuation_plan',
			'aggregate_id',p_plan_id,
			'aggregate_revision',1
		)
	);
	IF plan_kind='context_pack_fallback' THEN
		payload:=pg_catalog.jsonb_build_object(
			'kind','context_pack',
			'continuation_plan_id',p_plan_id,
			'routing_decision_id',p_decision_id,
			'fallback_context_pack_id',p_context_pack_id,
			'conversation_id',session_row.conversation_id,
			'pack_revision',pack_revision,
			'compiled_digest',p_compiled_digest,
			'dispatch_enabled',false
		);
		INSERT INTO decodex.activity(
			aggregate_kind,aggregate_id,revision,event_kind,
			correlation_key,payload
		) VALUES (
			'context_pack',p_context_pack_id::text,pack_revision,
			'context_pack_persisted',p_idempotency_key,payload
		) RETURNING sequence INTO activity_sequence;
		activity_rows:=activity_rows||pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object(
				'sequence',activity_sequence,
				'aggregate_kind','context_pack',
				'aggregate_id',p_context_pack_id,
				'revision',pack_revision,
				'event_kind','context_pack_persisted',
				'payload',payload
			)
		);
		INSERT INTO decodex.outbox(
			effect_key,aggregate_kind,aggregate_id,
			aggregate_revision,payload
		) VALUES (
			'activity/'||activity_sequence::text,
			'context_pack',p_context_pack_id::text,pack_revision,
			pg_catalog.jsonb_build_object(
				'activity_sequence',activity_sequence,
				'event_kind','context_pack_persisted',
				'aggregate_kind','context_pack',
				'aggregate_id',p_context_pack_id,
				'revision',pack_revision,
				'payload',payload
			)
		) RETURNING id INTO outbox_id;
		outbox_rows:=outbox_rows||pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object(
				'id',outbox_id,
				'effect_key','activity/'||activity_sequence::text,
				'aggregate_kind','context_pack',
				'aggregate_id',p_context_pack_id,
				'aggregate_revision',pack_revision
			)
		);
		payload:=pg_catalog.jsonb_build_object(
			'kind','runtime_session',
			'continuation_plan_id',p_plan_id,
			'routing_decision_id',p_decision_id,
			'fallback_runtime_session_id',p_fallback_session_id,
			'conversation_id',session_row.conversation_id,
			'selected_account_id',decision_row.selected_account_id,
			'state','starting',
			'revision',1,
			'dispatch_enabled',false
		);
		INSERT INTO decodex.activity(
			aggregate_kind,aggregate_id,revision,event_kind,
			correlation_key,payload
		) VALUES (
			'runtime_session',p_fallback_session_id::text,1,
			'runtime_session_created',p_idempotency_key,payload
		) RETURNING sequence INTO activity_sequence;
		activity_rows:=activity_rows||pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object(
				'sequence',activity_sequence,
				'aggregate_kind','runtime_session',
				'aggregate_id',p_fallback_session_id,
				'revision',1,
				'event_kind','runtime_session_created',
				'payload',payload
			)
		);
		INSERT INTO decodex.outbox(
			effect_key,aggregate_kind,aggregate_id,
			aggregate_revision,payload
		) VALUES (
			'activity/'||activity_sequence::text,
			'runtime_session',p_fallback_session_id::text,1,
			pg_catalog.jsonb_build_object(
				'activity_sequence',activity_sequence,
				'event_kind','runtime_session_created',
				'aggregate_kind','runtime_session',
				'aggregate_id',p_fallback_session_id,
				'revision',1,
				'payload',payload
			)
		) RETURNING id INTO outbox_id;
		outbox_rows:=outbox_rows||pg_catalog.jsonb_build_array(
			pg_catalog.jsonb_build_object(
				'id',outbox_id,
				'effect_key','activity/'||activity_sequence::text,
				'aggregate_kind','runtime_session',
				'aggregate_id',p_fallback_session_id,
				'aggregate_revision',1
			)
		);
	END IF;

	effect:=core||pg_catalog.jsonb_build_object(
		'activity_effects',activity_rows,
		'outbox_effects',outbox_rows,
		'effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
			public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),
			'hex'
		)
	);
	response:=pg_catalog.convert_to(
		pg_catalog.jsonb_build_object(
			'classification','success',
			'effect',effect
		)::text,
		'UTF8'
	);
	INSERT INTO decodex.continuation_plans(
		plan_id,operation_id,routing_decision_id,consumer_kind,
		consumer_conversation_id,conversation_revision,turn_id,
		managed_run_id,managed_run_revision,managed_execution_id,
		conversation_id,source_runtime_session_id,
		source_runtime_session_revision,selected_account_id,kind,
		codex_thread_id,fallback_context_pack_id,
		fallback_runtime_session_id,routing_evidence_id,
		routing_evidence_revision,schema_fingerprint,codex_experiment_id,
		codex_experiment_revision,codex_observation_id,
		same_thread_provider_attempt_id,
		same_thread_provider_attempt_revision,
		same_thread_provider_evidence_id,replay_permitted,
		dispatch_enabled,revision,request_envelope,effect_envelope,
		response_bytes,planned_at
	) VALUES (
		p_plan_id,p_operation_id,p_decision_id,
		decision_row.consumer_kind,decision_row.conversation_id,
		decision_row.conversation_revision,decision_row.turn_id,
		decision_row.managed_run_id,decision_row.managed_run_revision,
		decision_row.managed_execution_id,session_row.conversation_id,
		session_row.runtime_session_id,session_row.revision,
		decision_row.selected_account_id,plan_kind,thread_value,
		CASE WHEN plan_kind='context_pack_fallback'
			THEN p_context_pack_id END,
		CASE WHEN plan_kind='context_pack_fallback'
			THEN p_fallback_session_id END,
		selected_evidence_id,selected_evidence_revision,
		selected_schema_fingerprint,selected_experiment_id,
		selected_experiment_revision,selected_observation_id,
		selected_provider_attempt_id,selected_provider_attempt_revision,
		selected_provider_evidence_id,false,false,1,request,effect,response,planned
	);
	UPDATE decodex.exact_command_receipts
	SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol
		AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE OR REPLACE FUNCTION decodex.enforce_routing_decision_completeness()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex AS $$
DECLARE member_count bigint; quota_count bigint; capability_count bigint;
DECLARE blocker_count bigint; included_count bigint; exclusion_count bigint;
BEGIN
	SELECT pg_catalog.count(*) INTO member_count
	FROM decodex.routing_decision_member_refs WHERE decision_id=NEW.decision_id;
	SELECT pg_catalog.count(*) INTO quota_count
	FROM decodex.routing_decision_quota_refs WHERE decision_id=NEW.decision_id;
	SELECT pg_catalog.count(*) INTO capability_count
	FROM decodex.routing_decision_capability_refs WHERE decision_id=NEW.decision_id;
	SELECT pg_catalog.count(*) INTO blocker_count
	FROM decodex.routing_decision_blocker_refs WHERE decision_id=NEW.decision_id;
	SELECT pg_catalog.count(*) INTO included_count
	FROM decodex.routing_decision_member_refs AS reference
	JOIN decodex.routing_snapshot_members AS member
		ON member.snapshot_id=reference.snapshot_id
		AND member.account_id=reference.account_id
	WHERE reference.decision_id=NEW.decision_id
		AND member.disposition='included';
	SELECT pg_catalog.count(*) INTO exclusion_count
	FROM decodex.routing_decision_exclusions WHERE decision_id=NEW.decision_id;

	IF NOT EXISTS (
		SELECT 1 FROM decodex.routing_snapshots AS snapshot
		WHERE snapshot.snapshot_id=NEW.snapshot_id
			AND (
				snapshot.consumer_kind,
				snapshot.conversation_id,
				snapshot.conversation_revision,
				snapshot.turn_id,
				snapshot.managed_run_id,
				snapshot.managed_run_revision,
				snapshot.managed_execution_id
			) IS NOT DISTINCT FROM (
				NEW.consumer_kind,
				NEW.conversation_id,
				NEW.conversation_revision,
				NEW.turn_id,
				NEW.managed_run_id,
				NEW.managed_run_revision,
				NEW.managed_execution_id
			)
	) OR member_count=0
		OR quota_count<>member_count*2
		OR capability_count<>member_count*8
		OR EXISTS (
			SELECT position,account_id
			FROM decodex.routing_snapshot_members
			WHERE snapshot_id=NEW.snapshot_id
			EXCEPT
			SELECT position,account_id
			FROM decodex.routing_decision_member_refs
			WHERE decision_id=NEW.decision_id
		)
		OR EXISTS (
			SELECT position,account_id
			FROM decodex.routing_decision_member_refs
			WHERE decision_id=NEW.decision_id
			EXCEPT
			SELECT position,account_id
			FROM decodex.routing_snapshot_members
			WHERE snapshot_id=NEW.snapshot_id
		)
		OR EXISTS (
			SELECT 1
			FROM decodex.routing_decision_member_refs AS reference
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=reference.snapshot_id
				AND member.account_id=reference.account_id
			WHERE reference.decision_id=NEW.decision_id
				AND (
					(
						member.disposition='excluded'
						AND NOT EXISTS (
							SELECT 1
							FROM decodex.routing_decision_blocker_refs AS blocker
							WHERE blocker.decision_id=reference.decision_id
								AND blocker.account_id=reference.account_id
								AND blocker.blocker='excluded_by_policy'
						)
					) OR (
						member.disposition='included'
						AND EXISTS (
							SELECT 1
							FROM decodex.routing_decision_blocker_refs AS blocker
							WHERE blocker.decision_id=reference.decision_id
								AND blocker.account_id=reference.account_id
								AND blocker.blocker='excluded_by_policy'
						)
					)
				)
		)
		OR EXISTS (
			SELECT blocker.account_id,blocker.position,blocker.blocker
			FROM decodex.routing_snapshot_blockers AS blocker
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=blocker.snapshot_id
				AND member.account_id=blocker.account_id
			WHERE blocker.snapshot_id=NEW.snapshot_id
				AND member.disposition='excluded'
			EXCEPT
			SELECT blocker.account_id,blocker.position,blocker.blocker
			FROM decodex.routing_decision_blocker_refs AS blocker
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=blocker.snapshot_id
				AND member.account_id=blocker.account_id
			WHERE blocker.decision_id=NEW.decision_id
				AND member.disposition='excluded'
		)
		OR EXISTS (
			SELECT blocker.account_id,blocker.position,blocker.blocker
			FROM decodex.routing_decision_blocker_refs AS blocker
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=blocker.snapshot_id
				AND member.account_id=blocker.account_id
			WHERE blocker.decision_id=NEW.decision_id
				AND member.disposition='excluded'
			EXCEPT
			SELECT blocker.account_id,blocker.position,blocker.blocker
			FROM decodex.routing_snapshot_blockers AS blocker
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=blocker.snapshot_id
				AND member.account_id=blocker.account_id
			WHERE blocker.snapshot_id=NEW.snapshot_id
				AND member.disposition='excluded'
		)
	THEN
		RAISE EXCEPTION 'routing decision evidence is incomplete or cross-linked'
			USING ERRCODE='23514',CONSTRAINT='routing_decision_complete';
	END IF;

	IF NEW.kind::text='selected' THEN
		IF NOT EXISTS (
			SELECT 1
			FROM decodex.routing_decision_member_refs AS reference
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=reference.snapshot_id
				AND member.account_id=reference.account_id
			WHERE reference.decision_id=NEW.decision_id
				AND reference.account_id=NEW.selected_account_id
				AND member.disposition='included'
				AND NOT EXISTS (
					SELECT 1 FROM decodex.routing_decision_blocker_refs AS blocker
					WHERE blocker.decision_id=NEW.decision_id
						AND blocker.account_id=reference.account_id
				)
		) THEN
			RAISE EXCEPTION 'selected route is not independently eligible'
				USING ERRCODE='23514',CONSTRAINT='routing_decision_complete';
		END IF;
	ELSIF NEW.kind::text='waiting_usage' THEN
		IF included_count=0 OR EXISTS (
			SELECT 1
			FROM decodex.routing_decision_member_refs AS reference
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=reference.snapshot_id
				AND member.account_id=reference.account_id
			LEFT JOIN decodex.routing_decision_blocker_refs AS blocker
				ON blocker.decision_id=reference.decision_id
				AND blocker.account_id=reference.account_id
			WHERE reference.decision_id=NEW.decision_id
				AND member.disposition='included'
			GROUP BY reference.account_id
			HAVING pg_catalog.count(blocker.blocker)=0
				OR pg_catalog.bool_or(
					blocker.blocker::text NOT IN (
						'quota_five_hour_depleted','quota_seven_day_depleted'
					)
				)
		) OR exclusion_count=0 THEN
			RAISE EXCEPTION 'waiting_usage is not pure positive quota depletion'
				USING ERRCODE='23514',CONSTRAINT='routing_decision_complete';
		END IF;
	ELSIF NEW.kind::text='waiting_reconciliation' THEN
		IF included_count=0 OR exclusion_count<>0 OR EXISTS (
			SELECT 1
			FROM decodex.routing_decision_member_refs AS reference
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=reference.snapshot_id
				AND member.account_id=reference.account_id
			LEFT JOIN decodex.routing_decision_blocker_refs AS blocker
				ON blocker.decision_id=reference.decision_id
				AND blocker.account_id=reference.account_id
			WHERE reference.decision_id=NEW.decision_id
				AND member.disposition='included'
			GROUP BY reference.account_id
			HAVING pg_catalog.count(blocker.blocker)=0
				OR pg_catalog.bool_or(
					blocker.blocker::text NOT IN (
						'process_generation_unresolved',
						'provider_attempt_unresolved'
					)
				)
		) THEN
			RAISE EXCEPTION 'waiting_reconciliation is not a pure unresolved authority'
				USING ERRCODE='23514',CONSTRAINT='routing_decision_complete';
		END IF;
	ELSIF NEW.kind::text='no_route' THEN
		IF blocker_count=0 THEN
			RAISE EXCEPTION 'NoRoute has no exact blocker cause'
				USING ERRCODE='23514',CONSTRAINT='routing_decision_complete';
		END IF;
	END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.route_account_exact(
	p_protocol text,
	p_idempotency_key text,
	p_operation_id uuid,
	p_routing_policy_id uuid,
	p_expected_routing_policy_revision bigint,
	p_consumer_kind decodex.provider_attempt_consumer_kind,
	p_conversation_id uuid,
	p_expected_conversation_revision bigint,
	p_source_runtime_session_id uuid,
	p_expected_source_runtime_session_revision bigint,
	p_turn_id uuid,
	p_managed_run_id uuid,
	p_expected_managed_run_revision bigint,
	p_managed_execution_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, decodex AS $$
DECLARE request jsonb; replay bytea; snapshot_row decodex.routing_snapshots%ROWTYPE;
DECLARE run_row decodex.managed_runs%ROWTYPE; selected_account uuid;
DECLARE selected_position integer; decided timestamptz; decided_micros bigint;
DECLARE decision_kind text; no_route_value text; ready_micros bigint;
DECLARE decision_uuid uuid; core jsonb; effect jsonb; response bytea;
BEGIN
	request:=pg_catalog.jsonb_build_object(
		'operation','route_account',
		'protocol',p_protocol,
		'operation_id',p_operation_id,
		'routing_policy_id',p_routing_policy_id,
		'expected_routing_policy_revision',p_expected_routing_policy_revision,
		'consumer_kind',p_consumer_kind,
		'conversation_id',p_conversation_id,
		'conversation_revision',p_expected_conversation_revision,
		'source_runtime_session_id',p_source_runtime_session_id,
		'source_runtime_session_revision',p_expected_source_runtime_session_revision,
		'turn_id',p_turn_id,
		'managed_run_id',p_managed_run_id,
		'managed_run_revision',p_expected_managed_run_revision,
		'managed_execution_id',p_managed_execution_id
	);
	replay:=decodex.reserve_exact_routing_command(
		p_protocol,p_idempotency_key,request
	);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL
		OR p_routing_policy_id IS NULL
		OR p_expected_routing_policy_revision IS NULL
		OR p_expected_routing_policy_revision<=0
		OR NOT COALESCE((
			(
				p_consumer_kind::text='conversation_turn'
				AND p_conversation_id IS NOT NULL
				AND p_expected_conversation_revision>0
				AND p_source_runtime_session_id IS NOT NULL
				AND p_expected_source_runtime_session_revision>0
				AND p_turn_id IS NOT NULL
				AND p_managed_run_id IS NULL
				AND p_expected_managed_run_revision IS NULL
				AND p_managed_execution_id IS NULL
			) OR (
				p_consumer_kind::text='managed_run_execution'
				AND p_conversation_id IS NULL
				AND p_expected_conversation_revision IS NULL
				AND p_source_runtime_session_id IS NULL
				AND p_expected_source_runtime_session_revision IS NULL
				AND p_turn_id IS NULL
				AND p_managed_run_id IS NOT NULL
				AND p_expected_managed_run_revision>0
				AND p_managed_execution_id IS NOT NULL
			)
		),false)
	THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account','malformed_input'
		);
	END IF;

	PERFORM pg_catalog.pg_advisory_xact_lock_shared(1400);
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1338,
		pg_catalog.hashtext(COALESCE(p_conversation_id::text,p_managed_run_id::text))
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(1356);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1359,pg_catalog.hashtext(p_operation_id::text)
	);
	PERFORM pg_catalog.pg_advisory_xact_lock(
		1401,
		pg_catalog.hashtext(
			p_consumer_kind::text||':'||
			COALESCE(p_conversation_id::text,p_managed_run_id::text)
		)
	);
	LOCK TABLE decodex.accounts,decodex.quota_windows,
		decodex.routing_policy_heads,decodex.policies,decodex.policy_revisions,
		decodex.role_profiles,decodex.role_profile_revisions,
		decodex.profile_snapshots,decodex.account_snapshots,
		decodex.runtime_sessions,decodex.conversations,
		decodex.routing_policy_revisions,decodex.routing_policy_members,
		decodex.routing_policy_required_capabilities,
		decodex.routing_compatibility_evidence,
		decodex.routing_capability_evidence,decodex.routing_snapshots,
		decodex.routing_snapshot_members,decodex.routing_snapshot_quota_facts,
		decodex.routing_snapshot_capability_facts,
		decodex.routing_snapshot_blockers,decodex.process_generations,
		decodex.process_generation_execution_epochs,decodex.provider_attempts
		IN SHARE MODE;

	PERFORM 1 FROM decodex.routing_policy_heads
	WHERE routing_policy_id=p_routing_policy_id
		AND current_revision=p_expected_routing_policy_revision
	FOR SHARE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account','stale_routing_policy'
		);
	END IF;
	IF p_consumer_kind::text='conversation_turn' THEN
		PERFORM 1 FROM decodex.conversations
		WHERE conversation_id=p_conversation_id
			AND revision=p_expected_conversation_revision
			AND status='open'
		FOR SHARE;
		IF NOT FOUND OR NOT EXISTS (
			SELECT 1 FROM decodex.runtime_sessions
			WHERE runtime_session_id=p_source_runtime_session_id
				AND revision=p_expected_source_runtime_session_revision
				AND conversation_id=p_conversation_id
			FOR SHARE
		) THEN
			RETURN decodex.complete_exact_routing_rejection(
				p_protocol,p_idempotency_key,'route_account','stale_consumer'
			);
		END IF;
	ELSE
		SELECT * INTO run_row FROM decodex.managed_runs
		WHERE managed_run_id=p_managed_run_id
			AND revision=p_expected_managed_run_revision
		FOR SHARE;
		IF NOT FOUND THEN
			RETURN decodex.complete_exact_routing_rejection(
				p_protocol,p_idempotency_key,'route_account','stale_consumer'
			);
		END IF;
	END IF;

	SELECT snapshot.* INTO snapshot_row
	FROM decodex.routing_snapshots AS snapshot
	WHERE snapshot.routing_policy_id=p_routing_policy_id
		AND snapshot.routing_policy_revision=p_expected_routing_policy_revision
		AND (
			snapshot.consumer_kind,
			snapshot.conversation_id,
			snapshot.conversation_revision,
			snapshot.turn_id,
			snapshot.managed_run_id,
			snapshot.managed_run_revision,
			snapshot.managed_execution_id
		) IS NOT DISTINCT FROM (
			p_consumer_kind,
			p_conversation_id,
			p_expected_conversation_revision,
			p_turn_id,
			p_managed_run_id,
			p_expected_managed_run_revision,
			p_managed_execution_id
		)
		AND (
			p_consumer_kind::text='managed_run_execution'
			OR (
				snapshot.runtime_session_id=p_source_runtime_session_id
				AND snapshot.runtime_session_revision=
					p_expected_source_runtime_session_revision
			)
		)
	ORDER BY snapshot.resolved_at DESC,snapshot.snapshot_id
	LIMIT 1 FOR SHARE;
	IF NOT FOUND THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account','snapshot_missing'
		);
	END IF;
	IF NOT EXISTS (
		SELECT 1 FROM decodex.policies
		WHERE policy_id=snapshot_row.accepted_policy_id
			AND current_revision=snapshot_row.accepted_policy_revision
		FOR SHARE
	) OR NOT EXISTS (
		SELECT 1 FROM decodex.role_profiles
		WHERE role=snapshot_row.required_role
			AND current_revision=snapshot_row.required_role_profile_revision
		FOR SHARE
	) OR NOT EXISTS (
		SELECT 1 FROM decodex.runtime_sessions
		WHERE runtime_session_id=snapshot_row.runtime_session_id
			AND revision=snapshot_row.runtime_session_revision
		FOR SHARE
	) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account',
			'concurrent_authority_change'
		);
	END IF;

	decided:=pg_catalog.clock_timestamp();
	decided_micros:=(extract(epoch FROM decided)*1000000)::bigint;
	IF EXISTS (
		SELECT account_id,revision FROM decodex.accounts
		EXCEPT
		SELECT account_id,account_revision
		FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
	) OR EXISTS (
		SELECT account_id,account_revision
		FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
		EXCEPT
		SELECT account_id,revision FROM decodex.accounts
	) OR EXISTS (
		SELECT position,account_id,account_revision,disposition
		FROM decodex.routing_policy_members
		WHERE routing_policy_id=p_routing_policy_id
			AND routing_policy_revision=p_expected_routing_policy_revision
		EXCEPT
		SELECT position,account_id,account_revision,disposition
		FROM decodex.routing_snapshot_members
		WHERE snapshot_id=snapshot_row.snapshot_id
	) OR EXISTS (
		SELECT 1
		FROM decodex.routing_snapshot_members AS member
		LEFT JOIN decodex.accounts AS account
			ON account.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND (
				account.account_id IS NULL
				OR account.revision<>member.account_revision
				OR account.state<>member.account_state
				OR decodex.rfc3339_utc(account.observed_at)<>
					member.account_observed_at_utc
				OR account.observed_at>decided
				OR decided-account.observed_at>INTERVAL '300 seconds'
			)
	) OR EXISTS (
		SELECT 1
		FROM decodex.routing_snapshot_members AS member
		LEFT JOIN LATERAL (
			SELECT evidence_id,evidence_revision,ingested_at
			FROM decodex.routing_compatibility_evidence
			WHERE account_id=member.account_id
			ORDER BY evidence_revision DESC LIMIT 1
		) AS current_evidence ON true
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND (
				member.evidence_id IS DISTINCT FROM current_evidence.evidence_id
				OR member.evidence_revision IS DISTINCT FROM
					current_evidence.evidence_revision
				OR (
					member.evidence_id IS NOT NULL
					AND (
						current_evidence.ingested_at>decided
						OR decided-current_evidence.ingested_at>
							INTERVAL '300 seconds'
					)
				)
			)
	) OR EXISTS (
		SELECT 1
		FROM decodex.routing_snapshot_quota_facts AS fact
		LEFT JOIN decodex.quota_windows AS quota
			ON quota.account_id=fact.account_id
			AND quota.window_class=fact.window_class
			AND quota.duration_minutes=fact.duration_minutes
		WHERE fact.snapshot_id=snapshot_row.snapshot_id
			AND (
				(fact.observation_revision IS NULL)<>(quota.account_id IS NULL)
				OR fact.observation_revision IS DISTINCT FROM quota.revision
				OR fact.remaining_percent IS DISTINCT FROM quota.remaining_percent
				OR fact.observed_at_micros IS DISTINCT FROM
					(extract(epoch FROM quota.observed_at)*1000000)::bigint
				OR fact.resets_at_micros IS DISTINCT FROM
					(extract(epoch FROM quota.resets_at)*1000000)::bigint
				OR fact.confidence IS DISTINCT FROM quota.confidence
			)
	) THEN
		RETURN decodex.complete_exact_routing_rejection(
			p_protocol,p_idempotency_key,'route_account',
			'concurrent_authority_change'
		);
	END IF;

	decision_uuid:=pg_catalog.gen_random_uuid();
	INSERT INTO decodex.routing_decision_member_refs(
		decision_id,snapshot_id,position,account_id
	)
	SELECT decision_uuid,snapshot_id,position,account_id
	FROM decodex.routing_snapshot_members
	WHERE snapshot_id=snapshot_row.snapshot_id
	ORDER BY position;
	INSERT INTO decodex.routing_decision_quota_refs(
		decision_id,snapshot_id,account_id,position,window_class,
		duration_minutes,observation_revision,remaining_percent,
		observed_at_micros,resets_at_micros,confidence,source_id,
		timestamp_precision,raw_observed_at,raw_resets_at
	)
	SELECT decision_uuid,fact.snapshot_id,fact.account_id,fact.position,
		fact.window_class,fact.duration_minutes,fact.observation_revision,
		fact.remaining_percent,fact.observed_at_micros,fact.resets_at_micros,
		fact.confidence,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=
				fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN quota.metadata->>'source_id' END,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=
				fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN 'unix_microsecond' END,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=
				fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN quota.metadata->>'raw_observed_at' END,
		CASE WHEN quota.metadata->>'timestamp_precision'='unix_microsecond'
			AND quota.metadata->>'evidence_revision'=
				fact.observation_revision::text
			AND quota.metadata->>'source_id'<>''
			AND pg_catalog.octet_length(quota.metadata->>'source_id')<=256
			AND NOT decodex.has_credential_material(quota.metadata->>'source_id')
			AND quota.metadata->>'raw_observed_at'=fact.observed_at_micros::text
			AND quota.metadata->>'raw_resets_at'=fact.resets_at_micros::text
			THEN quota.metadata->>'raw_resets_at' END
	FROM decodex.routing_snapshot_quota_facts AS fact
	LEFT JOIN decodex.quota_windows AS quota
		ON quota.account_id=fact.account_id
		AND quota.window_class=fact.window_class
		AND quota.duration_minutes=fact.duration_minutes
	WHERE fact.snapshot_id=snapshot_row.snapshot_id
	ORDER BY fact.account_id,fact.position;
	INSERT INTO decodex.routing_decision_capability_refs
	SELECT decision_uuid,snapshot_id,account_id,position,capability,
		applicable,evidence_state
	FROM decodex.routing_snapshot_capability_facts
	WHERE snapshot_id=snapshot_row.snapshot_id
	ORDER BY account_id,position;
	INSERT INTO decodex.routing_decision_blocker_refs
	SELECT decision_uuid,blocker.snapshot_id,blocker.account_id,
		blocker.position,blocker.blocker
	FROM decodex.routing_snapshot_blockers AS blocker
	JOIN decodex.routing_snapshot_members AS member
		ON member.snapshot_id=blocker.snapshot_id
		AND member.account_id=blocker.account_id
	WHERE blocker.snapshot_id=snapshot_row.snapshot_id
		AND (
			member.disposition='excluded'
			OR blocker.blocker::text NOT IN (
				'quota_five_hour_missing','quota_five_hour_from_future',
				'quota_five_hour_stale','quota_five_hour_unknown',
				'quota_five_hour_reset_elapsed','quota_five_hour_depleted',
				'quota_seven_day_missing','quota_seven_day_from_future',
				'quota_seven_day_stale','quota_seven_day_unknown',
				'quota_seven_day_reset_elapsed','quota_seven_day_depleted'
			)
		)
	ORDER BY blocker.account_id,blocker.position;

	-- V16 classifies both quota windows again at its own database-authored instant. A fact that
	-- was current at V14 can become stale or reset-expired before V16 commits. Retain that exact
	-- duration-typed cause instead of producing an empty or falsely pure projection.
	WITH classified AS (
		SELECT fact.account_id,fact.position AS quota_position,
			CASE
				WHEN fact.observation_revision IS NULL THEN CASE fact.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_missing'::decodex.routing_blocker
					ELSE 'quota_seven_day_missing'::decodex.routing_blocker
				END
				WHEN fact.observed_at_micros IS NULL
					OR fact.remaining_percent IS NULL
					OR fact.confidence IS DISTINCT FROM 'high'
				THEN CASE fact.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_unknown'::decodex.routing_blocker
					ELSE 'quota_seven_day_unknown'::decodex.routing_blocker
				END
				WHEN fact.observed_at_micros>decided_micros THEN CASE fact.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_from_future'::decodex.routing_blocker
					ELSE 'quota_seven_day_from_future'::decodex.routing_blocker
				END
				WHEN decided_micros-fact.observed_at_micros>300000000 THEN
					CASE fact.window_class
						WHEN 'five_hour' THEN
							'quota_five_hour_stale'::decodex.routing_blocker
						ELSE 'quota_seven_day_stale'::decodex.routing_blocker
					END
				WHEN fact.resets_at_micros IS NULL
					OR fact.resets_at_micros<=decided_micros
				THEN CASE fact.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_reset_elapsed'::decodex.routing_blocker
					ELSE 'quota_seven_day_reset_elapsed'::decodex.routing_blocker
				END
				WHEN fact.remaining_percent=0 THEN CASE fact.window_class
					WHEN 'five_hour' THEN
						'quota_five_hour_depleted'::decodex.routing_blocker
					ELSE 'quota_seven_day_depleted'::decodex.routing_blocker
				END
			END AS blocker
		FROM decodex.routing_decision_quota_refs AS fact
		JOIN decodex.routing_snapshot_members AS member
			ON member.snapshot_id=fact.snapshot_id
			AND member.account_id=fact.account_id
		WHERE fact.decision_id=decision_uuid
			AND member.disposition='included'
	),
	novel AS (
		SELECT classified.account_id,classified.quota_position,classified.blocker
		FROM classified
		WHERE classified.blocker IS NOT NULL
			AND NOT EXISTS (
				SELECT 1
				FROM decodex.routing_decision_blocker_refs AS existing
				WHERE existing.decision_id=decision_uuid
					AND existing.account_id=classified.account_id
					AND existing.blocker=classified.blocker
			)
	),
	positioned AS (
		SELECT novel.*,
			(COALESCE((
				SELECT pg_catalog.max(existing.position)
				FROM decodex.routing_decision_blocker_refs AS existing
				WHERE existing.decision_id=decision_uuid
					AND existing.account_id=novel.account_id
			),0)+pg_catalog.row_number() OVER (
				PARTITION BY novel.account_id ORDER BY novel.quota_position
			))::integer AS blocker_position
		FROM novel
	)
	INSERT INTO decodex.routing_decision_blocker_refs(
		decision_id,snapshot_id,account_id,position,blocker
	)
	SELECT decision_uuid,snapshot_row.snapshot_id,account_id,
		blocker_position,blocker
	FROM positioned
	ORDER BY account_id,blocker_position;

	-- Preserve ManagedRun domain blockers without allowing them to impersonate positive quota.
	IF p_consumer_kind::text='managed_run_execution'
		AND run_row.lifecycle='waiting'
	THEN
		INSERT INTO decodex.routing_decision_blocker_refs(
			decision_id,snapshot_id,account_id,position,blocker
		)
		SELECT decision_uuid,snapshot_row.snapshot_id,member.account_id,
			COALESCE((
				SELECT pg_catalog.max(existing.position)
				FROM decodex.routing_decision_blocker_refs AS existing
				WHERE existing.decision_id=decision_uuid
					AND existing.account_id=member.account_id
			),0)+1,
			cause.blocker
		FROM decodex.routing_snapshot_members AS member
		CROSS JOIN LATERAL (
			SELECT CASE run_row.wait_reason::text
				WHEN 'usage' THEN CASE WHEN EXISTS (
					SELECT 1
					FROM decodex.routing_snapshot_blockers AS quota_blocker
					WHERE quota_blocker.snapshot_id=member.snapshot_id
						AND quota_blocker.account_id=member.account_id
						AND quota_blocker.blocker::text IN (
							'quota_five_hour_depleted',
							'quota_seven_day_depleted'
						)
				) THEN NULL
				ELSE 'usage_unproven'::decodex.routing_blocker END
				WHEN 'auth' THEN
					'authentication_required'::decodex.routing_blocker
				WHEN 'plugin' THEN 'plugin_unready'::decodex.routing_blocker
				WHEN 'dependency' THEN
					'dependency_blocked'::decodex.routing_blocker
				WHEN 'approval' THEN
					'approval_required'::decodex.routing_blocker
				WHEN 'user' THEN 'user_required'::decodex.routing_blocker
				WHEN 'external' THEN 'external_blocked'::decodex.routing_blocker
				WHEN 'reconciliation' THEN CASE
					WHEN EXISTS (
						SELECT 1
						FROM decodex.provider_attempts AS attempt
						WHERE (
							(
								p_consumer_kind::text='conversation_turn'
								AND attempt.consumer_kind::text='conversation_turn'
								AND attempt.conversation_id=p_conversation_id
								AND attempt.turn_id=p_turn_id
							) OR (
								p_consumer_kind::text='managed_run_execution'
								AND attempt.consumer_kind::text=
									'managed_run_execution'
								AND attempt.managed_run_id=p_managed_run_id
								AND attempt.managed_execution_id=
									p_managed_execution_id
							)
						)
					) OR EXISTS (
						SELECT 1
						FROM decodex.process_generations AS generation
						WHERE generation.account_id=member.account_id
							AND generation.state<>'dead'
							AND NOT (
								generation.state='ready'
								AND generation.process_id IS NOT NULL
								AND EXISTS (
									SELECT 1
									FROM decodex.process_generation_execution_epochs
										AS epoch
									WHERE epoch.execution_epoch_id=
										generation.execution_epoch_id
										AND epoch.retired_at IS NULL
								)
							)
					) THEN NULL
					ELSE 'reconciliation_unproven'::decodex.routing_blocker
				END
				WHEN 'reviewer_unavailable' THEN
					'reviewer_unavailable'::decodex.routing_blocker
				WHEN 'reviewer_failed' THEN
					'reviewer_failed'::decodex.routing_blocker
				WHEN 'reviewer_ambiguous' THEN
					'reviewer_ambiguous'::decodex.routing_blocker
				ELSE NULL
			END AS blocker
		) AS cause
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND member.disposition='included'
			AND cause.blocker IS NOT NULL;
		END IF;

	-- V14 owns the quota values. V16 also requires the exact timestamp provenance that turns a
	-- current positive value into selectable evidence. Preserve missing provenance as a cause.
	INSERT INTO decodex.routing_decision_blocker_refs(
		decision_id,snapshot_id,account_id,position,blocker
	)
	SELECT decision_uuid,snapshot_row.snapshot_id,member.account_id,
		COALESCE((
			SELECT pg_catalog.max(existing.position)
			FROM decodex.routing_decision_blocker_refs AS existing
			WHERE existing.decision_id=decision_uuid
				AND existing.account_id=member.account_id
		),0)+1,
		'usage_unproven'::decodex.routing_blocker
	FROM decodex.routing_snapshot_members AS member
	WHERE member.snapshot_id=snapshot_row.snapshot_id
		AND member.disposition='included'
		AND EXISTS (
			SELECT 1
			FROM decodex.routing_decision_quota_refs AS fact
			WHERE fact.decision_id=decision_uuid
				AND fact.account_id=member.account_id
				AND fact.observation_revision IS NOT NULL
				AND fact.remaining_percent IS NOT NULL
				AND fact.confidence='high'
				AND fact.observed_at_micros<=decided_micros
				AND decided_micros-fact.observed_at_micros<=300000000
				AND fact.resets_at_micros>decided_micros
				AND fact.source_id IS NULL
		)
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.routing_decision_blocker_refs AS existing
			WHERE existing.decision_id=decision_uuid
				AND existing.account_id=member.account_id
				AND existing.blocker='usage_unproven'
		);

	-- An existing exact intent is blocked at that intent only. Unrelated Conversation turns and
	-- ManagedRun executions do not inherit its ambiguity. Every route for the same exact intent
	-- remains blocked because changing accounts would still replay that intent.
	INSERT INTO decodex.routing_decision_blocker_refs(
		decision_id,snapshot_id,account_id,position,blocker
	)
	SELECT decision_uuid,snapshot_row.snapshot_id,member.account_id,
		COALESCE((
			SELECT pg_catalog.max(existing.position)
			FROM decodex.routing_decision_blocker_refs AS existing
			WHERE existing.decision_id=decision_uuid
				AND existing.account_id=member.account_id
		),0)+1,
		CASE WHEN EXISTS (
			SELECT 1 FROM decodex.provider_attempts AS unresolved
			WHERE unresolved.state IN (
					'prepared','dispatch_authorized','unknown'
				)
				AND (
					(
						p_consumer_kind::text='conversation_turn'
						AND unresolved.consumer_kind::text='conversation_turn'
						AND unresolved.conversation_id=p_conversation_id
						AND unresolved.turn_id=p_turn_id
					) OR (
						p_consumer_kind::text='managed_run_execution'
						AND unresolved.consumer_kind::text='managed_run_execution'
						AND unresolved.managed_run_id=p_managed_run_id
						AND unresolved.managed_execution_id=p_managed_execution_id
					)
				)
		) THEN 'provider_attempt_unresolved'::decodex.routing_blocker
		ELSE 'provider_attempt_completed'::decodex.routing_blocker END
	FROM decodex.routing_snapshot_members AS member
	WHERE member.snapshot_id=snapshot_row.snapshot_id
		AND member.disposition='included'
		AND EXISTS (
			SELECT 1 FROM decodex.provider_attempts AS attempt
			WHERE (
					(
						p_consumer_kind::text='conversation_turn'
						AND attempt.consumer_kind::text='conversation_turn'
						AND attempt.conversation_id=p_conversation_id
						AND attempt.turn_id=p_turn_id
					) OR (
						p_consumer_kind::text='managed_run_execution'
						AND attempt.consumer_kind::text='managed_run_execution'
						AND attempt.managed_run_id=p_managed_run_id
						AND attempt.managed_execution_id=p_managed_execution_id
					)
				)
		);

	INSERT INTO decodex.routing_decision_blocker_refs(
		decision_id,snapshot_id,account_id,position,blocker
	)
	SELECT decision_uuid,snapshot_row.snapshot_id,member.account_id,
		COALESCE((
			SELECT pg_catalog.max(existing.position)
			FROM decodex.routing_decision_blocker_refs AS existing
			WHERE existing.decision_id=decision_uuid
				AND existing.account_id=member.account_id
		),0)+1,
		CASE WHEN EXISTS (
			SELECT 1 FROM decodex.process_generations AS generation
			WHERE generation.account_id=member.account_id
				AND generation.state<>'dead'
		) THEN 'process_generation_unresolved'::decodex.routing_blocker
		ELSE 'process_generation_unavailable'::decodex.routing_blocker END
	FROM decodex.routing_snapshot_members AS member
	WHERE member.snapshot_id=snapshot_row.snapshot_id
		AND member.disposition='included'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.routing_decision_blocker_refs AS existing
			WHERE existing.decision_id=decision_uuid
				AND existing.account_id=member.account_id
				AND existing.blocker::text NOT IN (
					'quota_five_hour_depleted',
					'quota_seven_day_depleted'
				)
		)
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.provider_attempts AS attempt
			WHERE (
				(
					p_consumer_kind::text='conversation_turn'
					AND attempt.consumer_kind::text='conversation_turn'
					AND attempt.conversation_id=p_conversation_id
					AND attempt.turn_id=p_turn_id
				) OR (
					p_consumer_kind::text='managed_run_execution'
					AND attempt.consumer_kind::text='managed_run_execution'
					AND attempt.managed_run_id=p_managed_run_id
					AND attempt.managed_execution_id=p_managed_execution_id
				)
			)
		)
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.process_generations AS generation
			JOIN decodex.process_generation_execution_epochs AS epoch
				ON epoch.execution_epoch_id=generation.execution_epoch_id
			WHERE generation.account_id=member.account_id
				AND generation.state='ready'
				AND generation.process_id IS NOT NULL
				AND epoch.retired_at IS NULL
		);

	SELECT member.position,member.account_id
	INTO selected_position,selected_account
	FROM decodex.routing_snapshot_members AS member
	WHERE member.snapshot_id=snapshot_row.snapshot_id
		AND member.disposition='included'
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.routing_decision_blocker_refs AS blocker
			WHERE blocker.decision_id=decision_uuid
				AND blocker.account_id=member.account_id
		)
		AND NOT EXISTS (
			SELECT 1
			FROM decodex.routing_decision_quota_refs AS fact
			WHERE fact.decision_id=decision_uuid
				AND fact.account_id=member.account_id
				AND (
					fact.observation_revision IS NULL
					OR fact.remaining_percent IS NULL
					OR fact.remaining_percent=0
					OR fact.confidence<>'high'
					OR fact.observed_at_micros>decided_micros
					OR decided_micros-fact.observed_at_micros>300000000
					OR fact.resets_at_micros IS NULL
					OR fact.resets_at_micros<=decided_micros
					OR fact.source_id IS NULL
					OR fact.timestamp_precision<>'unix_microsecond'
				)
		)
		AND (
			SELECT pg_catalog.count(*)
			FROM decodex.routing_decision_blocker_refs AS blocker
			JOIN decodex.routing_snapshot_members AS predecessor
				ON predecessor.snapshot_id=blocker.snapshot_id
				AND predecessor.account_id=blocker.account_id
			WHERE blocker.decision_id=decision_uuid
				AND predecessor.disposition='included'
				AND predecessor.position<member.position
				AND blocker.blocker::text IN (
					'quota_five_hour_depleted',
					'quota_seven_day_depleted'
				)
		)=(
			SELECT pg_catalog.count(*)
			FROM decodex.routing_decision_blocker_refs AS blocker
			JOIN decodex.routing_snapshot_members AS predecessor
				ON predecessor.snapshot_id=blocker.snapshot_id
				AND predecessor.account_id=blocker.account_id
			JOIN decodex.routing_decision_quota_refs AS fact
				ON fact.decision_id=blocker.decision_id
				AND fact.account_id=blocker.account_id
				AND fact.window_class=CASE blocker.blocker::text
					WHEN 'quota_five_hour_depleted'
						THEN 'five_hour'::decodex.quota_window_class
					ELSE 'seven_day'::decodex.quota_window_class
				END
			WHERE blocker.decision_id=decision_uuid
				AND predecessor.disposition='included'
				AND predecessor.position<member.position
				AND blocker.blocker::text IN (
					'quota_five_hour_depleted',
					'quota_seven_day_depleted'
				)
				AND fact.remaining_percent=0
				AND fact.confidence='high'
				AND fact.observed_at_micros<=decided_micros
				AND decided_micros-fact.observed_at_micros<=300000000
				AND fact.resets_at_micros>decided_micros
				AND fact.source_id IS NOT NULL
				AND fact.timestamp_precision='unix_microsecond'
				AND fact.raw_observed_at=fact.observed_at_micros::text
				AND fact.raw_resets_at=fact.resets_at_micros::text
		)
	ORDER BY member.sticky DESC,member.position,member.account_id
	LIMIT 1;
	IF FOUND THEN
		decision_kind:='selected';
	ELSIF EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members
		WHERE snapshot_id=snapshot_row.snapshot_id
			AND disposition='included'
	) AND NOT EXISTS (
		SELECT 1
		FROM decodex.routing_snapshot_members AS member
		LEFT JOIN decodex.routing_decision_blocker_refs AS blocker
			ON blocker.decision_id=decision_uuid
			AND blocker.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND member.disposition='included'
		GROUP BY member.account_id
		HAVING pg_catalog.count(blocker.blocker)=0
			OR pg_catalog.bool_or(
				blocker.blocker::text NOT IN (
					'quota_five_hour_depleted','quota_seven_day_depleted'
				)
			)
	) AND NOT EXISTS (
		SELECT 1
		FROM decodex.routing_snapshot_members AS member
		JOIN decodex.routing_decision_quota_refs AS fact
			ON fact.decision_id=decision_uuid
			AND fact.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND member.disposition='included'
			AND (
				fact.observation_revision IS NULL
				OR fact.remaining_percent IS NULL
				OR fact.confidence<>'high'
				OR fact.observed_at_micros>decided_micros
				OR decided_micros-fact.observed_at_micros>300000000
				OR fact.resets_at_micros IS NULL
				OR fact.resets_at_micros<=decided_micros
				OR fact.source_id IS NULL
				OR fact.timestamp_precision<>'unix_microsecond'
				OR fact.raw_observed_at<>fact.observed_at_micros::text
				OR fact.raw_resets_at<>fact.resets_at_micros::text
			)
	) THEN
		decision_kind:='waiting_usage';
		SELECT pg_catalog.min(account_ready) INTO ready_micros
		FROM (
			SELECT pg_catalog.max(fact.resets_at_micros) AS account_ready
			FROM decodex.routing_snapshot_members AS member
			JOIN decodex.routing_decision_quota_refs AS fact
				ON fact.decision_id=decision_uuid
				AND fact.account_id=member.account_id
			WHERE member.snapshot_id=snapshot_row.snapshot_id
				AND member.disposition='included'
				AND fact.remaining_percent=0
			GROUP BY member.account_id
		) AS readiness;
	ELSIF EXISTS (
		SELECT 1 FROM decodex.routing_snapshot_members
		WHERE snapshot_id=snapshot_row.snapshot_id
			AND disposition='included'
	) AND NOT EXISTS (
		SELECT 1
		FROM decodex.routing_snapshot_members AS member
		LEFT JOIN decodex.routing_decision_blocker_refs AS blocker
			ON blocker.decision_id=decision_uuid
			AND blocker.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND member.disposition='included'
		GROUP BY member.account_id
		HAVING pg_catalog.count(blocker.blocker)=0
			OR pg_catalog.bool_or(
				blocker.blocker::text NOT IN (
					'process_generation_unresolved',
					'provider_attempt_unresolved'
				)
			)
	) THEN
		decision_kind:='waiting_reconciliation';
	ELSE
		decision_kind:='no_route';
		no_route_value:='blocked_evidence';
	END IF;

	IF decision_kind='selected' THEN
		INSERT INTO decodex.routing_decision_exclusions
		SELECT decision_uuid,member.account_id,member.position,quota.window_class,
			quota.duration_minutes,quota.observation_revision,0,quota.observed_at_micros,
			quota.resets_at_micros,quota.confidence,quota.source_id,
			quota.timestamp_precision,quota.raw_observed_at,quota.raw_resets_at,
			'usage_depleted'
		FROM decodex.routing_snapshot_members AS member
		JOIN decodex.routing_decision_quota_refs AS quota
			ON quota.decision_id=decision_uuid
			AND quota.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND member.disposition='included'
			AND member.position<selected_position
			AND quota.remaining_percent=0
			AND quota.confidence='high'
			AND quota.observed_at_micros<=decided_micros
			AND decided_micros-quota.observed_at_micros<=300000000
			AND quota.resets_at_micros>decided_micros
			AND quota.source_id IS NOT NULL
			AND quota.raw_observed_at IS NOT NULL
			AND quota.raw_resets_at IS NOT NULL
			AND EXISTS (
				SELECT 1
				FROM decodex.routing_decision_blocker_refs AS blocker
				WHERE blocker.decision_id=decision_uuid
					AND blocker.account_id=member.account_id
					AND blocker.blocker=CASE quota.window_class
						WHEN 'five_hour' THEN
							'quota_five_hour_depleted'::decodex.routing_blocker
						ELSE 'quota_seven_day_depleted'::decodex.routing_blocker
					END
			)
		ORDER BY member.position,quota.position;
	ELSIF decision_kind='waiting_usage' THEN
		INSERT INTO decodex.routing_decision_exclusions
		SELECT decision_uuid,member.account_id,member.position,quota.window_class,
			quota.duration_minutes,quota.observation_revision,0,quota.observed_at_micros,
			quota.resets_at_micros,quota.confidence,quota.source_id,
			quota.timestamp_precision,quota.raw_observed_at,quota.raw_resets_at,
			'usage_depleted'
		FROM decodex.routing_snapshot_members AS member
		JOIN decodex.routing_decision_quota_refs AS quota
			ON quota.decision_id=decision_uuid
			AND quota.account_id=member.account_id
		WHERE member.snapshot_id=snapshot_row.snapshot_id
			AND member.disposition='included'
			AND quota.remaining_percent=0
		ORDER BY member.position,quota.position;
	END IF;

	INSERT INTO decodex.routing_decisions(
		decision_id,operation_id,snapshot_id,routing_policy_id,
		routing_policy_revision,consumer_kind,conversation_id,
		conversation_revision,turn_id,managed_run_id,managed_run_revision,
		managed_execution_id,kind,selected_account_id,
		waiting_ready_at_micros,no_route_reason,decided_at
	) VALUES (
		decision_uuid,p_operation_id,snapshot_row.snapshot_id,
		p_routing_policy_id,p_expected_routing_policy_revision,p_consumer_kind,
		p_conversation_id,p_expected_conversation_revision,p_turn_id,
		p_managed_run_id,p_expected_managed_run_revision,p_managed_execution_id,
		decision_kind::decodex.routing_decision_kind,selected_account,
		ready_micros,no_route_value::decodex.routing_no_route_reason,decided
	);

	core:=pg_catalog.jsonb_build_object(
		'operation','route_account',
		'decision_id',decision_uuid,
		'operation_id',p_operation_id,
		'snapshot_id',snapshot_row.snapshot_id,
		'consumer_kind',p_consumer_kind,
		'conversation_id',p_conversation_id,
		'conversation_revision',p_expected_conversation_revision,
		'source_runtime_session_id',p_source_runtime_session_id,
		'source_runtime_session_revision',
			p_expected_source_runtime_session_revision,
		'turn_id',p_turn_id,
		'managed_run_id',p_managed_run_id,
		'managed_run_revision',p_expected_managed_run_revision,
		'managed_execution_id',p_managed_execution_id,
		'kind',decision_kind,
		'selected_account_id',selected_account,
		'waiting_ready_at_micros',ready_micros,
		'no_route_reason',CASE
			WHEN decision_kind='no_route' THEN 'blocked_evidence'
		END,
		'decided_at_micros',decided_micros,
		'members',(
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.jsonb_build_object(
					'position',member.position,
					'account_id',member.account_id,
					'disposition',member.disposition,
					'sticky',member.sticky,
					'blockers',COALESCE((
						SELECT pg_catalog.jsonb_agg(
							blocker.blocker ORDER BY blocker.position
						)
						FROM decodex.routing_decision_blocker_refs AS blocker
						WHERE blocker.decision_id=decision_uuid
							AND blocker.account_id=member.account_id
					),'[]'::jsonb)
				)
				ORDER BY member.position
			)
			FROM decodex.routing_snapshot_members AS member
			WHERE member.snapshot_id=snapshot_row.snapshot_id
		),
		'quota_facts',(
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.to_jsonb(quota.*)-'decision_id'-'snapshot_id'
				ORDER BY member.position,quota.position
			)
			FROM decodex.routing_decision_quota_refs AS quota
			JOIN decodex.routing_decision_member_refs AS member
				USING(decision_id,account_id)
			WHERE quota.decision_id=decision_uuid
		),
		'capability_facts',(
			SELECT pg_catalog.jsonb_agg(
				pg_catalog.to_jsonb(capability.*)-'decision_id'-'snapshot_id'
				ORDER BY member.position,capability.position
			)
			FROM decodex.routing_decision_capability_refs AS capability
			JOIN decodex.routing_decision_member_refs AS member
				USING(decision_id,account_id)
			WHERE capability.decision_id=decision_uuid
		),
		'exclusions',(
			SELECT COALESCE(
				pg_catalog.jsonb_agg(
					pg_catalog.to_jsonb(exclusion.*)-'decision_id'
					ORDER BY member_position,window_class
				),
				'[]'::jsonb
			)
			FROM decodex.routing_decision_exclusions AS exclusion
			WHERE decision_id=decision_uuid
		),
		'causes',CASE WHEN decision_kind='selected' THEN '[]'::jsonb ELSE (
			SELECT COALESCE(
				pg_catalog.jsonb_agg(
					pg_catalog.jsonb_build_object(
						'account_id',blocker.account_id,
						'blocker',blocker.blocker
					)
					ORDER BY member.position,blocker.position
				),
				'[]'::jsonb
			)
			FROM decodex.routing_decision_blocker_refs AS blocker
			JOIN decodex.routing_snapshot_members AS member
				ON member.snapshot_id=blocker.snapshot_id
				AND member.account_id=blocker.account_id
			WHERE blocker.decision_id=decision_uuid
				AND (
					decision_kind='no_route'
					OR member.disposition='included'
				)
		) END
	);
	effect:=core||pg_catalog.jsonb_build_object(
		'effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(
			public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),
			'hex'
		)
	);
	response:=pg_catalog.convert_to(
		pg_catalog.jsonb_build_object(
			'classification','success',
			'effect',effect
		)::text,
		'UTF8'
	);
	UPDATE decodex.exact_command_receipts
	SET receipt_state='completed_success',outcome_class='success',
		effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol
		AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

REVOKE ALL ON FUNCTION
	decodex.resolve_routing_snapshot_exact(
		text,text,uuid,bigint,decodex.provider_attempt_consumer_kind,
		uuid,bigint,uuid,bigint,uuid,uuid,bigint,uuid
	),
	decodex.route_account_exact(
		text,text,uuid,uuid,bigint,
		decodex.provider_attempt_consumer_kind,
		uuid,bigint,uuid,bigint,uuid,uuid,bigint,uuid
	),
	decodex.plan_continuation_exact(
		text,text,uuid,uuid,bigint,uuid,uuid,uuid,uuid,bytea,text,text,
		integer,integer,text,boolean,integer,text[],text[],bigint[],
		text[],bigint[],bigint[],text[],text[],text[],bigint[]
		),
		decodex.read_execution_decision_exact(uuid),
		decodex.read_managed_run_execution_exact(uuid,uuid,bigint),
		decodex.enforce_continuation_plan_completeness(),
		decodex.enforce_provider_attempt_binding()
	FROM PUBLIC;

-- Derive the one accepted runtime principal from the still-live V24 ProviderAttempt anchor.
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
	WHERE role.rolname=current_user;
	anchor_oid:=pg_catalog.to_regprocedure(
		'decodex.prepare_provider_attempt_exact(pg_catalog.uuid,decodex.provider_attempt_consumer_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,pg_catalog.uuid,pg_catalog.text,pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.text)'
	);
	IF anchor_oid IS NULL OR NOT EXISTS (
		SELECT 1
		FROM pg_catalog.pg_proc AS procedure
		WHERE procedure.oid=anchor_oid
			AND procedure.proowner=migration_role_oid
	) THEN
		RAISE EXCEPTION 'V26 runtime principal anchor is missing or not migration-owned'
			USING ERRCODE='42501';
	END IF;
	SELECT
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee=migration_role_oid
		),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee<>migration_role_oid
				AND role.oid IS NOT NULL
		),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee=0
				OR privilege.grantor<>migration_role_oid
				OR (
					privilege.grantee<>migration_role_oid
					AND (privilege.is_grantable OR role.oid IS NULL)
				)
		),
		pg_catalog.min(role.rolname) FILTER (
			WHERE privilege.grantee<>migration_role_oid
				AND role.oid IS NOT NULL
		)
	INTO owner_execute_count,runtime_role_count,
		invalid_execute_count,runtime_role
	FROM pg_catalog.pg_proc AS procedure
	CROSS JOIN LATERAL pg_catalog.aclexplode(
		COALESCE(
			procedure.proacl,
			pg_catalog.acldefault('f',procedure.proowner)
		)
	) AS privilege
	LEFT JOIN pg_catalog.pg_roles AS role
		ON role.oid=privilege.grantee
	WHERE procedure.oid=anchor_oid
		AND privilege.privilege_type='EXECUTE';
	IF owner_execute_count<>1
		OR runtime_role_count>1
		OR invalid_execute_count<>0
	THEN
		RAISE EXCEPTION 'V26 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count=1 THEN
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.resolve_routing_snapshot_exact(text,text,uuid,bigint,decodex.provider_attempt_consumer_kind,uuid,bigint,uuid,bigint,uuid,uuid,bigint,uuid) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.route_account_exact(text,text,uuid,uuid,bigint,decodex.provider_attempt_consumer_kind,uuid,bigint,uuid,bigint,uuid,uuid,bigint,uuid) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.plan_continuation_exact(text,text,uuid,uuid,bigint,uuid,uuid,uuid,uuid,bytea,text,text,integer,integer,text,boolean,integer,text[],text[],bigint[],text[],bigint[],bigint[],text[],text[],text[],bigint[]) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_execution_decision_exact(uuid) TO %I',
			runtime_role
		);
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_managed_run_execution_exact(uuid,uuid,bigint) TO %I',
			runtime_role
		);
	END IF;
END
$$;
