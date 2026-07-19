-- XY-1362 durable, inert, ledger-first wake lifecycle for one exact persisted V16
-- waiting_usage decision. The append-only transition is authority; the mutable head is
-- only a due-order projection and fence. No candidate, evidence, selection, credential,
-- continuation, dispatch, or production scheduler authority is accepted or emitted.

CREATE TYPE decodex.waiting_usage_wake_state AS ENUM (
	'pending', 'leased', 'fired', 'cancelled', 'superseded'
);
CREATE TYPE decodex.waiting_usage_wake_transition_kind AS ENUM (
	'registered', 'claimed', 'reclaimed', 'fired', 'cancelled', 'superseded'
);
CREATE TYPE decodex.waiting_usage_wake_terminal_reason AS ENUM (
	'explicit_cancellation', 'managed_run_stale', 'policy_revision_stale',
	'ambiguous_decision_lineage'
);

CREATE TABLE decodex.waiting_usage_wake_transitions (
	transition_id uuid PRIMARY KEY,
	wake_id uuid NOT NULL,
	revision bigint NOT NULL CHECK (revision > 0),
	predecessor_revision bigint,
	predecessor_transition_id uuid,
	operation_id uuid NOT NULL UNIQUE,
	transition_kind decodex.waiting_usage_wake_transition_kind NOT NULL,
	request_envelope jsonb NOT NULL,
	request_digest bytea NOT NULL,
	registration_operation_id uuid NOT NULL,
	routing_decision_id uuid NOT NULL REFERENCES decodex.routing_decisions(decision_id),
	routing_decision_revision bigint NOT NULL DEFAULT 1 CHECK (routing_decision_revision = 1),
	routing_policy_id uuid NOT NULL,
	routing_policy_revision bigint NOT NULL CHECK (routing_policy_revision > 0),
	managed_run_id uuid NOT NULL,
	managed_run_revision bigint NOT NULL CHECK (managed_run_revision > 0),
	earliest_ready_at_micros bigint NOT NULL
		CHECK (earliest_ready_at_micros BETWEEN 0 AND 253402300799999999),
	state decodex.waiting_usage_wake_state NOT NULL,
	claim_id uuid,
	lease_holder uuid,
	lease_fence_id uuid,
	lease_acquired_at timestamptz,
	lease_expires_at timestamptz,
	terminal_reason decodex.waiting_usage_wake_terminal_reason,
	routing_resolution_request_id uuid UNIQUE,
	fresh_routing_resolution_only boolean NOT NULL DEFAULT true
		CHECK (fresh_routing_resolution_only),
	prior_decision_reusable boolean NOT NULL DEFAULT false CHECK (NOT prior_decision_reusable),
	production_enabled boolean NOT NULL DEFAULT false CHECK (NOT production_enabled),
	registered_at timestamptz NOT NULL,
	transitioned_at timestamptz NOT NULL,
	activity_sequence bigint NOT NULL UNIQUE,
	outbox_id bigint NOT NULL UNIQUE,
	effect_envelope jsonb NOT NULL,
	effect_digest bytea NOT NULL,
	response_bytes bytea NOT NULL,
	CONSTRAINT waiting_usage_wake_transitions_identity UNIQUE (
		wake_id, revision, transition_id
	),
	CONSTRAINT waiting_usage_wake_transitions_revision UNIQUE (wake_id, revision),
	CONSTRAINT waiting_usage_wake_transitions_predecessor_fk FOREIGN KEY (
		wake_id, predecessor_revision, predecessor_transition_id
	) REFERENCES decodex.waiting_usage_wake_transitions(wake_id, revision, transition_id)
		DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT waiting_usage_wake_transitions_decision_fk FOREIGN KEY (
		routing_decision_id, managed_run_id, managed_run_revision
	) REFERENCES decodex.routing_decisions(
		decision_id, managed_run_id, managed_run_revision
	),
	CONSTRAINT waiting_usage_wake_transitions_policy_fk FOREIGN KEY (
		routing_policy_id, routing_policy_revision
	) REFERENCES decodex.routing_policy_revisions(routing_policy_id, revision),
	CONSTRAINT waiting_usage_wake_transitions_chain CHECK (
		(revision = 1 AND predecessor_revision IS NULL
			AND predecessor_transition_id IS NULL AND transition_kind = 'registered'
			AND operation_id = registration_operation_id)
		OR (revision > 1 AND predecessor_revision = revision - 1
			AND predecessor_transition_id IS NOT NULL AND transition_kind <> 'registered')
	),
	CONSTRAINT waiting_usage_wake_transitions_state_shape CHECK (
		(transition_kind = 'registered' AND state = 'pending'
			AND claim_id IS NULL AND lease_holder IS NULL AND lease_fence_id IS NULL
			AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason IS NULL AND routing_resolution_request_id IS NULL)
		OR (transition_kind IN ('claimed','reclaimed') AND state = 'leased'
			AND claim_id IS NOT NULL AND lease_holder IS NOT NULL AND lease_fence_id IS NOT NULL
			AND lease_acquired_at IS NOT NULL AND lease_expires_at IS NOT NULL
			AND terminal_reason IS NULL AND routing_resolution_request_id IS NULL)
		OR (transition_kind = 'fired' AND state = 'fired'
			AND claim_id IS NULL AND lease_holder IS NULL AND lease_fence_id IS NULL
			AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason IS NULL AND routing_resolution_request_id IS NOT NULL)
		OR (transition_kind = 'cancelled' AND state = 'cancelled'
			AND claim_id IS NULL AND lease_holder IS NULL AND lease_fence_id IS NULL
			AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason = 'explicit_cancellation'
			AND routing_resolution_request_id IS NULL)
		OR (transition_kind = 'superseded' AND state = 'superseded'
			AND claim_id IS NULL AND lease_holder IS NULL AND lease_fence_id IS NULL
			AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason IN ('managed_run_stale','policy_revision_stale',
				'ambiguous_decision_lineage') AND routing_resolution_request_id IS NULL)
	),
	CONSTRAINT waiting_usage_wake_transitions_lease_duration CHECK (
		state <> 'leased' OR lease_expires_at = lease_acquired_at + INTERVAL '60 seconds'
	),
	CONSTRAINT waiting_usage_wake_transitions_finite_times CHECK (
		pg_catalog.isfinite(registered_at) AND pg_catalog.isfinite(transitioned_at)
		AND transitioned_at >= registered_at
		AND (lease_acquired_at IS NULL OR pg_catalog.isfinite(lease_acquired_at))
		AND (lease_expires_at IS NULL OR pg_catalog.isfinite(lease_expires_at))
		AND (lease_acquired_at IS NULL OR lease_acquired_at = transitioned_at)
	),
	CONSTRAINT waiting_usage_wake_transitions_request_exact CHECK (
		request_envelope->>'operation' IS NOT NULL
		AND request_envelope->>'operation_id' IS NOT NULL
		AND request_digest = public.digest(
			pg_catalog.convert_to(request_envelope::text,'UTF8'),'sha256')
		AND request_envelope->>'operation_id' = operation_id::text
		AND NOT decodex.has_credential_material(request_envelope)
	),
	CONSTRAINT waiting_usage_wake_transitions_effect_exact CHECK (
		effect_envelope->>'operation' IS NOT NULL
		AND effect_envelope->>'operation_id' IS NOT NULL
		AND effect_envelope->>'transition_id' IS NOT NULL
		AND effect_envelope->>'revision' IS NOT NULL
		AND effect_envelope->>'state' IS NOT NULL
		AND effect_envelope->>'transition_kind' IS NOT NULL
		AND effect_envelope->>'operation' = request_envelope->>'operation'
		AND effect_digest = public.digest(pg_catalog.convert_to(
			effect_envelope->>'effect_digest_source','UTF8'),'sha256')
		AND effect_envelope->>'effect_digest' = pg_catalog.encode(effect_digest,'hex')
		AND (effect_envelope - 'effect_digest' - 'effect_digest_source')::text
			= effect_envelope->>'effect_digest_source'
		AND effect_envelope->>'operation_id' = operation_id::text
		AND effect_envelope->>'transition_id' = transition_id::text
		AND (effect_envelope->>'revision')::bigint = revision
		AND effect_envelope->>'state' = state::text
		AND effect_envelope->>'transition_kind' = transition_kind::text
		AND NOT decodex.has_credential_material(effect_envelope)
	),
	CONSTRAINT waiting_usage_wake_transitions_response_exact CHECK (
		pg_catalog.convert_from(response_bytes,'UTF8')::jsonb
			= pg_catalog.jsonb_build_object('classification','success','effect',effect_envelope)
	)
);

