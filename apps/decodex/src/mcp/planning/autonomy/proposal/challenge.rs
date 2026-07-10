use serde_json::{self, Value};

use crate::{
	autonomy_proposal::AutonomyProposalChallengeInput,
	autonomy_runtime_policy,
	mcp::{
		self, McpServer, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
		planning::{
			self,
			autonomy::{args::AutonomyChallengeProposalToolArgs, results},
		},
	},
	state::StateStore,
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

		if params.challenge.actor.starts_with("decodex-runtime")
			|| params
				.challenge
				.evidence_refs
				.iter()
				.any(|reference| reference.starts_with("decodex:runtime-policy-"))
		{
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"Runtime-policy challenge actor and evidence namespaces are reserved for trusted Decodex evaluation.",
			);
		}

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
			return dry_run_challenge(store, &project_id, proposal_id, challenge, mode);
		}

		let _authority_lock =
			match autonomy_runtime_policy::acquire_autonomy_project_authority_lock(&project_id) {
				Ok(lock) => lock,
				Err(_) => {
					return mcp::tool_refusal(
						"autonomy_challenge_refused",
						"Autonomy proposal challenge could not acquire the trusted authority lock.",
					);
				},
			};

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

fn dry_run_challenge(
	store: &StateStore,
	project_id: &str,
	proposal_id: &str,
	challenge: AutonomyProposalChallengeInput,
	mode: &str,
) -> Value {
	let record = match store.autonomy_proposal(project_id, proposal_id) {
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

	match proposal.record_challenge(challenge) {
		Ok(()) => mcp::tool_success(results::autonomy_challenge_tool_result(
			project_id,
			&proposal,
			mode,
			false,
			Some(record.updated_at()),
		)),
		Err(error) => mcp::tool_refusal(
			"autonomy_challenge_refused",
			format!("Autonomy proposal challenge was refused: {error}"),
		),
	}
}
