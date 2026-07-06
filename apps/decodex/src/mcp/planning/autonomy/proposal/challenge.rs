use serde_json::{self, Value};

use crate::mcp::{
	self, McpServer, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
	planning::{
		self,
		autonomy::{args::AutonomyChallengeProposalToolArgs, results},
	},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_challenge_proposal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomyChallengeProposalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
					"`proposalId`, `challenge`, and optional `mode` are required.",
				);
			},
		};
		let Some(proposal_id) = mcp::non_empty_string(Some(params.proposal_id.as_str())) else {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`proposalId` is required.",
			);
		};

		if !mcp::safe_autonomy_record_identifier(proposal_id) {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode = match planning::planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning::planning_project_id(
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
		let store =
			match planning::planning_state_store(&self.context, TOOL_AUTONOMY_CHALLENGE_PROPOSAL) {
				Ok(store) => store,
				Err(result) => return result,
			};

		if mode == "apply" && !planning::planning_authority_present(params.authority.as_ref()) {
			return planning::missing_authority_refusal(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"autonomy_challenge_proposal apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			let record = match store.autonomy_proposal(&project_id, proposal_id) {
				Ok(Some(record)) => record,
				Ok(None) => {
					return mcp::tool_refusal(
						"proposal_not_found",
						"Autonomy proposal was not found in the current Decodex project.",
					);
				},
				Err(error) => {
					return mcp::tool_refusal(
						"autonomy_challenge_refused",
						format!("Autonomy proposal readback failed closed: {error}"),
					);
				},
			};
			let mut proposal = record.proposal().clone();

			return match proposal.record_challenge(challenge) {
				Ok(()) => mcp::tool_success(results::autonomy_challenge_tool_result(
					&project_id,
					&proposal,
					mode,
					false,
					Some(record.updated_at()),
				)),
				Err(error) => mcp::tool_refusal(
					"autonomy_challenge_refused",
					format!("Autonomy proposal challenge was refused: {error}"),
				),
			};
		}

		match store.record_autonomy_proposal_challenge(&project_id, proposal_id, challenge) {
			Ok(record) => mcp::tool_success(results::autonomy_challenge_tool_result(
				&project_id,
				record.proposal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => mcp::tool_refusal(
				"autonomy_challenge_refused",
				format!(
					"Autonomy proposal challenge was refused by Decodex authority checks: {error}"
				),
			),
		}
	}
}