CREATE UNIQUE INDEX waiting_usage_wake_transitions_decision_once
ON decodex.waiting_usage_wake_transitions(routing_decision_id) WHERE revision = 1;
CREATE UNIQUE INDEX waiting_usage_wake_transitions_run_once
ON decodex.waiting_usage_wake_transitions(managed_run_id, managed_run_revision)
WHERE revision = 1;
CREATE UNIQUE INDEX waiting_usage_wake_transitions_claim_once
ON decodex.waiting_usage_wake_transitions(claim_id) WHERE claim_id IS NOT NULL;
CREATE UNIQUE INDEX waiting_usage_wake_transitions_fence_once
ON decodex.waiting_usage_wake_transitions(lease_fence_id) WHERE lease_fence_id IS NOT NULL;

CREATE TABLE decodex.waiting_usage_wake_heads (
	wake_id uuid PRIMARY KEY,
	revision bigint NOT NULL CHECK (revision > 0),
	transition_id uuid NOT NULL,
	registration_operation_id uuid NOT NULL,
	routing_decision_id uuid NOT NULL UNIQUE,
	routing_decision_revision bigint NOT NULL CHECK (routing_decision_revision = 1),
	routing_policy_id uuid NOT NULL,
	routing_policy_revision bigint NOT NULL CHECK (routing_policy_revision > 0),
	managed_run_id uuid NOT NULL,
	managed_run_revision bigint NOT NULL CHECK (managed_run_revision > 0),
	earliest_ready_at_micros bigint NOT NULL
		CHECK (earliest_ready_at_micros BETWEEN 0 AND 253402300799999999),
	state decodex.waiting_usage_wake_state NOT NULL,
	claim_id uuid,
	lease_holder uuid,
	lease_fence_id uuid,
	lease_acquired_at timestamptz,
	lease_expires_at timestamptz,
	terminal_reason decodex.waiting_usage_wake_terminal_reason,
	routing_resolution_request_id uuid,
	registered_at timestamptz NOT NULL,
	updated_at timestamptz NOT NULL,
	CONSTRAINT waiting_usage_wake_heads_run_once UNIQUE (managed_run_id, managed_run_revision),
	CONSTRAINT waiting_usage_wake_heads_tip_fk FOREIGN KEY (wake_id, revision, transition_id)
		REFERENCES decodex.waiting_usage_wake_transitions(wake_id, revision, transition_id)
		DEFERRABLE INITIALLY DEFERRED,
	CONSTRAINT waiting_usage_wake_heads_shape CHECK (
		(state = 'pending' AND claim_id IS NULL AND lease_holder IS NULL
			AND lease_fence_id IS NULL AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason IS NULL AND routing_resolution_request_id IS NULL)
		OR (state = 'leased' AND claim_id IS NOT NULL AND lease_holder IS NOT NULL
			AND lease_fence_id IS NOT NULL AND lease_acquired_at IS NOT NULL
			AND lease_expires_at IS NOT NULL AND terminal_reason IS NULL
			AND routing_resolution_request_id IS NULL)
		OR (state = 'fired' AND claim_id IS NULL AND lease_holder IS NULL
			AND lease_fence_id IS NULL AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason IS NULL AND routing_resolution_request_id IS NOT NULL)
		OR (state IN ('cancelled','superseded') AND claim_id IS NULL
			AND lease_holder IS NULL AND lease_fence_id IS NULL
			AND lease_acquired_at IS NULL AND lease_expires_at IS NULL
			AND terminal_reason IS NOT NULL AND routing_resolution_request_id IS NULL)
	),
	CONSTRAINT waiting_usage_wake_heads_finite_times CHECK (
		pg_catalog.isfinite(registered_at) AND pg_catalog.isfinite(updated_at)
		AND updated_at >= registered_at
		AND (lease_acquired_at IS NULL OR pg_catalog.isfinite(lease_acquired_at))
		AND (lease_expires_at IS NULL OR pg_catalog.isfinite(lease_expires_at))
	)
);

CREATE INDEX waiting_usage_wake_heads_due_order
ON decodex.waiting_usage_wake_heads(earliest_ready_at_micros, registered_at, wake_id)
WHERE state IN ('pending','leased');

CREATE FUNCTION decodex.enforce_waiting_usage_wake_command_owner()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name;
BEGIN
	SELECT role.rolname INTO owner_name FROM pg_catalog.pg_class AS class
	JOIN pg_catalog.pg_roles AS role ON role.oid=class.relowner WHERE class.oid=TG_RELID;
	IF current_user::name<>owner_name THEN
		RAISE EXCEPTION 'waiting-usage wake state is writable only by its command owner'
			USING ERRCODE='42501', CONSTRAINT='waiting_usage_wake_command_owner';
	END IF;
	RETURN NULL;
END
$$;
CREATE TRIGGER waiting_usage_wake_transitions_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.waiting_usage_wake_transitions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_waiting_usage_wake_command_owner();
CREATE TRIGGER waiting_usage_wake_heads_command_owner
BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON decodex.waiting_usage_wake_heads
FOR EACH STATEMENT EXECUTE FUNCTION decodex.enforce_waiting_usage_wake_command_owner();

CREATE FUNCTION decodex.forbid_waiting_usage_wake_transition_mutation()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
BEGIN
	RAISE EXCEPTION 'waiting-usage wake transitions are append-only'
		USING ERRCODE='55000', CONSTRAINT='waiting_usage_wake_transition_immutable';
END
$$;
CREATE TRIGGER waiting_usage_wake_transitions_immutable
BEFORE UPDATE OR DELETE OR TRUNCATE ON decodex.waiting_usage_wake_transitions
FOR EACH STATEMENT EXECUTE FUNCTION decodex.forbid_waiting_usage_wake_transition_mutation();

