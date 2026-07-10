use serde_json::{self, Value};

use crate::{
	autonomy_proposal::AutonomyProposalAuthorityActorKind,
	autonomy_runtime_policy,
	config::ServiceConfig,
	mcp::{
		self, McpServer, TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY, TOOL_AUTONOMY_APPLY_RUNTIME_POLICY,
		planning::{
			self,
			autonomy::{
				args::{AutonomyAcceptRuntimePolicyToolArgs, AutonomyApplyRuntimePolicyToolArgs},
				results,
			},
		},
	},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_accept_runtime_policy_tool(
		&self,
		arguments: Value,
	) -> Value {
		let params = match serde_json::from_value::<AutonomyAcceptRuntimePolicyToolArgs>(arguments)
		{
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
					"`publicNonGoals` and explicit user `authority` are required.",
				);
			},
		};
		let mode = match planning::planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let config = match self
			.registered_runtime_policy_config(TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY, &project_id)
		{
			Ok(config) => config,
			Err(result) => return result,
		};
		let store = match planning::planning_state_store(
			&self.context,
			TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
		) {
			Ok(store) => store,
			Err(result) => return result,
		};

		if mode == "apply" {
			return mcp::tool_refusal(
				"autonomy_runtime_policy_operator_cli_required",
				"Runtime policy acceptance is unavailable over MCP; use the interactive Decodex operator CLI ceremony.",
			);
		}

		let Some(authority) = params.authority else {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
				"dry_run requires explicit user `authority` and `publicNonGoals`.",
			);
		};

		if authority.accepted_by_kind != AutonomyProposalAuthorityActorKind::User {
			return mcp::tool_refusal(
				"autonomy_runtime_policy_user_acceptance_required",
				"Runtime policy acceptance requires explicit user authority.",
			);
		}

		let accepted_by = match mcp::non_empty_string(Some(&authority.accepted_by)) {
			Some(value) => value,
			None => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
					"`authority.acceptedBy` is required.",
				);
			},
		};
		let acceptance_source = match mcp::non_empty_string(Some(&authority.acceptance_source)) {
			Some(value) => value,
			None => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_ACCEPT_RUNTIME_POLICY,
					"`authority.acceptanceSource` is required.",
				);
			},
		};
		let accepted_at = authority.accepted_at;
		let candidate = autonomy_runtime_policy::registered_policy_candidate(
			&config,
			store,
			&project_id,
			accepted_by,
			&accepted_at,
			acceptance_source,
			params.public_non_goals,
		);
		let policy = candidate.and_then(|candidate| {
			let digest = autonomy_runtime_policy::runtime_policy_candidate_digest(&candidate)?;

			Ok((candidate, digest))
		});

		match policy {
			Ok((policy, digest)) =>
				mcp::tool_success(results::autonomy_runtime_policy_acceptance_result(
					&project_id,
					&policy,
					&digest,
					mode,
					false,
				)),
			Err(_) => mcp::tool_refusal(
				"autonomy_runtime_policy_acceptance_refused",
				"Runtime policy acceptance failed closed against registered config, accepted Objective state, immutable replay, or public projection validation.",
			),
		}
	}

	pub(in crate::mcp) fn call_autonomy_apply_runtime_policy_tool(
		&self,
		arguments: Value,
	) -> Value {
		let params = match serde_json::from_value::<AutonomyApplyRuntimePolicyToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_APPLY_RUNTIME_POLICY,
					"`proposalId` and optional `mode` are required.",
				);
			},
		};
		let Some(proposal_id) = mcp::non_empty_string(Some(&params.proposal_id)) else {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_APPLY_RUNTIME_POLICY,
				"`proposalId` is required.",
			);
		};

		if !mcp::safe_autonomy_record_identifier(proposal_id) {
			return mcp::invalid_tool_arguments(
				TOOL_AUTONOMY_APPLY_RUNTIME_POLICY,
				"`proposalId` must be a safe Decodex autonomy identifier.",
			);
		}

		let mode = match planning::planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_APPLY_RUNTIME_POLICY,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_APPLY_RUNTIME_POLICY,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let config = match self
			.registered_runtime_policy_config(TOOL_AUTONOMY_APPLY_RUNTIME_POLICY, &project_id)
		{
			Ok(config) => config,
			Err(result) => return result,
		};
		let store =
			match planning::planning_state_store(&self.context, TOOL_AUTONOMY_APPLY_RUNTIME_POLICY)
			{
				Ok(store) => store,
				Err(result) => return result,
			};
		let intake_team_issue_identifier = config
			.autonomy()
			.auto_intake()
			.then(|| config.autonomy().runtime_policy())
			.flatten()
			.and_then(|policy| policy.team_issue_identifier());
		let evaluation = match autonomy_runtime_policy::evaluate_registered_policy_promotion(
			&config,
			store,
			&project_id,
			proposal_id,
		) {
			Ok(evaluation) => evaluation,
			Err(_) => {
				return mcp::tool_refusal(
					"autonomy_runtime_policy_apply_refused",
					"Runtime-policy promotion failed closed against accepted policy, Objective, proposal lineage, or existing Decision Contract authority.",
				);
			},
		};

		if mode == "dry_run" || !evaluation.objections.is_empty() {
			return mcp::tool_success(results::autonomy_runtime_policy_apply_result(
				&project_id,
				proposal_id,
				&evaluation.contract_id,
				mode,
				false,
				&evaluation.objections,
				false,
				evaluation.execution_authority_granted,
				evaluation.program_intake_present,
				evaluation.program_intake_state.as_str(),
				intake_team_issue_identifier,
			));
		}

		match autonomy_runtime_policy::apply_registered_policy_promotion(
			&config,
			store,
			&project_id,
			proposal_id,
		) {
			Ok(outcome) => mcp::tool_success(results::autonomy_runtime_policy_apply_result(
				&project_id,
				proposal_id,
				outcome.contract.contract_id(),
				mode,
				true,
				&[],
				outcome.challenge_recorded,
				true,
				outcome.program_intake_present,
				outcome.program_intake_state.as_str(),
				intake_team_issue_identifier,
			)),
			Err(_) => mcp::tool_refusal(
				"autonomy_runtime_policy_apply_refused",
				"Runtime-policy promotion failed closed while recording trusted challenge or Decision Contract authority.",
			),
		}
	}

	fn registered_runtime_policy_config(
		&self,
		tool: &str,
		project_id: &str,
	) -> Result<ServiceConfig, Value> {
		let Some(config_path) = self.context.config_path.as_deref() else {
			return Err(planning::missing_authority_refusal(
				tool,
				"Registered project config is required for runtime-policy operations.",
			));
		};
		let config = ServiceConfig::from_path(config_path).map_err(|_| {
			mcp::tool_refusal(
				"autonomy_runtime_policy_config_refused",
				"Registered project config could not be loaded.",
			)
		})?;

		if config.service_id() != project_id {
			return Err(mcp::tool_refusal(
				"autonomy_runtime_policy_config_refused",
				"Registered project config does not match the requested project.",
			));
		}

		Ok(config)
	}
}
