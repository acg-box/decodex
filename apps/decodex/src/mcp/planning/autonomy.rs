use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{self, Value};

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		AutonomyObjectiveState,
	},
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalKind, AutonomySignalPrivacy,
		AutonomySignalReviewEvidence, AutonomySignalSourceType,
	},
};

use super::{
	PlanningAuthorityArgs, mcp_now_rfc3339, missing_authority_refusal, planning_authority_present,
	planning_mode, planning_project_id, planning_state_store,
};
use crate::mcp::{
	McpServer, TOOL_AUTONOMY_ACCEPT_OBJECTIVE, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
	TOOL_AUTONOMY_COMPILE_PROPOSAL, TOOL_AUTONOMY_DRAFT_OBJECTIVE, TOOL_AUTONOMY_REQUEST_PROMOTION,
	TOOL_AUTONOMY_SUBMIT_SIGNAL,
	autonomy_resources::{
		mcp_autonomy_objective_summary, mcp_autonomy_proposal_summary, mcp_autonomy_signal_summary,
	},
	invalid_tool_arguments, non_empty_string,
	observability::mcp_sanitized_value,
	safe_autonomy_record_identifier, tool_refusal, tool_success,
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyDraftObjectiveToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	objective: AutonomyObjectiveContract,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyAcceptObjectiveToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	objective_id: String,
	objective_version: u64,
	authority: Option<AutonomyObjectiveAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyObjectiveAcceptanceArgs {
	accepted_by: String,
	accepted_by_kind: AutonomyObjectiveActorKind,
	accepted_at: Option<String>,
	acceptance_source: String,
}

impl AutonomyObjectiveAcceptanceArgs {
	fn into_acceptance(self) -> Result<AutonomyObjectiveAcceptance, Value> {
		if self.accepted_by_kind == AutonomyObjectiveActorKind::RuntimePolicy {
			return Err(tool_refusal(
				"objective_acceptance_refused",
				"Runtime-policy Objective Contract acceptance must be resolved from trusted Decodex authority state; caller-supplied runtime_policy acceptance fails closed.",
			));
		}

		AutonomyObjectiveAcceptance::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(mcp_now_rfc3339),
			self.acceptance_source,
		)
		.map_err(|error| {
			tool_refusal(
				"objective_acceptance_refused",
				format!("Objective Contract acceptance authority was refused: {error}"),
			)
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomySubmitSignalToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	kind: AutonomySignalKind,
	signal: AutonomySignalInputArgs,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomySignalInputArgs {
	objective_id: String,
	objective_version: u64,
	source_type: AutonomySignalSourceType,
	source_refs: Vec<String>,
	#[serde(default)]
	primary_source_refs: Vec<String>,
	issue_id: Option<String>,
	run_id: Option<String>,
	attempt_id: Option<String>,
	head_sha: Option<String>,
	captured_at: Option<String>,
	freshness: AutonomySignalFreshness,
	summary: String,
	evidence: Vec<String>,
	evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	contradictions: Vec<String>,
	#[serde(default)]
	gaps: Vec<String>,
	confidence: AutonomySignalConfidence,
	privacy: AutonomySignalPrivacy,
	#[serde(default)]
	observed_counts: BTreeMap<String, u64>,
	review_evidence: Option<AutonomySignalReviewEvidence>,
	proposal_only: Option<bool>,
	created_at: Option<String>,
}

impl AutonomySignalInputArgs {
	fn into_signal_input(self, project_id: &str) -> AutonomySignalInput {
		let now = mcp_now_rfc3339();

		AutonomySignalInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_type: self.source_type,
			source_refs: self.source_refs,
			primary_source_refs: self.primary_source_refs,
			issue_id: self.issue_id,
			run_id: self.run_id,
			attempt_id: self.attempt_id,
			head_sha: self.head_sha,
			captured_at: self.captured_at.unwrap_or_else(|| now.clone()),
			freshness: self.freshness,
			summary: self.summary,
			evidence: self.evidence,
			evidence_class: self.evidence_class,
			contradictions: self.contradictions,
			gaps: self.gaps,
			confidence: self.confidence,
			privacy: self.privacy,
			observed_counts: self.observed_counts,
			review_evidence: self.review_evidence,
			proposal_only: self.proposal_only.unwrap_or(true),
			created_at: self.created_at.unwrap_or(now),
		}
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyCompileProposalToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	proposal: AutonomyProposalCompileArgs,
	#[serde(default)]
	signal_ids: Vec<String>,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyProposalCompileArgs {
	objective_id: String,
	objective_version: u64,
	source_family: String,
	intended_surface: String,
	#[serde(default)]
	affected_identifiers: Vec<String>,
	summary: String,
	#[serde(default)]
	challenge_requirements: Vec<String>,
	#[serde(default)]
	rejected_alternatives: Vec<String>,
	rollback_path: String,
	#[serde(default)]
	weakened_validation_or_review: Vec<String>,
	created_at: Option<String>,
}

impl AutonomyProposalCompileArgs {
	fn into_compile_input(self, project_id: &str) -> Result<AutonomyProposalCompileInput, Value> {
		if self.objective_version == 0 {
			return Err(invalid_tool_arguments(
				TOOL_AUTONOMY_COMPILE_PROPOSAL,
				"`proposal.objectiveVersion` must be greater than zero.",
			));
		}

		Ok(AutonomyProposalCompileInput {
			project_id: project_id.to_owned(),
			objective_id: self.objective_id,
			objective_version: self.objective_version,
			source_family: self.source_family,
			intended_surface: self.intended_surface,
			affected_identifiers: self.affected_identifiers,
			summary: self.summary,
			challenge_requirements: self.challenge_requirements,
			rejected_alternatives: self.rejected_alternatives,
			rollback_path: self.rollback_path,
			weakened_validation_or_review: self.weakened_validation_or_review,
			created_at: self.created_at.unwrap_or_else(mcp_now_rfc3339),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyChallengeProposalToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	proposal_id: String,
	challenge: AutonomyProposalChallengeArgs,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyProposalChallengeArgs {
	source: AutonomyProposalChallengeSource,
	actor: String,
	summary: String,
	#[serde(default)]
	objections: Vec<String>,
	#[serde(default)]
	evidence_refs: Vec<String>,
	recorded_at: Option<String>,
}

impl AutonomyProposalChallengeArgs {
	fn into_challenge_input(self) -> Result<AutonomyProposalChallengeInput, Value> {
		if non_empty_string(Some(self.actor.as_str())).is_none()
			|| non_empty_string(Some(self.summary.as_str())).is_none()
		{
			return Err(invalid_tool_arguments(
				TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
				"`challenge.actor` and `challenge.summary` are required.",
			));
		}

		Ok(AutonomyProposalChallengeInput {
			source: self.source,
			actor: self.actor,
			summary: self.summary,
			objections: self.objections,
			evidence_refs: self.evidence_refs,
			recorded_at: self.recorded_at.unwrap_or_else(mcp_now_rfc3339),
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyRequestPromotionToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	proposal_id: String,
	authority: Option<AutonomyProposalAcceptanceArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutonomyProposalAcceptanceArgs {
	accepted_by: String,
	accepted_by_kind: AutonomyProposalAuthorityActorKind,
	accepted_at: Option<String>,
	acceptance_source: String,
	reason: String,
	proposal_actor: String,
	proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	accepted_project_policy: Option<Value>,
}

impl AutonomyProposalAcceptanceArgs {
	fn into_decision_bridge_authority(
		self,
	) -> Result<AutonomyProposalDecisionBridgeAuthority, Value> {
		if self.accepted_project_policy.is_some() {
			return Err(tool_refusal(
				"autonomy_policy_authority_refused",
				"acceptedProjectPolicy must be resolved from trusted Decodex authority state; MCP request payloads cannot prove accepted policy authority.",
			));
		}

		AutonomyProposalDecisionBridgeAuthority::new(
			self.accepted_by,
			self.accepted_by_kind,
			self.accepted_at.unwrap_or_else(mcp_now_rfc3339),
			self.acceptance_source,
			self.reason,
			self.proposal_actor,
			self.proposal_actor_kind,
			None,
		)
		.map_err(|error| {
			tool_refusal(
				"autonomy_acceptance_authority_refused",
				format!("Autonomy proposal acceptance authority was refused: {error}"),
			)
		})
	}
}

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

fn autonomy_objective_tool_result(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"objective": mcp_autonomy_objective_summary(objective, updated_at),
		"authority_effect": "draft_only_no_execution_authority",
		"next_action": "Accept an Objective Contract only through explicit human or accepted-policy authority; MCP profile access is not acceptance authority.",
		"updated_at": updated_at
	}))
}

fn autonomy_objective_accept_tool_result(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_objective_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"objective": mcp_autonomy_objective_summary(objective, updated_at),
		"authority_effect": "accepted_objective_no_execution_authority",
		"next_action": "Accepted Objective Contracts allow objective-bound signals and proposals; execution still requires proposal acceptance, Decision Contract promotion, and Program Intake.",
		"updated_at": updated_at
	}))
}

fn autonomy_signal_tool_result(
	project_id: &str,
	signal: &AutonomySignal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_signal_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"signal": mcp_autonomy_signal_summary(signal, updated_at),
		"authority_effect": "proposal_only_evidence_no_execution_authority",
		"next_action": "Cluster accepted-objective signals into a non-executable proposal before any Decision Contract promotion.",
		"updated_at": updated_at
	}))
}

fn autonomy_proposal_tool_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_proposal_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": mcp_autonomy_proposal_summary(proposal, updated_at),
		"authority_effect": "non_executable_proposal_evidence",
		"next_action": "Challenge the proposal and request explicit promotion authority before creating a latent Decision Contract candidate.",
		"updated_at": updated_at
	}))
}