CREATE FUNCTION decodex.enforce_waiting_usage_wake_transition_complete()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE predecessor decodex.waiting_usage_wake_transitions%ROWTYPE;
BEGIN
	IF NEW.revision>1 THEN
		SELECT * INTO STRICT predecessor FROM decodex.waiting_usage_wake_transitions
		WHERE wake_id=NEW.wake_id AND revision=NEW.predecessor_revision
			AND transition_id=NEW.predecessor_transition_id;
		IF predecessor.registration_operation_id<>NEW.registration_operation_id
			OR predecessor.routing_decision_id<>NEW.routing_decision_id
			OR predecessor.routing_decision_revision<>NEW.routing_decision_revision
			OR predecessor.routing_policy_id<>NEW.routing_policy_id
			OR predecessor.routing_policy_revision<>NEW.routing_policy_revision
			OR predecessor.managed_run_id<>NEW.managed_run_id
			OR predecessor.managed_run_revision<>NEW.managed_run_revision
			OR predecessor.earliest_ready_at_micros<>NEW.earliest_ready_at_micros
			OR predecessor.registered_at<>NEW.registered_at
			OR predecessor.state IN ('fired','cancelled','superseded')
			OR (NEW.transition_kind='claimed' AND predecessor.state<>'pending')
			OR (NEW.transition_kind='reclaimed' AND (
				predecessor.state<>'leased'
				OR predecessor.lease_expires_at>NEW.transitioned_at))
			OR (NEW.transition_kind='fired' AND predecessor.state<>'leased')
			OR (NEW.transition_kind='cancelled' AND predecessor.state NOT IN ('pending','leased'))
			OR (NEW.transition_kind='superseded' AND predecessor.state NOT IN ('pending','leased'))
		THEN
			RAISE EXCEPTION 'waiting-usage wake transition has a forged predecessor'
				USING ERRCODE='23514', CONSTRAINT='waiting_usage_wake_transition_chain';
		END IF;
	END IF;
	IF (NEW.transition_kind='registered' AND (
			NEW.request_envelope->>'operation' IS DISTINCT FROM 'register_waiting_usage_wake'
			OR NEW.request_envelope->>'decision_id' IS DISTINCT FROM NEW.routing_decision_id::text
			OR (NEW.request_envelope->>'expected_managed_run_revision')::bigint
				IS DISTINCT FROM NEW.managed_run_revision))
		OR (NEW.transition_kind IN ('claimed','reclaimed') AND (
			NEW.request_envelope->>'operation' IS DISTINCT FROM 'claim_due_waiting_usage_wake'
			OR NEW.request_envelope->>'claim_id' IS DISTINCT FROM NEW.claim_id::text
			OR NEW.request_envelope->>'holder_id' IS DISTINCT FROM NEW.lease_holder::text))
		OR (NEW.transition_kind='fired' AND (
			NEW.request_envelope->>'operation' IS DISTINCT FROM 'fire_waiting_usage_wake'
			OR NEW.request_envelope->>'wake_id' IS DISTINCT FROM NEW.wake_id::text
			OR (NEW.request_envelope->>'expected_revision')::bigint IS DISTINCT FROM predecessor.revision
			OR NEW.request_envelope->>'expected_transition_id' IS DISTINCT FROM predecessor.transition_id::text
			OR NEW.request_envelope->>'holder_id' IS DISTINCT FROM predecessor.lease_holder::text
			OR NEW.request_envelope->>'lease_fence_id' IS DISTINCT FROM predecessor.lease_fence_id::text))
		OR (NEW.transition_kind='cancelled' AND (
			NEW.request_envelope->>'operation' IS DISTINCT FROM 'cancel_waiting_usage_wake'
			OR NEW.request_envelope->>'wake_id' IS DISTINCT FROM NEW.wake_id::text
			OR (NEW.request_envelope->>'expected_revision')::bigint IS DISTINCT FROM predecessor.revision
			OR NEW.request_envelope->>'expected_transition_id' IS DISTINCT FROM predecessor.transition_id::text))
		OR (NEW.transition_kind='superseded' AND NOT (
			(NEW.request_envelope->>'operation' IS NOT DISTINCT FROM 'claim_due_waiting_usage_wake')
			OR (NEW.request_envelope->>'operation' IS NOT DISTINCT FROM 'fire_waiting_usage_wake'
				AND NEW.request_envelope->>'wake_id' IS NOT DISTINCT FROM NEW.wake_id::text
				AND (NEW.request_envelope->>'expected_revision')::bigint IS NOT DISTINCT FROM predecessor.revision
				AND NEW.request_envelope->>'expected_transition_id' IS NOT DISTINCT FROM predecessor.transition_id::text
				AND NEW.request_envelope->>'holder_id' IS NOT DISTINCT FROM predecessor.lease_holder::text
				AND NEW.request_envelope->>'lease_fence_id' IS NOT DISTINCT FROM predecessor.lease_fence_id::text)))
	THEN
		RAISE EXCEPTION 'waiting-usage wake operation request is not bound to its transition'
			USING ERRCODE='23514', CONSTRAINT='waiting_usage_wake_transition_request';
	END IF;
	IF NOT EXISTS (
		SELECT 1 FROM decodex.routing_decisions AS decision
		JOIN decodex.routing_snapshots AS snapshot ON snapshot.snapshot_id=decision.snapshot_id
		WHERE decision.decision_id=NEW.routing_decision_id AND decision.kind='waiting_usage'
			AND decision.waiting_ready_at_micros=NEW.earliest_ready_at_micros
			AND decision.routing_policy_id=NEW.routing_policy_id
			AND decision.routing_policy_revision=NEW.routing_policy_revision
			AND decision.managed_run_id=NEW.managed_run_id
			AND decision.managed_run_revision=NEW.managed_run_revision
			AND snapshot.routing_policy_id=decision.routing_policy_id
			AND snapshot.routing_policy_revision=decision.routing_policy_revision
			AND snapshot.managed_run_id=decision.managed_run_id
			AND snapshot.managed_run_revision=decision.managed_run_revision
	) OR NOT EXISTS (
		SELECT 1 FROM decodex.waiting_usage_wake_heads AS head
		WHERE (head.wake_id,head.revision,head.transition_id)=
			(NEW.wake_id,NEW.revision,NEW.transition_id)
			AND head.registration_operation_id=NEW.registration_operation_id
			AND head.routing_decision_id=NEW.routing_decision_id
			AND head.routing_decision_revision=NEW.routing_decision_revision
			AND head.routing_policy_id=NEW.routing_policy_id
			AND head.routing_policy_revision=NEW.routing_policy_revision
			AND head.managed_run_id=NEW.managed_run_id
			AND head.managed_run_revision=NEW.managed_run_revision
			AND head.earliest_ready_at_micros=NEW.earliest_ready_at_micros
			AND head.state=NEW.state AND head.claim_id IS NOT DISTINCT FROM NEW.claim_id
			AND head.lease_holder IS NOT DISTINCT FROM NEW.lease_holder
			AND head.lease_fence_id IS NOT DISTINCT FROM NEW.lease_fence_id
			AND head.lease_acquired_at IS NOT DISTINCT FROM NEW.lease_acquired_at
			AND head.lease_expires_at IS NOT DISTINCT FROM NEW.lease_expires_at
			AND head.terminal_reason IS NOT DISTINCT FROM NEW.terminal_reason
			AND head.routing_resolution_request_id IS NOT DISTINCT FROM
				NEW.routing_resolution_request_id
			AND head.registered_at=NEW.registered_at AND head.updated_at=NEW.transitioned_at
	) OR NOT EXISTS (
		SELECT 1 FROM decodex.activity AS activity
		WHERE activity.sequence=NEW.activity_sequence
			AND activity.aggregate_kind='waiting_usage_wake'
			AND activity.aggregate_id=NEW.wake_id::text AND activity.revision=NEW.revision
			AND activity.correlation_key=NEW.operation_id::text
			AND activity.event_kind=CASE NEW.transition_kind
				WHEN 'registered' THEN 'waiting_usage_wake_registered'
				WHEN 'claimed' THEN 'waiting_usage_wake_claimed'
				WHEN 'reclaimed' THEN 'waiting_usage_wake_reclaimed'
				WHEN 'fired' THEN 'waiting_usage_wake_fired'
				WHEN 'cancelled' THEN 'waiting_usage_wake_cancelled'
				WHEN 'superseded' THEN 'waiting_usage_wake_superseded' END
			AND activity.payload->>'waiting_usage_wake_transition_id'=NEW.transition_id::text
	) OR NOT EXISTS (
		SELECT 1 FROM decodex.outbox AS outbox
		WHERE outbox.id=NEW.outbox_id
			AND outbox.effect_key='activity/'||NEW.activity_sequence::text
			AND outbox.aggregate_kind='waiting_usage_wake'
			AND outbox.aggregate_id=NEW.wake_id::text
			AND outbox.aggregate_revision=NEW.revision
			AND outbox.payload->>'waiting_usage_wake_transition_id'=NEW.transition_id::text
	) THEN
		RAISE EXCEPTION 'waiting-usage wake transition cluster is incomplete'
			USING ERRCODE='23514', CONSTRAINT='waiting_usage_wake_transition_complete';
	END IF;
	RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER waiting_usage_wake_transition_complete
AFTER INSERT ON decodex.waiting_usage_wake_transitions DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_waiting_usage_wake_transition_complete();

CREATE FUNCTION decodex.enforce_waiting_usage_wake_head_projection()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE tip decodex.waiting_usage_wake_transitions%ROWTYPE;
BEGIN
	IF TG_OP='DELETE' THEN
		RAISE EXCEPTION 'waiting-usage wake head cannot be deleted'
			USING ERRCODE='23514', CONSTRAINT='waiting_usage_wake_head_projection';
	END IF;
	SELECT * INTO STRICT tip FROM decodex.waiting_usage_wake_transitions
	WHERE (wake_id,revision,transition_id)=(NEW.wake_id,NEW.revision,NEW.transition_id);
	IF tip.registration_operation_id<>NEW.registration_operation_id
		OR tip.routing_decision_id<>NEW.routing_decision_id
		OR tip.routing_decision_revision<>NEW.routing_decision_revision
		OR tip.routing_policy_id<>NEW.routing_policy_id
		OR tip.routing_policy_revision<>NEW.routing_policy_revision
		OR tip.managed_run_id<>NEW.managed_run_id
		OR tip.managed_run_revision<>NEW.managed_run_revision
		OR tip.earliest_ready_at_micros<>NEW.earliest_ready_at_micros
		OR tip.state<>NEW.state OR tip.claim_id IS DISTINCT FROM NEW.claim_id
		OR tip.lease_holder IS DISTINCT FROM NEW.lease_holder
		OR tip.lease_fence_id IS DISTINCT FROM NEW.lease_fence_id
		OR tip.lease_acquired_at IS DISTINCT FROM NEW.lease_acquired_at
		OR tip.lease_expires_at IS DISTINCT FROM NEW.lease_expires_at
		OR tip.terminal_reason IS DISTINCT FROM NEW.terminal_reason
		OR tip.routing_resolution_request_id IS DISTINCT FROM NEW.routing_resolution_request_id
		OR tip.registered_at<>NEW.registered_at OR tip.transitioned_at<>NEW.updated_at
		OR (TG_OP='INSERT' AND NEW.revision<>1)
		OR (TG_OP='UPDATE' AND (
			NEW.wake_id<>OLD.wake_id OR NEW.registration_operation_id<>OLD.registration_operation_id
			OR NEW.routing_decision_id<>OLD.routing_decision_id
			OR NEW.routing_decision_revision<>OLD.routing_decision_revision
			OR NEW.routing_policy_id<>OLD.routing_policy_id
			OR NEW.routing_policy_revision<>OLD.routing_policy_revision
			OR NEW.managed_run_id<>OLD.managed_run_id
			OR NEW.managed_run_revision<>OLD.managed_run_revision
			OR NEW.earliest_ready_at_micros<>OLD.earliest_ready_at_micros
			OR NEW.registered_at<>OLD.registered_at OR NEW.revision<>OLD.revision+1
			OR tip.predecessor_revision<>OLD.revision
			OR tip.predecessor_transition_id<>OLD.transition_id
		))
	THEN
		RAISE EXCEPTION 'waiting-usage wake head is not the exact monotonic ledger tip'
			USING ERRCODE='23514', CONSTRAINT='waiting_usage_wake_head_projection';
	END IF;
	RETURN NULL;
