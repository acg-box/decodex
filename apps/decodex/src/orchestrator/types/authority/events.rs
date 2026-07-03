use crate::{
	orchestrator::types::{
		self, AuthorityBoundaryCheckInput, AuthorityDecisionRequestInput, Result, StateStore,
		authority::{boundary, decision_request},
	},
	state::PrivateExecutionEvent,
};

pub(crate) const AUTHORITY_DECISION_REQUEST_SCHEMA: &str = "decodex.authority_decision_request/1";
pub(crate) const AUTHORITY_DECISION_REQUEST_EVENT_TYPE: &str = "authority_decision_request";
#[allow(dead_code)]
pub(crate) const AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE: &str = "authority_boundary_check";
pub(crate) const ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE: &str = "architecture_recovery_packet";
pub(crate) const ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE: &str = "architecture_recovery_started";
pub(crate) const ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE: &str = "architecture_recovery_terminal";
pub(crate) const PHASE_GOAL_RECOVERY_EVENT_TYPE: &str = "phase_goal_recovery";
pub(crate) const PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE: &str = "phase_goal_recovery_blocked";
pub(crate) const PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT: i64 = 1;
pub(crate) const PHASE_ACCEPTANCE_CHECK_EVENT_TYPE: &str = "phase_acceptance_check";
#[allow(dead_code)]
pub(crate) const AUTHORITY_BOUNDARY_CHECK_SCHEMA: &str = "decodex.authority_boundary_check/1";
pub(crate) const ARCHITECTURE_RECOVERY_PACKET_SCHEMA: &str =
	"decodex.architecture_recovery_packet/1";

#[allow(dead_code)]
pub(crate) fn record_authority_boundary_check_private_event(
	state_store: &StateStore,
	input: AuthorityBoundaryCheckInput<'_>,
) -> Result<PrivateExecutionEvent> {
	boundary::validate_authority_boundary_check_input(&input)?;

	let changed_surfaces = input
		.changed_surfaces
		.iter()
		.map(|surface| {
			types::json!({
				"surface": surface.surface.as_str(),
				"change_summary": surface.change_summary,
				"policy_decision": surface.policy_decision.as_str(),
				"legacy_disposition": surface.legacy_disposition.as_str(),
			})
		})
		.collect::<Vec<_>>();
	let improvement_signals = input
		.improvement_signals
		.iter()
		.map(|signal| {
			types::json!({
				"kind": signal.kind,
				"reason_code": signal.reason_code,
				"target": signal.target,
				"recommendation": signal.recommendation,
			})
		})
		.collect::<Vec<_>>();
	let payload = types::json!({
		"schema": AUTHORITY_BOUNDARY_CHECK_SCHEMA,
		"record_version": 1,
		"issue": {
			"id": input.issue_id,
			"identifier": input.issue_identifier,
		},
		"run": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number,
		},
		"decision_contract_ids": input.decision_contract_ids,
		"attempted_recovery_reason": input.attempted_recovery_reason,
		"changed_surfaces": changed_surfaces,
		"policy_decision": input.policy_decision.as_str(),
		"policy": {
			"decision": input.policy_decision.as_str(),
			"allows_autonomous_recovery": input.policy_decision.allows_autonomous_recovery(),
			"requires_enhanced_evidence": input.policy_decision.requires_enhanced_evidence(),
			"blocks_landing": input.policy_decision.blocks_landing(),
		},
		"disposition": input.disposition.as_str(),
		"final_disposition": {
			"disposition": input.disposition.as_str(),
			"reason": input.final_disposition_reason,
		},
		"improvement_signals": improvement_signals,
	});

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
		payload,
	)
}

pub(crate) fn record_authority_decision_request_private_event(
	state_store: &StateStore,
	input: AuthorityDecisionRequestInput<'_>,
) -> Result<PrivateExecutionEvent> {
	decision_request::validate_authority_decision_request_input(&input)?;

	let options = input
		.options
		.iter()
		.map(|option| {
			types::json!({
				"label": option.label,
				"description": option.description,
			})
		})
		.collect::<Vec<_>>();
	let payload = types::json!({
		"schema": AUTHORITY_DECISION_REQUEST_SCHEMA,
		"record_version": 1,
		"decision_request_id": input.decision_request_id,
		"issue": {
			"id": input.issue_id,
			"identifier": input.issue_identifier,
		},
		"run": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number,
		},
		"authority_boundary_check": {
			"record_id": input.boundary_check_record_id,
			"event_type": AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
		},
		"phase": "human_required",
		"reason": input.reason_code,
		"boundary": input.boundary_type,
		"proposed_change": input.proposed_change,
		"why_exceeds_authority": input.why_exceeds_authority,
		"options": options,
		"recommendation": input.recommendation,
		"resume_condition": input.resume_condition,
		"next_action": input.resume_condition,
		"retained_worktree_evidence": input.retained_worktree_evidence,
		"retained_diff_evidence": input.retained_diff_evidence,
		"recovery_attempt_context": input.recovery_attempt_context,
	});

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		payload,
	)
}