fn autonomy_challenge_tool_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	updated_at: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_challenge_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": mcp_autonomy_proposal_summary(proposal, updated_at),
		"challenge_evidence_count": proposal.challenge_evidence().len(),
		"authority_effect": "challenge_evidence_not_acceptance_authority",
		"next_action": "Carry challenge objections as promotion constraints and request explicit promotion authority before creating execution work.",
		"updated_at": updated_at
	}))
}

fn autonomy_promotion_request_result(
	project_id: &str,
	proposal: &AutonomyProposal,
	mode: &str,
	persisted: bool,
	decision_contract_id: Option<&str>,
) -> Value {
	mcp_sanitized_value(serde_json::json!({
		"schema": "decodex.mcp.autonomy_promotion_request_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"project_id": project_id,
		"proposal": mcp_autonomy_proposal_summary(proposal, None),
		"decision_contract_id": decision_contract_id,
		"execution_authority_granted": false,
		"required_authority": [
			"acceptedBy",
			"acceptedByKind",
			"acceptanceSource",
			"reason",
			"proposalActor",
			"proposalActorKind",
			"trusted Decodex policy authority when runtime policy or external-agent self-acceptance is involved"
		],
		"authority_effect": if persisted {
			"latent_decision_contract_candidate_only"
		} else {
			"promotion_requirements_readback_only"
		},
		"next_action": if persisted {
			"Promote the resulting Decision Contract through research_promote before Program Intake or issue work."
		} else {
			"Re-run with mode=apply only after explicit proposal acceptance authority is available."
		}
	}))
}