END
$$;
CREATE CONSTRAINT TRIGGER waiting_usage_wake_head_projection
AFTER INSERT OR UPDATE OR DELETE ON decodex.waiting_usage_wake_heads
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION decodex.enforce_waiting_usage_wake_head_projection();

CREATE FUNCTION decodex.enforce_waiting_usage_wake_event_namespace()
RETURNS trigger LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE owner_name name; linked boolean;
BEGIN
	SELECT role.rolname INTO owner_name FROM pg_catalog.pg_class AS class
	JOIN pg_catalog.pg_roles AS role ON role.oid=class.relowner WHERE class.oid=TG_RELID;
	IF TG_TABLE_NAME='activity' THEN
		linked:=NEW.aggregate_kind IN ('waiting_usage_wake','routing_resolution_request')
			OR NEW.event_kind LIKE 'waiting_usage_wake_%'
			OR pg_catalog.jsonb_path_exists(NEW.payload,'$.** ? (
				exists(@.waiting_usage_wake_id) || exists(@.waiting_usage_wake_transition_id)
				|| exists(@.routing_resolution_request_id))');
		IF TG_OP='UPDATE' THEN
			linked:=linked OR OLD.aggregate_kind IN ('waiting_usage_wake','routing_resolution_request')
				OR OLD.event_kind LIKE 'waiting_usage_wake_%'
				OR pg_catalog.jsonb_path_exists(OLD.payload,'$.** ? (
					exists(@.waiting_usage_wake_id) || exists(@.waiting_usage_wake_transition_id)
					|| exists(@.routing_resolution_request_id))');
		END IF;
		IF linked AND current_user::name<>owner_name THEN
			RAISE EXCEPTION 'waiting-usage wake activity/outbox namespace is command-owned'
				USING ERRCODE='42501', CONSTRAINT='waiting_usage_wake_event_namespace';
		END IF;
	ELSIF TG_TABLE_NAME='outbox' THEN
		linked:=NEW.aggregate_kind IN ('waiting_usage_wake','routing_resolution_request')
			OR pg_catalog.jsonb_path_exists(NEW.payload,'$.** ? (
				exists(@.waiting_usage_wake_id) || exists(@.waiting_usage_wake_transition_id)
				|| exists(@.routing_resolution_request_id))');
		IF TG_OP='UPDATE' THEN
			linked:=linked OR OLD.aggregate_kind IN ('waiting_usage_wake','routing_resolution_request')
				OR pg_catalog.jsonb_path_exists(OLD.payload,'$.** ? (
					exists(@.waiting_usage_wake_id) || exists(@.waiting_usage_wake_transition_id)
					|| exists(@.routing_resolution_request_id))');
		END IF;
		IF linked AND current_user::name<>owner_name THEN
			IF TG_OP='INSERT' THEN
				RAISE EXCEPTION 'waiting-usage wake activity/outbox namespace is command-owned'
					USING ERRCODE='42501', CONSTRAINT='waiting_usage_wake_event_namespace';
			ELSIF NEW.id IS DISTINCT FROM OLD.id OR NEW.effect_key IS DISTINCT FROM OLD.effect_key
			OR NEW.aggregate_kind IS DISTINCT FROM OLD.aggregate_kind
			OR NEW.aggregate_id IS DISTINCT FROM OLD.aggregate_id
			OR NEW.aggregate_revision IS DISTINCT FROM OLD.aggregate_revision
			OR NEW.payload IS DISTINCT FROM OLD.payload OR NEW.created_at IS DISTINCT FROM OLD.created_at
			THEN
				RAISE EXCEPTION 'waiting-usage wake outbox authority fields are command-owned'
					USING ERRCODE='42501', CONSTRAINT='waiting_usage_wake_event_namespace';
			END IF;
		END IF;
	ELSE
		RAISE EXCEPTION 'waiting-usage wake event namespace has unexpected trigger relation'
			USING ERRCODE='42501', CONSTRAINT='waiting_usage_wake_event_namespace';
	END IF;
	RETURN NEW;
END
$$;
CREATE TRIGGER activity_waiting_usage_wake_namespace
BEFORE INSERT OR UPDATE ON decodex.activity
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_waiting_usage_wake_event_namespace();
CREATE TRIGGER outbox_waiting_usage_wake_namespace
BEFORE INSERT OR UPDATE ON decodex.outbox
FOR EACH ROW EXECUTE FUNCTION decodex.enforce_waiting_usage_wake_event_namespace();

CREATE FUNCTION decodex.reserve_exact_waiting_usage_wake_command(
	p_protocol text, p_idempotency_key text, p_request jsonb
) RETURNS bytea LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE stored record;
BEGIN
	IF pg_catalog.current_setting('transaction_isolation')<>'read committed' THEN
		RAISE EXCEPTION 'exact commands require READ COMMITTED' USING ERRCODE='40001';
	END IF;
	IF p_protocol IS NULL OR p_idempotency_key IS NULL OR p_request IS NULL
		OR pg_catalog.octet_length(p_protocol) NOT BETWEEN 1 AND 64
		OR p_protocol COLLATE pg_catalog."C" !~ '^[a-z0-9][a-z0-9._/-]{0,63}$'
		OR pg_catalog.octet_length(p_idempotency_key) NOT BETWEEN 1 AND 256
		OR decodex.has_credential_material(p_idempotency_key) THEN
		RAISE EXCEPTION 'exact waiting-usage wake command identity is invalid' USING ERRCODE='22023';
	END IF;
	INSERT INTO decodex.exact_command_receipts(
		protocol_version,idempotency_key,request_envelope,request_digest,receipt_state
	) VALUES(p_protocol,p_idempotency_key,p_request,
		public.digest(pg_catalog.convert_to(p_request::text,'UTF8'),'sha256'),'executing')
	ON CONFLICT DO NOTHING;
	SELECT request_envelope,response_bytes,receipt_state INTO STRICT stored
	FROM decodex.exact_command_receipts
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key FOR UPDATE;
	IF stored.request_envelope<>p_request THEN
		RAISE EXCEPTION 'idempotency key reused for another waiting-usage wake command'
			USING ERRCODE='DX001';
	END IF;
	IF stored.receipt_state<>'executing' THEN RETURN stored.response_bytes; END IF;
	RETURN NULL;
END
$$;

CREATE FUNCTION decodex.complete_exact_waiting_usage_wake_rejection(
	p_protocol text, p_idempotency_key text, p_operation text, p_code text
) RETURNS bytea LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE core jsonb; effect jsonb; response bytea;
BEGIN
	core:=pg_catalog.jsonb_build_object('operation',p_operation,'rejection',p_code);
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','stable_domain_rejection','effect',effect)::text,'UTF8');
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_rejected',
		outcome_class='stable_domain_rejection',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.replay_waiting_usage_wake_operation_exact(
	p_protocol text, p_idempotency_key text, p_operation text,
	p_operation_id uuid, p_request jsonb
) RETURNS bytea LANGUAGE plpgsql SET search_path = pg_catalog, decodex
AS $$
DECLARE stored decodex.waiting_usage_wake_transitions%ROWTYPE;
BEGIN
	SELECT * INTO stored FROM decodex.waiting_usage_wake_transitions
	WHERE operation_id=p_operation_id;
	IF NOT FOUND THEN RETURN NULL; END IF;
	IF stored.request_envelope<>p_request THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,p_operation,'operation_identity_conflict');
	END IF;
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=stored.effect_envelope,
		response_bytes=stored.response_bytes,completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN stored.response_bytes;
