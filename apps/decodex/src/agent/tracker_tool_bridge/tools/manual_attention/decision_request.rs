use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		ReviewHandoffContext, TrackerToolBridge,
		tools::manual_attention::NormalizedAuthorityDecisionRequest,
	},
	orchestrator::{
		self, AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_BOUNDARY_CHECK_SCHEMA,
		AuthorityDecisionOption, AuthorityDecisionRequestInput,
	},
	state::StateStore,
};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn append_private_authority_decision_request(
		&self,
		review_context: &ReviewHandoffContext,
		state_store: &StateStore,
		decision_request: &NormalizedAuthorityDecisionRequest,
	) -> Result<(), String> {
		let boundary_events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to inspect authority boundary evidence for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let Some(boundary_event) = boundary_events
			.iter()
			.find(|event| event.record_id() == decision_request.boundary_check_id)
		else {
			return Err(format!(
				"`decision_request.boundary_check_id` {} does not reference a private event for issue `{}` run `{}` attempt {}.",
				decision_request.boundary_check_id,
				self.issue.identifier,
				review_context.run_id,
				review_context.attempt_number
			));
		};

		if !boundary_event.matches_contract(
			AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
			AUTHORITY_BOUNDARY_CHECK_SCHEMA,
			2,
		) {
			return Err(format!(
				"`decision_request.boundary_check_id` {} references `{}` instead of an authority boundary check.",
				decision_request.boundary_check_id,
				boundary_event.event_type()
			));
		}

		let disposition = boundary_event.payload().get("disposition").and_then(Value::as_str);

		if disposition != Some("requires_human") {
			return Err(format!(
				"`decision_request.boundary_check_id` {} must reference a `requires_human` authority boundary check.",
				decision_request.boundary_check_id
			));
		}

		let options = decision_request
			.options
			.iter()
			.map(|option| AuthorityDecisionOption {
				label: option.label.as_str(),
				description: option.description.as_str(),
			})
			.collect::<Vec<_>>();
		let retained_worktree_evidence = decision_request
			.retained_worktree_evidence
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();
		let retained_diff_evidence =
			decision_request.retained_diff_evidence.iter().map(String::as_str).collect::<Vec<_>>();
		let recovery_attempt_context = decision_request
			.recovery_attempt_context
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>();

		orchestrator::record_authority_decision_request_private_event(
			state_store,
			AuthorityDecisionRequestInput {
				project_id: &review_context.service_id,
				issue_id: &self.issue.id,
				issue_identifier: &self.issue.identifier,
				run_id: &review_context.run_id,
				attempt_number: review_context.attempt_number,
				boundary_check_record_id: decision_request.boundary_check_id,
				decision_request_id: &decision_request.decision_request_id,
				reason_code: &decision_request.reason_code,
				boundary_type: &decision_request.boundary_type,
				proposed_change: &decision_request.proposed_change,
				why_exceeds_authority: &decision_request.why_exceeds_authority,
				options,
				recommendation: &decision_request.recommendation,
				resume_condition: &decision_request.resume_condition,
				retained_worktree_evidence,
				retained_diff_evidence,
				recovery_attempt_context,
			},
		)
		.map(|_| ())
		.map_err(|error| {
			format!(
				"Failed to persist authority decision request `{}` for issue `{}`: {error}",
				decision_request.decision_request_id, self.issue.identifier
			)
		})
	}
}
