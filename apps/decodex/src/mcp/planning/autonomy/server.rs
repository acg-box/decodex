use serde_json::{self, Value};

use crate::{
	autonomy_objective::AutonomyObjectiveState,
	autonomy_signal::{AutonomySignal, AutonomySignalKind},
	mcp::{
		McpServer, TOOL_AUTONOMY_ACCEPT_OBJECTIVE, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		TOOL_AUTONOMY_COMPILE_PROPOSAL, TOOL_AUTONOMY_DRAFT_OBJECTIVE,
		TOOL_AUTONOMY_REQUEST_PROMOTION, TOOL_AUTONOMY_SUBMIT_SIGNAL, invalid_tool_arguments,
		non_empty_string, safe_autonomy_record_identifier, tool_refusal, tool_success,
	},
};

use super::{
	super::{
		missing_authority_refusal, planning_authority_present, planning_mode, planning_project_id,
		planning_state_store,
	},
	args::{
		AutonomyAcceptObjectiveToolArgs, AutonomyChallengeProposalToolArgs,
		AutonomyCompileProposalToolArgs, AutonomyDraftObjectiveToolArgs,
		AutonomyRequestPromotionToolArgs, AutonomySignalInputArgs, AutonomySubmitSignalToolArgs,
	},
	results::{
		autonomy_challenge_tool_result, autonomy_objective_accept_tool_result,
		autonomy_objective_tool_result, autonomy_promotion_request_result,
		autonomy_proposal_tool_result, autonomy_signal_tool_result,
	},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_draft_objective_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyDraftObjectiveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_DRAFT_OBJECTIVE,
					"`objective` is required and `mode` must be dry_run or apply.",
				),
		};
		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_DRAFT_OBJECTIVE) {
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_DRAFT_OBJECTIVE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};

		if params.objective.project_id() != project_id {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_DRAFT_OBJECTIVE,
				"`objective.project_id` must match the MCP project context.",
			);
		}
		if params.objective.state() != AutonomyObjectiveState::Draft {
			return tool_refusal(
				"objective_draft_refused",
				"autonomy_draft_objective only stores draft Objective Contracts; acceptance uses a separate explicit authority surface.",
			);
		}

		if let Err(error) = params.objective.validate() {
			return tool_refusal(
				"objective_draft_refused",
				format!("Objective Contract draft did not validate: {error}"),
			);
		}

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_DRAFT_OBJECTIVE,
				"autonomy_draft_objective apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_objective_tool_result(
				&project_id,
				&params.objective,
				mode,
				false,
				None,
			));
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_DRAFT_OBJECTIVE) {
			Ok(store) => store,
			Err(result) => return result,
		};

		match store.upsert_autonomy_objective_draft(&project_id, params.objective) {
			Ok(record) => tool_success(autonomy_objective_tool_result(
				&project_id,
				record.objective(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"objective_draft_refused",
				format!(
					"Objective Contract draft was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_accept_objective_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyAcceptObjectiveToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
					"`objectiveId`, `objectiveVersion`, and optional `mode` are required.",
				),
		};
		let Some(objective_id) = non_empty_string(Some(params.objective_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(objective_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveId` must be a safe Decodex autonomy identifier.",
			);
		}
		if params.objective_version == 0 {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"`objectiveVersion` must be greater than zero.",
			);
		}

		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_ACCEPT_OBJECTIVE) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let record =
			match store.autonomy_objective(&project_id, objective_id, params.objective_version) {
				Ok(Some(record)) => record,
				Ok(None) =>
					return tool_refusal(
						"objective_not_found",
						"Autonomy Objective Contract draft was not found in the current Decodex project.",
					),
				Err(error) =>
					return tool_refusal(
						"objective_acceptance_refused",
						format!("Objective Contract readback failed closed: {error}"),
					),
			};

		if record.state() != AutonomyObjectiveState::Draft {
			return tool_refusal(
				"objective_acceptance_refused",
				"Only draft Objective Contract versions can be accepted through autonomy_accept_objective.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_objective_accept_tool_result(
				&project_id,
				record.objective(),
				mode,
				false,
				Some(record.updated_at()),
			));
		}

		let Some(authority) = params.authority else {
			return missing_authority_refusal(
				TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
				"autonomy_accept_objective apply requires explicit objective acceptance authority.",
			);
		};
		let acceptance = match authority.into_acceptance() {
			Ok(acceptance) => acceptance,
			Err(result) => return result,
		};

		match store.accept_autonomy_objective_version(
			&project_id,
			objective_id,
			params.objective_version,
			acceptance,
		) {
			Ok(record) => tool_success(autonomy_objective_accept_tool_result(
				&project_id,
				record.objective(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"objective_acceptance_refused",
				format!(
					"Objective Contract acceptance was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_submit_signal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomySubmitSignalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_SUBMIT_SIGNAL,
					"`kind`, `signal`, and optional `mode` are required.",
				),
		};
		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_SUBMIT_SIGNAL) {
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let signal = match autonomy_signal_from_tool_args(params.kind, params.signal, &project_id) {
			Ok(signal) => signal,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_SUBMIT_SIGNAL,
				"autonomy_submit_signal apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			return tool_success(autonomy_signal_tool_result(
				&project_id,
				&signal,
				mode,
				false,
				None,
			));
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_SUBMIT_SIGNAL) {
			Ok(store) => store,
			Err(result) => return result,
		};

		match store.record_autonomy_signal(&project_id, signal) {
			Ok(record) => tool_success(autonomy_signal_tool_result(
				&project_id,
				record.signal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"autonomy_signal_refused",
				format!("Autonomy signal was refused by Decodex authority checks: {error}"),
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_compile_proposal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyCompileProposalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_COMPILE_PROPOSAL,
					"`proposal`, `signalIds`, and optional `mode` are required.",
				),
		};
		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let input = match params.proposal.into_compile_input(&project_id) {
			Ok(input) => input,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"autonomy_compile_proposal apply requires authority.source and authority.reason.",
			);
		}

		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_COMPILE_PROPOSAL) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let proposal = match store.compile_autonomy_proposal_dry_run(input, &params.signal_ids) {
			Ok(proposal) => proposal,
			Err(error) =>
				return tool_refusal(
					"autonomy_proposal_refused",
					format!("Autonomy proposal compile was refused: {error}"),
				),
		};

		if mode == "dry_run" {
			return tool_success(autonomy_proposal_tool_result(
				&project_id,
				&proposal,
				mode,
				false,
				None,
			));
		}

		match store.record_autonomy_proposal(&project_id, proposal) {
			Ok(record) => tool_success(autonomy_proposal_tool_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"autonomy_proposal_refused",
				format!(
					"Autonomy proposal persistence was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_challenge_proposal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyChallengeProposalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
					"`proposalId`, `challenge`, and optional `mode` are required.",
				),
		};
		let Some(proposal_id) = non_empty_string(Some(params.proposal_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`proposalId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(proposal_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode = match planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let challenge = match params.challenge.into_challenge_input() {
			Ok(challenge) => challenge,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_CHALLENGE_PROPOSAL) {
			Ok(store) => store,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"autonomy_challenge_proposal apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			let record = match store.autonomy_proposal(&project_id, proposal_id) {
				Ok(Some(record)) => record,
				Ok(None) =>
					return tool_refusal(
						"proposal_not_found",
						"Autonomy proposal was not found in the current Decodex project.",
					),
				Err(error) =>
					return tool_refusal(
						"autonomy_challenge_refused",
						format!("Autonomy proposal readback failed closed: {error}"),
					),
			};
			let mut proposal = record.proposal().clone();

			return match proposal.record_challenge(challenge) {
				Ok(()) => tool_success(autonomy_challenge_tool_result(
					&project_id,
					&proposal,
					mode,
					false,
					Some(record.updated_at()),
				)),
				Err(error) => tool_refusal(
					"autonomy_challenge_refused",
					format!("Autonomy proposal challenge was refused: {error}"),
				),
			};
		}

		match store.record_autonomy_proposal_challenge(&project_id, proposal_id, challenge) {
			Ok(record) => tool_success(autonomy_challenge_tool_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => tool_refusal(
				"autonomy_challenge_refused",
				format!(
					"Autonomy proposal challenge was refused by Decodex authority checks: {error}"
				),
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_request_promotion_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyRequestPromotionToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) =>
				return invalid_tool_arguments(
					TOOL_AUTONOMY_REQUEST_PROMOTION,
					"`proposalId` and optional `mode` are required.",
				),
		};
		let Some(proposal_id) = non_empty_string(Some(params.proposal_id.as_str())) else {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"`proposalId` is required.",
			);
		};

		if !safe_autonomy_record_identifier(proposal_id) {
			return invalid_tool_arguments(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode =
			match planning_mode(params.mode.as_deref(), "dry_run", TOOL_AUTONOMY_REQUEST_PROMOTION)
			{
				Ok(mode) => mode,
				Err(result) => return result,
			};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_REQUEST_PROMOTION,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_AUTONOMY_REQUEST_PROMOTION) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let record = match store.autonomy_proposal(&project_id, proposal_id) {
			Ok(Some(record)) => record,
			Ok(None) =>
				return tool_refusal(
					"proposal_not_found",
					"Autonomy proposal was not found in the current Decodex project.",
				),
			Err(error) =>
				return tool_refusal(
					"autonomy_promotion_refused",
					format!("Autonomy proposal readback failed closed: {error}"),
				),
		};

		if mode == "dry_run" {
			return tool_success(autonomy_promotion_request_result(
				&project_id,
				record.proposal(),
				mode,
				false,
				None,
			));
		}

		let Some(authority) = params.authority else {
			return missing_authority_refusal(
				TOOL_AUTONOMY_REQUEST_PROMOTION,
				"autonomy_request_promotion apply requires explicit proposal acceptance authority.",
			);
		};
		let authority = match authority.into_decision_bridge_authority() {
			Ok(authority) => authority,
			Err(result) => return result,
		};

		match store.accept_autonomy_proposal_as_decision_contract_candidate(
			&project_id,
			proposal_id,
			authority,
		) {
			Ok(contract) => tool_success(autonomy_promotion_request_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(contract.contract_id()),
			)),
			Err(error) => tool_refusal(
				"autonomy_promotion_refused",
				format!(
					"Autonomy proposal promotion request was refused by Decodex authority checks: {error}"
				),
			),
		}
	}
}

fn autonomy_signal_from_tool_args(
	kind: AutonomySignalKind,
	input: AutonomySignalInputArgs,
	project_id: &str,
) -> Result<AutonomySignal, Value> {
	let input = input.into_signal_input(project_id);
	let signal = match kind {
		AutonomySignalKind::RuntimeHealth => AutonomySignal::runtime_health(input),
		AutonomySignalKind::ValidationRegression => AutonomySignal::validation_regression(input),
		AutonomySignalKind::ReviewFeedbackCluster => AutonomySignal::review_feedback_cluster(input),
		AutonomySignalKind::UserFeedbackCluster => AutonomySignal::user_feedback_cluster(input),
		AutonomySignalKind::SpecDrift => AutonomySignal::spec_drift(input),
		AutonomySignalKind::ProtocolDrift => AutonomySignal::protocol_drift(input),
		AutonomySignalKind::MetricRegression => AutonomySignal::metric_regression(input),
		AutonomySignalKind::ExecutionFriction => AutonomySignal::execution_friction(input),
		AutonomySignalKind::DocsSkillDrift => AutonomySignal::docs_skill_drift(input),
	};

	signal.map_err(|error| {
		tool_refusal(
			"autonomy_signal_refused",
			format!("Autonomy signal did not satisfy Decodex signal requirements: {error}"),
		)
	})
}