END
$$;

CREATE FUNCTION decodex.read_waiting_usage_wake_transition_exact(
	p_transition_id uuid, p_operation_id uuid
) RETURNS TABLE(
	transition_id text, wake_id text, revision bigint, predecessor_revision bigint,
	predecessor_transition_id text, operation_id text, transition_kind text,
	registration_operation_id text, routing_decision_id text,
	routing_decision_revision bigint, routing_policy_id text, routing_policy_revision bigint,
	managed_run_id text, managed_run_revision bigint, earliest_ready_at_micros bigint,
	state text, claim_id text, lease_holder text, lease_fence_id text,
	lease_acquired_at_micros bigint, lease_expires_at_micros bigint,
	registered_at_micros bigint, transitioned_at_micros bigint, terminal_reason text,
	routing_resolution_request_id text, fresh_routing_resolution_only boolean,
	prior_decision_reusable boolean, production_enabled boolean, effect_envelope jsonb,
	response_bytes bytea
) LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, decodex
AS $$
	SELECT transition.transition_id::text,transition.wake_id::text,transition.revision,
		transition.predecessor_revision,transition.predecessor_transition_id::text,
		transition.operation_id::text,transition.transition_kind::text,
		transition.registration_operation_id::text,transition.routing_decision_id::text,
		transition.routing_decision_revision,transition.routing_policy_id::text,
		transition.routing_policy_revision,transition.managed_run_id::text,
		transition.managed_run_revision,transition.earliest_ready_at_micros,
		transition.state::text,transition.claim_id::text,transition.lease_holder::text,
		transition.lease_fence_id::text,
		CASE WHEN transition.lease_acquired_at IS NULL THEN NULL ELSE
			(extract(epoch FROM transition.lease_acquired_at)*1000000)::bigint END,
		CASE WHEN transition.lease_expires_at IS NULL THEN NULL ELSE
			(extract(epoch FROM transition.lease_expires_at)*1000000)::bigint END,
		(extract(epoch FROM transition.registered_at)*1000000)::bigint,
		(extract(epoch FROM transition.transitioned_at)*1000000)::bigint,
		transition.terminal_reason::text,transition.routing_resolution_request_id::text,
		transition.fresh_routing_resolution_only,transition.prior_decision_reusable,
		transition.production_enabled,transition.effect_envelope,transition.response_bytes
	FROM decodex.waiting_usage_wake_transitions AS transition
	WHERE transition.transition_id=p_transition_id AND transition.operation_id=p_operation_id
$$;

CREATE FUNCTION decodex.register_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_decision_id uuid, p_expected_managed_run_revision bigint
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; decision_row record; run_row record; existing_head record;
DECLARE run_uuid uuid; wake_uuid uuid; transition_uuid uuid; now_value timestamptz;
DECLARE activity_sequence bigint; outbox_id bigint; core jsonb; effect jsonb; response bytea;
DECLARE payload jsonb;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','register_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,'decision_id',p_decision_id,
		'expected_managed_run_revision',p_expected_managed_run_revision);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_decision_id IS NULL
		OR p_expected_managed_run_revision IS NULL OR p_expected_managed_run_revision<=0 THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'register_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	SELECT managed_run_id INTO run_uuid FROM decodex.routing_decisions
	WHERE decision_id=p_decision_id;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'register_waiting_usage_wake','missing_decision'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(run_uuid::text));
	PERFORM pg_catalog.pg_advisory_xact_lock(1362,pg_catalog.hashtext(p_decision_id::text));
	SELECT * INTO decision_row FROM decodex.routing_decisions
	WHERE decision_id=p_decision_id FOR UPDATE;
	IF decision_row.kind<>'waiting_usage' THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','decision_not_waiting_usage');
	END IF;
	SELECT * INTO existing_head FROM decodex.waiting_usage_wake_heads
	WHERE routing_decision_id=p_decision_id FOR UPDATE;
	IF FOUND THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','decision_already_registered');
	END IF;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=decision_row.managed_run_id FOR UPDATE;
	IF run_row.managed_run_id IS NULL OR run_row.revision<>p_expected_managed_run_revision
		OR decision_row.managed_run_revision<>p_expected_managed_run_revision
		OR run_row.lifecycle<>'waiting' OR run_row.wait_reason<>'usage'
		OR NOT run_row.blocked OR run_row.diverged THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','stale_managed_run');
	END IF;
	IF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_heads
		WHERE routing_policy_id=decision_row.routing_policy_id
			AND current_revision=decision_row.routing_policy_revision FOR SHARE) THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake','stale_policy');
	END IF;
	IF EXISTS (SELECT 1 FROM decodex.routing_decisions AS other
		WHERE other.managed_run_id=decision_row.managed_run_id
			AND other.managed_run_revision>=decision_row.managed_run_revision
			AND other.decision_id<>decision_row.decision_id) THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'register_waiting_usage_wake',
			'ambiguous_decision_lineage');
	END IF;
	now_value:=pg_catalog.clock_timestamp();
	wake_uuid:=pg_catalog.gen_random_uuid();
	transition_uuid:=pg_catalog.gen_random_uuid();
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',wake_uuid,
		'waiting_usage_wake_transition_id',transition_uuid,'state','pending',
		'routing_decision_id',p_decision_id,'managed_run_id',decision_row.managed_run_id,
		'managed_run_revision',decision_row.managed_run_revision,'production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','register_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,
		'transition_kind','registered','wake_id',wake_uuid,'revision',1,
		'predecessor_revision',NULL,'predecessor_transition_id',NULL,
		'registration_operation_id',p_operation_id,'routing_decision_id',p_decision_id,
		'routing_decision_revision',1,'routing_policy_id',decision_row.routing_policy_id,
		'routing_policy_revision',decision_row.routing_policy_revision,
		'managed_run_id',decision_row.managed_run_id,
		'managed_run_revision',decision_row.managed_run_revision,
		'earliest_ready_at_micros',decision_row.waiting_ready_at_micros,'state','pending',
		'claim_id',NULL,'lease_holder',NULL,'lease_fence_id',NULL,
		'lease_acquired_at_micros',NULL,'lease_expires_at_micros',NULL,
		'registered_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'transitioned_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'terminal_reason',NULL,'routing_resolution_request_id',NULL,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind','waiting_usage_wake_registered')),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,operation_id,transition_kind,request_envelope,
		request_digest,registration_operation_id,routing_decision_id,routing_policy_id,
		routing_policy_revision,managed_run_id,managed_run_revision,earliest_ready_at_micros,
		state,registered_at,transitioned_at,activity_sequence,outbox_id,effect_envelope,
		effect_digest,response_bytes
	) VALUES(transition_uuid,wake_uuid,1,p_operation_id,'registered',request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),p_operation_id,
		p_decision_id,decision_row.routing_policy_id,decision_row.routing_policy_revision,
		decision_row.managed_run_id,decision_row.managed_run_revision,
		decision_row.waiting_ready_at_micros,'pending',now_value,now_value,
		activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	INSERT INTO decodex.waiting_usage_wake_heads(
		wake_id,revision,transition_id,registration_operation_id,routing_decision_id,
		routing_decision_revision,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,registered_at,updated_at
	) VALUES(wake_uuid,1,transition_uuid,p_operation_id,p_decision_id,1,
		decision_row.routing_policy_id,decision_row.routing_policy_revision,
		decision_row.managed_run_id,decision_row.managed_run_revision,
		decision_row.waiting_ready_at_micros,'pending',now_value,now_value);
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',wake_uuid::text,1,
		'waiting_usage_wake_registered',p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',wake_uuid::text,1,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.claim_due_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid,
	p_claim_id uuid, p_holder_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; head record; decision_row record; run_row record;
DECLARE now_value timestamptz; now_micros bigint; reason text; kind text; state_value text;
DECLARE transition_uuid uuid; fence_uuid uuid; revision_value bigint;
DECLARE activity_sequence bigint; outbox_id bigint; core jsonb; effect jsonb; response bytea;
DECLARE payload jsonb; event_kind text; claimed_value boolean;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','claim_due_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,
		'claim_id',p_claim_id,'holder_id',p_holder_id);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_claim_id IS NULL OR p_holder_id IS NULL THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	PERFORM pg_catalog.pg_advisory_xact_lock(1362,0);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF EXISTS (SELECT 1 FROM decodex.waiting_usage_wake_transitions
		WHERE claim_id=p_claim_id) THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake','claim_identity_conflict');
	END IF;
	now_value:=pg_catalog.clock_timestamp();
	now_micros:=(extract(epoch FROM now_value)*1000000)::bigint;
	SELECT * INTO head FROM decodex.waiting_usage_wake_heads
	WHERE earliest_ready_at_micros<=now_micros
		AND (state='pending' OR (state='leased' AND lease_expires_at<=now_value))
	ORDER BY earliest_ready_at_micros,registered_at,wake_id FOR UPDATE LIMIT 1;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'claim_due_waiting_usage_wake','no_due_wake'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(head.managed_run_id::text));
	SELECT * INTO decision_row FROM decodex.routing_decisions
	WHERE decision_id=head.routing_decision_id FOR SHARE;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=head.managed_run_id FOR SHARE;
	IF run_row.managed_run_id IS NULL OR run_row.revision<>head.managed_run_revision
		OR run_row.lifecycle<>'waiting' OR run_row.wait_reason<>'usage'
		OR NOT run_row.blocked OR run_row.diverged THEN reason:='managed_run_stale';
	ELSIF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_heads
		WHERE routing_policy_id=head.routing_policy_id
			AND current_revision=head.routing_policy_revision FOR SHARE) THEN
		reason:='policy_revision_stale';
	ELSIF decision_row.decision_id IS NULL OR decision_row.kind<>'waiting_usage'
		OR decision_row.managed_run_revision<>head.managed_run_revision
		OR EXISTS (SELECT 1 FROM decodex.routing_decisions AS other
			WHERE other.managed_run_id=head.managed_run_id
				AND other.managed_run_revision>=head.managed_run_revision
				AND other.decision_id<>head.routing_decision_id) THEN
		reason:='ambiguous_decision_lineage';
	END IF;
	transition_uuid:=pg_catalog.gen_random_uuid();
	revision_value:=head.revision+1;
	IF reason IS NULL THEN
		kind:=CASE WHEN head.state='pending' THEN 'claimed' ELSE 'reclaimed' END;
		state_value:='leased'; fence_uuid:=pg_catalog.gen_random_uuid();
		event_kind:=CASE WHEN kind='claimed' THEN 'waiting_usage_wake_claimed'
			ELSE 'waiting_usage_wake_reclaimed' END; claimed_value:=true;
	ELSE
		kind:='superseded'; state_value:='superseded'; event_kind:='waiting_usage_wake_superseded';
		claimed_value:=false;
	END IF;
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',head.wake_id,
		'waiting_usage_wake_transition_id',transition_uuid,'state',state_value,
		'terminal_reason',reason,'production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','claim_due_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,'transition_kind',kind,
		'wake_id',head.wake_id,'revision',revision_value,
		'predecessor_revision',head.revision,'predecessor_transition_id',head.transition_id,
		'registration_operation_id',head.registration_operation_id,
		'routing_decision_id',head.routing_decision_id,'routing_decision_revision',1,
		'routing_policy_id',head.routing_policy_id,'routing_policy_revision',head.routing_policy_revision,
		'managed_run_id',head.managed_run_id,'managed_run_revision',head.managed_run_revision,
		'earliest_ready_at_micros',head.earliest_ready_at_micros,'state',state_value,
		'claim_id',CASE WHEN reason IS NULL THEN p_claim_id ELSE NULL END,
		'lease_holder',CASE WHEN reason IS NULL THEN p_holder_id ELSE NULL END,
		'lease_fence_id',fence_uuid,
		'lease_acquired_at_micros',CASE WHEN reason IS NULL THEN now_micros ELSE NULL END,
		'lease_expires_at_micros',CASE WHEN reason IS NULL THEN now_micros+60000000 ELSE NULL END,
		'registered_at_micros',(extract(epoch FROM head.registered_at)*1000000)::bigint,
		'transitioned_at_micros',now_micros,'terminal_reason',reason,
		'routing_resolution_request_id',NULL,'fresh_routing_resolution_only',true,
		'prior_decision_reusable',false,'production_enabled',false,'claimed',claimed_value,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind',event_kind)),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,predecessor_revision,predecessor_transition_id,
		operation_id,transition_kind,request_envelope,request_digest,registration_operation_id,
		routing_decision_id,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,claim_id,lease_holder,lease_fence_id,
		lease_acquired_at,lease_expires_at,terminal_reason,registered_at,transitioned_at,
		activity_sequence,outbox_id,effect_envelope,effect_digest,response_bytes
	) VALUES(transition_uuid,head.wake_id,revision_value,head.revision,head.transition_id,
		p_operation_id,kind::decodex.waiting_usage_wake_transition_kind,request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),
		head.registration_operation_id,head.routing_decision_id,head.routing_policy_id,
		head.routing_policy_revision,head.managed_run_id,head.managed_run_revision,
		head.earliest_ready_at_micros,state_value::decodex.waiting_usage_wake_state,
		CASE WHEN reason IS NULL THEN p_claim_id ELSE NULL END,
		CASE WHEN reason IS NULL THEN p_holder_id ELSE NULL END,fence_uuid,
		CASE WHEN reason IS NULL THEN now_value ELSE NULL END,
		CASE WHEN reason IS NULL THEN now_value+INTERVAL '60 seconds' ELSE NULL END,
		reason::decodex.waiting_usage_wake_terminal_reason,head.registered_at,now_value,
		activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	UPDATE decodex.waiting_usage_wake_heads SET revision=revision_value,
		transition_id=transition_uuid,state=state_value::decodex.waiting_usage_wake_state,
		claim_id=CASE WHEN reason IS NULL THEN p_claim_id ELSE NULL END,
		lease_holder=CASE WHEN reason IS NULL THEN p_holder_id ELSE NULL END,
		lease_fence_id=fence_uuid,
		lease_acquired_at=CASE WHEN reason IS NULL THEN now_value ELSE NULL END,
		lease_expires_at=CASE WHEN reason IS NULL THEN now_value+INTERVAL '60 seconds' ELSE NULL END,
		terminal_reason=reason::decodex.waiting_usage_wake_terminal_reason,updated_at=now_value
	WHERE wake_id=head.wake_id AND revision=head.revision AND transition_id=head.transition_id;
	IF NOT FOUND THEN RAISE EXCEPTION 'waiting-usage wake head changed during claim'
		USING ERRCODE='40001'; END IF;
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',head.wake_id::text,revision_value,
		event_kind,p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',head.wake_id::text,revision_value,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.fire_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,
	p_expected_revision bigint, p_expected_transition_id uuid,
	p_holder_id uuid, p_lease_fence_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; head record; run_row record; now_value timestamptz;
DECLARE transition_uuid uuid; request_uuid uuid; revision_value bigint; reason text;
DECLARE activity_sequence bigint; outbox_id bigint; core jsonb; effect jsonb; response bytea;
DECLARE payload jsonb; state_value text; event_kind text; run_uuid uuid;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','fire_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,'wake_id',p_wake_id,
		'expected_revision',p_expected_revision,'expected_transition_id',p_expected_transition_id,
		'holder_id',p_holder_id,'lease_fence_id',p_lease_fence_id);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_wake_id IS NULL OR p_expected_revision IS NULL
		OR p_expected_revision<=0 OR p_expected_transition_id IS NULL
		OR p_holder_id IS NULL OR p_lease_fence_id IS NULL THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'fire_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	SELECT managed_run_id INTO run_uuid FROM decodex.waiting_usage_wake_heads
	WHERE wake_id=p_wake_id;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'fire_waiting_usage_wake','wake_not_found'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(run_uuid::text));
	SELECT * INTO head FROM decodex.waiting_usage_wake_heads WHERE wake_id=p_wake_id FOR UPDATE;
	IF head.state IN ('fired','cancelled','superseded') THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','wake_terminal');
	END IF;
	IF head.revision<>p_expected_revision OR head.transition_id<>p_expected_transition_id THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','stale_wake_tip');
	END IF;
	now_value:=pg_catalog.clock_timestamp();
	IF head.state<>'leased' OR head.lease_holder<>p_holder_id
		OR head.lease_fence_id<>p_lease_fence_id OR head.lease_expires_at<=now_value THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'fire_waiting_usage_wake','lease_lost');
	END IF;
	SELECT * INTO run_row FROM decodex.managed_runs
	WHERE managed_run_id=head.managed_run_id FOR SHARE;
	IF run_row.managed_run_id IS NULL OR run_row.revision<>head.managed_run_revision
		OR run_row.lifecycle<>'waiting' OR run_row.wait_reason<>'usage'
		OR NOT run_row.blocked OR run_row.diverged THEN reason:='managed_run_stale';
	ELSIF NOT EXISTS (SELECT 1 FROM decodex.routing_policy_heads
		WHERE routing_policy_id=head.routing_policy_id
			AND current_revision=head.routing_policy_revision FOR SHARE) THEN
		reason:='policy_revision_stale';
	ELSIF NOT EXISTS (SELECT 1 FROM decodex.routing_decisions
		WHERE decision_id=head.routing_decision_id AND kind='waiting_usage'
			AND managed_run_id=head.managed_run_id
			AND managed_run_revision=head.managed_run_revision)
		OR EXISTS (SELECT 1 FROM decodex.routing_decisions AS other
			WHERE other.managed_run_id=head.managed_run_id
				AND other.managed_run_revision>=head.managed_run_revision
				AND other.decision_id<>head.routing_decision_id) THEN
		reason:='ambiguous_decision_lineage';
	END IF;
	transition_uuid:=pg_catalog.gen_random_uuid(); revision_value:=head.revision+1;
	IF reason IS NULL THEN
		request_uuid:=pg_catalog.gen_random_uuid(); state_value:='fired';
		event_kind:='waiting_usage_wake_fired';
	ELSE state_value:='superseded'; event_kind:='waiting_usage_wake_superseded'; END IF;
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',head.wake_id,
		'waiting_usage_wake_transition_id',transition_uuid,'state',state_value,
		'terminal_reason',reason,'routing_resolution_request_id',request_uuid,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','fire_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,
		'transition_kind',CASE WHEN reason IS NULL THEN 'fired' ELSE 'superseded' END,
		'wake_id',head.wake_id,'revision',revision_value,
		'predecessor_revision',head.revision,'predecessor_transition_id',head.transition_id,
		'registration_operation_id',head.registration_operation_id,
		'routing_decision_id',head.routing_decision_id,'routing_decision_revision',1,
		'routing_policy_id',head.routing_policy_id,'routing_policy_revision',head.routing_policy_revision,
		'managed_run_id',head.managed_run_id,'managed_run_revision',head.managed_run_revision,
		'earliest_ready_at_micros',head.earliest_ready_at_micros,'state',state_value,
		'claim_id',NULL,'lease_holder',NULL,'lease_fence_id',NULL,
		'lease_acquired_at_micros',NULL,'lease_expires_at_micros',NULL,
		'registered_at_micros',(extract(epoch FROM head.registered_at)*1000000)::bigint,
		'transitioned_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'terminal_reason',reason,'routing_resolution_request_id',request_uuid,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind',event_kind)),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,predecessor_revision,predecessor_transition_id,
		operation_id,transition_kind,request_envelope,request_digest,registration_operation_id,
		routing_decision_id,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,terminal_reason,
		routing_resolution_request_id,registered_at,transitioned_at,activity_sequence,outbox_id,
		effect_envelope,effect_digest,response_bytes
	) VALUES(transition_uuid,head.wake_id,revision_value,head.revision,head.transition_id,
		p_operation_id,CASE WHEN reason IS NULL THEN 'fired' ELSE 'superseded' END::
			decodex.waiting_usage_wake_transition_kind,request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),
		head.registration_operation_id,head.routing_decision_id,head.routing_policy_id,
		head.routing_policy_revision,head.managed_run_id,head.managed_run_revision,
		head.earliest_ready_at_micros,state_value::decodex.waiting_usage_wake_state,
		reason::decodex.waiting_usage_wake_terminal_reason,request_uuid,head.registered_at,now_value,
		activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	UPDATE decodex.waiting_usage_wake_heads SET revision=revision_value,
		transition_id=transition_uuid,state=state_value::decodex.waiting_usage_wake_state,
		claim_id=NULL,lease_holder=NULL,lease_fence_id=NULL,lease_acquired_at=NULL,
		lease_expires_at=NULL,terminal_reason=reason::decodex.waiting_usage_wake_terminal_reason,
		routing_resolution_request_id=request_uuid,updated_at=now_value
	WHERE wake_id=head.wake_id AND revision=head.revision AND transition_id=head.transition_id;
	IF NOT FOUND THEN RAISE EXCEPTION 'waiting-usage wake head changed during fire'
		USING ERRCODE='40001'; END IF;
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',head.wake_id::text,revision_value,
		event_kind,p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',head.wake_id::text,revision_value,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

CREATE FUNCTION decodex.cancel_waiting_usage_wake_exact(
	p_protocol text, p_idempotency_key text, p_operation_id uuid, p_wake_id uuid,
	p_expected_revision bigint, p_expected_transition_id uuid
) RETURNS bytea LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, decodex
AS $$
DECLARE request jsonb; replay bytea; head record; now_value timestamptz; run_uuid uuid;
DECLARE transition_uuid uuid; revision_value bigint; activity_sequence bigint; outbox_id bigint;
DECLARE core jsonb; effect jsonb; response bytea; payload jsonb;
BEGIN
	request:=pg_catalog.jsonb_build_object('operation','cancel_waiting_usage_wake',
		'protocol',p_protocol,'operation_id',p_operation_id,'wake_id',p_wake_id,
		'expected_revision',p_expected_revision,'expected_transition_id',p_expected_transition_id);
	replay:=decodex.reserve_exact_waiting_usage_wake_command(p_protocol,p_idempotency_key,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	IF p_operation_id IS NULL OR p_wake_id IS NULL OR p_expected_revision IS NULL
		OR p_expected_revision<=0 OR p_expected_transition_id IS NULL THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','invalid_input');
	END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1271);
	replay:=decodex.replay_waiting_usage_wake_operation_exact(
		p_protocol,p_idempotency_key,'cancel_waiting_usage_wake',p_operation_id,request);
	IF replay IS NOT NULL THEN RETURN replay; END IF;
	SELECT managed_run_id INTO run_uuid FROM decodex.waiting_usage_wake_heads WHERE wake_id=p_wake_id;
	IF NOT FOUND THEN RETURN decodex.complete_exact_waiting_usage_wake_rejection(
		p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','wake_not_found'); END IF;
	PERFORM pg_catalog.pg_advisory_xact_lock(1338,pg_catalog.hashtext(run_uuid::text));
	SELECT * INTO head FROM decodex.waiting_usage_wake_heads WHERE wake_id=p_wake_id FOR UPDATE;
	IF head.state IN ('fired','cancelled','superseded') THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','wake_terminal');
	END IF;
	IF head.revision<>p_expected_revision OR head.transition_id<>p_expected_transition_id THEN
		RETURN decodex.complete_exact_waiting_usage_wake_rejection(
			p_protocol,p_idempotency_key,'cancel_waiting_usage_wake','stale_wake_tip');
	END IF;
	now_value:=pg_catalog.clock_timestamp(); transition_uuid:=pg_catalog.gen_random_uuid();
	revision_value:=head.revision+1;
	activity_sequence:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.activity','sequence')::pg_catalog.regclass);
	outbox_id:=pg_catalog.nextval(
		pg_catalog.pg_get_serial_sequence('decodex.outbox','id')::pg_catalog.regclass);
	payload:=pg_catalog.jsonb_build_object('waiting_usage_wake_id',head.wake_id,
		'waiting_usage_wake_transition_id',transition_uuid,'state','cancelled',
		'terminal_reason','explicit_cancellation','production_enabled',false);
	core:=pg_catalog.jsonb_build_object('operation','cancel_waiting_usage_wake',
		'operation_id',p_operation_id,'transition_id',transition_uuid,
		'transition_kind','cancelled','wake_id',head.wake_id,'revision',revision_value,
		'predecessor_revision',head.revision,'predecessor_transition_id',head.transition_id,
		'registration_operation_id',head.registration_operation_id,
		'routing_decision_id',head.routing_decision_id,'routing_decision_revision',1,
		'routing_policy_id',head.routing_policy_id,'routing_policy_revision',head.routing_policy_revision,
		'managed_run_id',head.managed_run_id,'managed_run_revision',head.managed_run_revision,
		'earliest_ready_at_micros',head.earliest_ready_at_micros,'state','cancelled',
		'claim_id',NULL,'lease_holder',NULL,'lease_fence_id',NULL,
		'lease_acquired_at_micros',NULL,'lease_expires_at_micros',NULL,
		'registered_at_micros',(extract(epoch FROM head.registered_at)*1000000)::bigint,
		'transitioned_at_micros',(extract(epoch FROM now_value)*1000000)::bigint,
		'terminal_reason','explicit_cancellation','routing_resolution_request_id',NULL,
		'fresh_routing_resolution_only',true,'prior_decision_reusable',false,
		'production_enabled',false,
		'activity_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'sequence',activity_sequence,'event_kind','waiting_usage_wake_cancelled')),
		'outbox_effects',pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object(
			'id',outbox_id,'effect_key','activity/'||activity_sequence::text)));
	effect:=core||pg_catalog.jsonb_build_object('effect_digest_source',core::text,
		'effect_digest',pg_catalog.encode(public.digest(
			pg_catalog.convert_to(core::text,'UTF8'),'sha256'),'hex'));
	response:=pg_catalog.convert_to(pg_catalog.jsonb_build_object(
		'classification','success','effect',effect)::text,'UTF8');
	INSERT INTO decodex.waiting_usage_wake_transitions(
		transition_id,wake_id,revision,predecessor_revision,predecessor_transition_id,
		operation_id,transition_kind,request_envelope,request_digest,registration_operation_id,
		routing_decision_id,routing_policy_id,routing_policy_revision,managed_run_id,
		managed_run_revision,earliest_ready_at_micros,state,terminal_reason,registered_at,
		transitioned_at,activity_sequence,outbox_id,effect_envelope,effect_digest,response_bytes
	) VALUES(transition_uuid,head.wake_id,revision_value,head.revision,head.transition_id,
		p_operation_id,'cancelled',request,
		public.digest(pg_catalog.convert_to(request::text,'UTF8'),'sha256'),
		head.registration_operation_id,head.routing_decision_id,head.routing_policy_id,
		head.routing_policy_revision,head.managed_run_id,head.managed_run_revision,
		head.earliest_ready_at_micros,'cancelled','explicit_cancellation',head.registered_at,
		now_value,activity_sequence,outbox_id,effect,
		public.digest(pg_catalog.convert_to(core::text,'UTF8'),'sha256'),response);
	UPDATE decodex.waiting_usage_wake_heads SET revision=revision_value,
		transition_id=transition_uuid,state='cancelled',claim_id=NULL,lease_holder=NULL,
		lease_fence_id=NULL,lease_acquired_at=NULL,lease_expires_at=NULL,
		terminal_reason='explicit_cancellation',updated_at=now_value
	WHERE wake_id=head.wake_id AND revision=head.revision AND transition_id=head.transition_id;
	IF NOT FOUND THEN RAISE EXCEPTION 'waiting-usage wake head changed during cancellation'
		USING ERRCODE='40001'; END IF;
	INSERT INTO decodex.activity(sequence,aggregate_kind,aggregate_id,revision,event_kind,
		correlation_key,payload) OVERRIDING SYSTEM VALUE
	VALUES(activity_sequence,'waiting_usage_wake',head.wake_id::text,revision_value,
		'waiting_usage_wake_cancelled',p_operation_id::text,payload);
	INSERT INTO decodex.outbox(id,effect_key,aggregate_kind,aggregate_id,aggregate_revision,payload)
	OVERRIDING SYSTEM VALUE VALUES(outbox_id,'activity/'||activity_sequence::text,
		'waiting_usage_wake',head.wake_id::text,revision_value,payload);
	UPDATE decodex.exact_command_receipts SET receipt_state='completed_success',
		outcome_class='success',effect_envelope=effect,response_bytes=response,
		completed_at=pg_catalog.clock_timestamp()
	WHERE protocol_version=p_protocol AND idempotency_key=p_idempotency_key;
	RETURN response;
END
$$;

REVOKE ALL ON TABLE decodex.waiting_usage_wake_transitions,
	decodex.waiting_usage_wake_heads FROM PUBLIC;
REVOKE ALL ON TYPE decodex.waiting_usage_wake_state,
	decodex.waiting_usage_wake_transition_kind,
	decodex.waiting_usage_wake_terminal_reason FROM PUBLIC;
REVOKE ALL ON FUNCTION decodex.enforce_waiting_usage_wake_command_owner(),
	decodex.forbid_waiting_usage_wake_transition_mutation(),
	decodex.enforce_waiting_usage_wake_transition_complete(),
	decodex.enforce_waiting_usage_wake_head_projection(),
	decodex.enforce_waiting_usage_wake_event_namespace(),
	decodex.reserve_exact_waiting_usage_wake_command(text,text,jsonb),
	decodex.complete_exact_waiting_usage_wake_rejection(text,text,text,text),
	decodex.replay_waiting_usage_wake_operation_exact(text,text,text,uuid,jsonb),
	decodex.read_waiting_usage_wake_transition_exact(uuid,uuid),
	decodex.register_waiting_usage_wake_exact(text,text,uuid,uuid,bigint),
	decodex.claim_due_waiting_usage_wake_exact(text,text,uuid,uuid,uuid),
	decodex.fire_waiting_usage_wake_exact(text,text,uuid,uuid,bigint,uuid,uuid,uuid),
	decodex.cancel_waiting_usage_wake_exact(text,text,uuid,uuid,bigint,uuid) FROM PUBLIC;

-- Bind the V18 entrypoints only to the exact unambiguous migration-owned V12 anchor grantee.
DO $$
DECLARE anchor_oid pg_catalog.oid;
DECLARE migration_role_oid pg_catalog.oid;
DECLARE owner_execute_count pg_catalog.int8;
DECLARE runtime_role_count pg_catalog.int8;
DECLARE invalid_execute_count pg_catalog.int8;
DECLARE runtime_role pg_catalog.name;
BEGIN
	SELECT role.oid INTO migration_role_oid FROM pg_catalog.pg_roles AS role
	WHERE role.rolname=current_user;
	anchor_oid:=pg_catalog.to_regprocedure(
		'decodex.apply_managed_run_safety_input_exact(pg_catalog.text,pg_catalog.text,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.int8,decodex.managed_run_safety_input_kind,pg_catalog.uuid,pg_catalog.uuid,pg_catalog.uuid)');
	IF anchor_oid IS NULL OR NOT EXISTS (
		SELECT 1 FROM pg_catalog.pg_proc AS procedure
		WHERE procedure.oid=anchor_oid AND procedure.proowner=migration_role_oid
	) THEN
		RAISE EXCEPTION 'V18 runtime principal anchor is missing or not migration-owned'
			USING ERRCODE='42501';
	END IF;
	SELECT
		pg_catalog.count(*) FILTER (WHERE privilege.grantee=migration_role_oid),
		pg_catalog.count(*) FILTER (
			WHERE privilege.grantee<>migration_role_oid AND role.oid IS NOT NULL),
		pg_catalog.count(*) FILTER (WHERE privilege.grantee=0
			OR privilege.grantor<>migration_role_oid
			OR (privilege.grantee<>migration_role_oid
				AND (privilege.is_grantable OR role.oid IS NULL))),
		pg_catalog.min(role.rolname) FILTER (
			WHERE privilege.grantee<>migration_role_oid AND role.oid IS NOT NULL)
	INTO owner_execute_count,runtime_role_count,invalid_execute_count,runtime_role
	FROM pg_catalog.pg_proc AS procedure
	CROSS JOIN LATERAL pg_catalog.aclexplode(
		COALESCE(procedure.proacl,pg_catalog.acldefault('f',procedure.proowner))) AS privilege
	LEFT JOIN pg_catalog.pg_roles AS role ON role.oid=privilege.grantee
	WHERE procedure.oid=anchor_oid AND privilege.privilege_type='EXECUTE';
	IF owner_execute_count<>1 OR runtime_role_count>1 OR invalid_execute_count<>0 THEN
		RAISE EXCEPTION 'V18 runtime principal anchor ACL is ambiguous or unsafe'
			USING ERRCODE='42501';
	END IF;
	IF runtime_role_count=1 THEN
		EXECUTE pg_catalog.format(
			'GRANT EXECUTE ON FUNCTION decodex.read_waiting_usage_wake_transition_exact(uuid,uuid), decodex.register_waiting_usage_wake_exact(text,text,uuid,uuid,bigint), decodex.claim_due_waiting_usage_wake_exact(text,text,uuid,uuid,uuid), decodex.fire_waiting_usage_wake_exact(text,text,uuid,uuid,bigint,uuid,uuid,uuid), decodex.cancel_waiting_usage_wake_exact(text,text,uuid,uuid,bigint,uuid) TO %I',
			runtime_role);
	END IF;
END
$$;
