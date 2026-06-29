use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{self, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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
	config::ServiceConfig,
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	prelude::eyre,
	program_intake::{
		self, GoalIntakeCommandRequest, GoalIntakeIssueReport, GoalIntakeReport,
		GoalIntakeRunRequest,
	},
	research_design::{
		self, ResearchDesignOutcome, ResearchDesignRunInput, ResearchDesignRunReport,
	},
	state::StateStore,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
	},
	workflow::WorkflowDocument,
};

use super::{
	McpContext, McpServer, TOOL_AUTONOMY_ACCEPT_OBJECTIVE, TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
	TOOL_AUTONOMY_COMPILE_PROPOSAL, TOOL_AUTONOMY_DRAFT_OBJECTIVE, TOOL_AUTONOMY_REQUEST_PROMOTION,
	TOOL_AUTONOMY_SUBMIT_SIGNAL, TOOL_INTAKE_GOAL, TOOL_PLAN, TOOL_RESEARCH_COMPILE,
	TOOL_RESEARCH_PROMOTE, invalid_tool_arguments, non_empty_string,
	observability::mcp_sanitized_value,
	resources::{
		mcp_autonomy_objective_summary, mcp_autonomy_proposal_summary, mcp_autonomy_signal_summary,
	},
	safe_autonomy_record_identifier, safe_runtime_identifier, tool_refusal, tool_refusal_value,
	tool_success,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanToolArgs {
	intent: String,
	issue: Option<String>,
	contract_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResearchCompileToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	input: Option<ResearchDesignRunInput>,
	intent: Option<String>,
	source_issue: Option<String>,
	outcome: Option<ResearchDesignOutcome>,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResearchPromoteToolArgs {
	mode: Option<String>,
	project_id: Option<String>,
	contract_id: String,
	authority: Option<PlanningAuthorityArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntakeGoalToolArgs {
	mode: Option<String>,
	contract_id: String,
	team_issue_identifier: Option<String>,
	authority: Option<PlanningAuthorityArgs>,
}

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanningAuthorityArgs {
	source: Option<String>,
	reason: Option<String>,
	accepted_by: Option<String>,
	accepted_at: Option<String>,
	acceptance_source: Option<String>,
	run_id: Option<String>,
	expected_turn_id: Option<String>,
}

struct McpDryRunTracker;
impl IssueTracker for McpDryRunTracker {
	fn list_issues_with_label(
		&self,
		_label_name: &str,
	) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn find_team_label_id(
		&self,
		_team_id: &str,
		_label_name: &str,
	) -> crate::prelude::Result<Option<String>> {
		Ok(None)
	}

	fn get_issue_by_identifier(
		&self,
		_issue_identifier: &str,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		Ok(None)
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> crate::prelude::Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate issue state.")
	}

	fn add_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate labels.")
	}

	fn remove_issue_labels(
		&self,
		_issue_id: &str,
		_label_ids: &[String],
	) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not mutate labels.")
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
		eyre::bail!("MCP dry-run tracker does not create comments.")
	}

	fn create_issue(&self, _request: &TrackerIssueCreate) -> crate::prelude::Result<TrackerIssue> {
		eyre::bail!("MCP dry-run tracker does not create issues.")
	}

	fn update_issue_brief(
		&self,
		_issue_id: &str,
		_request: &TrackerIssueBriefUpdate,
	) -> crate::prelude::Result<TrackerIssue> {
		eyre::bail!("MCP dry-run tracker does not update issue briefs.")
	}
}

struct PromotionAuthority<'a> {
	accepted_by: &'a str,
	accepted_at: Option<&'a String>,
	acceptance_source: &'a str,
	reason: Option<&'a String>,
}

impl McpServer {
	pub(super) fn call_research_compile_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ResearchCompileToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_RESEARCH_COMPILE,
					"`mode` must be dry_run or apply, with either `input` or `intent`.",
				);
			},
		};
		let mode = match planning_mode(params.mode.as_deref(), "dry_run", TOOL_RESEARCH_COMPILE) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_RESEARCH_COMPILE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};

		if mode == "apply" && !planning_authority_present(params.authority.as_ref()) {
			return missing_authority_refusal(
				TOOL_RESEARCH_COMPILE,
				"research_compile apply requires authority.source and authority.reason.",
			);
		}

		let input = match research_compile_input(params) {
			Ok(input) => input,
			Err(result) => return result,
		};
		let report = if mode == "apply" {
			let store = match planning_state_store(&self.context, TOOL_RESEARCH_COMPILE) {
				Ok(store) => store,
				Err(result) => return result,
			};

			research_design::persist_research_design_run(store, &project_id, input)
		} else {
			research_design::dry_run_research_design_compile(input, &project_id)
		};

		match report {
			Ok(report) => tool_success(research_compile_result(&report, mode == "apply", mode)),
			Err(_) => tool_refusal(
				"research_compile_refused",
				"Research compile input did not satisfy Decodex Decision Contract requirements.",
			),
		}
	}

	pub(super) fn call_research_promote_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<ResearchPromoteToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_RESEARCH_PROMOTE,
					"`contractId` is required and `mode` must be dry_run or apply.",
				);
			},
		};
		let Some(contract_id) = non_empty_string(Some(params.contract_id.as_str())) else {
			return invalid_tool_arguments(TOOL_RESEARCH_PROMOTE, "`contractId` is required.");
		};

		if !safe_runtime_identifier(contract_id) {
			return invalid_tool_arguments(
				TOOL_RESEARCH_PROMOTE,
				"`contractId` must be a safe Decodex runtime identifier.",
			);
		}

		let mode = match planning_mode(params.mode.as_deref(), "dry_run", TOOL_RESEARCH_PROMOTE) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_RESEARCH_PROMOTE,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let store = match planning_state_store(&self.context, TOOL_RESEARCH_PROMOTE) {
			Ok(store) => store,
			Err(result) => return result,
		};

		if mode == "dry_run" {
			return match store.decision_contract(&project_id, contract_id) {
				Ok(Some(record)) => tool_success(research_promote_readiness_result(
					record.contract_id(),
					record.status().as_str(),
					record.contract().execution_readiness().ready_for_issue_shaping(),
					false,
					mode,
				)),
				Ok(None) => tool_refusal(
					"contract_not_found",
					"Decision Contract was not found in the current Decodex project.",
				),
				Err(_) => tool_refusal(
					"research_promote_refused",
					"Decision Contract readback failed before promotion.",
				),
			};
		}

		let authority = match promotion_authority(params.authority.as_ref()) {
			Ok(authority) => authority,
			Err(result) => return result,
		};
		let accepted_at = match authority.accepted_at {
			Some(accepted_at) => accepted_at.to_owned(),
			None => match OffsetDateTime::now_utc().format(&Rfc3339) {
				Ok(value) => value,
				Err(_) => {
					return tool_refusal(
						"research_promote_refused",
						"Promotion timestamp could not be prepared.",
					);
				},
			},
		};
		let promotion = match DecisionPromotion::new(
			authority.accepted_by,
			DecisionPromotionActorKind::User,
			accepted_at,
			authority.acceptance_source,
			authority.reason.cloned(),
		) {
			Ok(promotion) => promotion,
			Err(_) => {
				return tool_refusal(
					"research_promote_refused",
					"Promotion authority did not satisfy Decodex Decision Contract requirements.",
				);
			},
		};

		match research_design::promote_research_design_contract(
			store,
			&project_id,
			contract_id,
			promotion,
		) {
			Ok(record) => tool_success(research_promote_readiness_result(
				record.contract_id(),
				record.status().as_str(),
				record.contract().execution_readiness().ready_for_issue_shaping(),
				true,
				mode,
			)),
			Err(_) => tool_refusal(
				"research_promote_refused",
				"Decision Contract promotion was refused by Decodex authority checks.",
			),
		}
	}

	pub(super) fn call_intake_goal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<IntakeGoalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return invalid_tool_arguments(
					TOOL_INTAKE_GOAL,
					"`contractId` is required and `mode` must be dry_run or apply.",
				);
			},
		};
		let Some(contract_id) = non_empty_string(Some(params.contract_id.as_str())) else {
			return invalid_tool_arguments(TOOL_INTAKE_GOAL, "`contractId` is required.");
		};

		if !safe_runtime_identifier(contract_id) {
			return invalid_tool_arguments(
				TOOL_INTAKE_GOAL,
				"`contractId` must be a safe Decodex runtime identifier.",
			);
		}

		let mode = match planning_mode(params.mode.as_deref(), "dry_run", TOOL_INTAKE_GOAL) {
			Ok(mode) => mode,
			Err(result) => return result,
		};

		if mode == "apply" {
			if !planning_authority_present(params.authority.as_ref()) {
				return missing_authority_refusal(
					TOOL_INTAKE_GOAL,
					"intake_goal apply requires authority.source and authority.reason.",
				);
			}

			return self
				.apply_intake_goal_tool(contract_id, params.team_issue_identifier.as_deref());
		}

		let store = match planning_state_store(&self.context, TOOL_INTAKE_GOAL) {
			Ok(store) => store,
			Err(result) => return result,
		};
		let config_path = match self.context.config_path.as_deref() {
			Some(path) => path,
			None => {
				return tool_refusal(
					"missing_project_context",
					"intake_goal dry-run requires a registered Decodex project config or --config.",
				);
			},
		};
		let config = match ServiceConfig::from_path(config_path) {
			Ok(config) => config,
			Err(_) => {
				return tool_refusal(
					"missing_project_context",
					"intake_goal dry-run could not load the Decodex project config.",
				);
			},
		};
		let workflow = match WorkflowDocument::from_path(config.workflow_path()) {
			Ok(workflow) => workflow,
			Err(_) => {
				return tool_refusal(
					"missing_project_context",
					"intake_goal dry-run could not load the Decodex workflow contract.",
				);
			},
		};
		let tracker = McpDryRunTracker;

		match program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id,
			team_issue_identifier: params.team_issue_identifier,
			dry_run: true,
			apply: false,
		}) {
			Ok(report) => tool_success(intake_goal_result(&report, mode)),
			Err(_) => tool_refusal(
				"intake_goal_refused",
				"Goal intake dry-run was refused by Decodex authority checks.",
			),
		}
	}

	fn apply_intake_goal_tool(
		&self,
		contract_id: &str,
		team_issue_identifier: Option<&str>,
	) -> Value {
		let Some(config_path) = self.context.config_path.as_deref() else {
			return tool_refusal(
				"missing_project_context",
				"intake_goal apply requires a registered Decodex project config or --config.",
			);
		};

		match program_intake::run_goal_intake_command(GoalIntakeCommandRequest {
			config_path: Some(config_path),
			project_id: self.context.project_id.as_deref(),
			contract_id,
			team_issue_identifier,
			dry_run: false,
			apply: true,
		}) {
			Ok(report) => tool_success(intake_goal_result(&report, "apply")),
			Err(_) => tool_refusal(
				"intake_goal_refused",
				"Goal intake apply was refused by Decodex authority or tracker checks.",
			),
		}
	}

	pub(super) fn call_autonomy_draft_objective_tool(&self, arguments: Value) -> Value {
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

	pub(super) fn call_autonomy_accept_objective_tool(&self, arguments: Value) -> Value {
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

	pub(super) fn call_autonomy_submit_signal_tool(&self, arguments: Value) -> Value {
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

	pub(super) fn call_autonomy_compile_proposal_tool(&self, arguments: Value) -> Value {
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

	pub(super) fn call_autonomy_challenge_proposal_tool(&self, arguments: Value) -> Value {
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

	pub(super) fn call_autonomy_request_promotion_tool(&self, arguments: Value) -> Value {
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

pub(super) fn call_plan_tool(arguments: Value) -> Value {
	let params = match serde_json::from_value::<PlanToolArgs>(arguments) {
		Ok(params) => params,
		Err(_) => {
			return invalid_tool_arguments(
				TOOL_PLAN,
				"`intent` is required and must be one of research, validation_ready, handoff, or lane_control.",
			);
		},
	};

	if !matches!(
		params.intent.as_str(),
		"research" | "validation_ready" | "handoff" | "lane_control"
	) {
		return invalid_tool_arguments(
			TOOL_PLAN,
			"`intent` must be one of research, validation_ready, handoff, or lane_control.",
		);
	}

	tool_success(plan_tool_result(&params))
}

fn plan_tool_result(params: &PlanToolArgs) -> Value {
	let (prompt, resource_hint, next_action) = match params.intent.as_str() {
		"research" => (
			"decodex_research",
			"decodex://docs/spec/loop-runtime",
			"Use the research prompt and keep output latent until explicit promotion.",
		),
		"handoff" => (
			"decodex_handoff",
			"decodex://docs/spec/review-orchestration",
			"Run bounded review and repo validation before PR-backed handoff.",
		),
		"lane_control" => (
			"decodex_lane_control",
			"decodex://docs/spec/lane-control",
			"Inspect first; then call guarded MCP lane-control with explicit authority and current run/turn preconditions.",
		),
		_ => (
			"decodex_validation_ready",
			"decodex://docs/reference/build-test-run",
			"Implement locally, run targeted validation, record docs impact, and complete the phase goal.",
		),
	};

	serde_json::json!({
		"schema": "decodex.mcp.plan_result/1",
		"status": "ok",
		"intent": params.intent.as_str(),
		"prompt": prompt,
		"resource": resource_hint,
		"next_action": next_action,
		"issue": params.issue.as_deref(),
		"contract_id": params.contract_id.as_deref()
	})
}

fn research_compile_input(
	params: ResearchCompileToolArgs,
) -> Result<ResearchDesignRunInput, Value> {
	match (params.input, params.intent) {
		(Some(input), None) => Ok(input),
		(None, Some(intent)) => Ok(ResearchDesignRunInput::from_intent(
			intent,
			params.source_issue,
			params.outcome.unwrap_or(ResearchDesignOutcome::NotDecisionReady),
		)),
		(None, None) => Err(invalid_tool_arguments(
			TOOL_RESEARCH_COMPILE,
			"research_compile requires either `input` or `intent`.",
		)),
		(Some(_), Some(_)) => Err(invalid_tool_arguments(
			TOOL_RESEARCH_COMPILE,
			"research_compile accepts `input` or `intent`, not both.",
		)),
	}
}

fn research_compile_result(report: &ResearchDesignRunReport, persisted: bool, mode: &str) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.research_compile_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"contract_id": report.contract_id,
		"contract_status": report.contract_status.as_str(),
		"ready_for_issue_shaping": report.ready_for_issue_shaping,
		"issue_generation_ready_after_promotion": report.issue_generation_ready_after_promotion,
		"execution_authority_granted": report.execution_authority_granted,
		"proposed_issue_count": report.proposed_issues.len(),
		"promotion_targets": report.promotion_targets,
		"conflict_domains": report.conflict_domains,
		"next_action": if persisted {
			"Promote the Decision Contract only after explicit acceptance."
		} else {
			"Re-run with mode=apply and explicit authority to persist a latent Decision Contract."
		}
	})
}

fn research_promote_readiness_result(
	contract_id: &str,
	contract_status: &str,
	ready_for_issue_shaping: bool,
	persisted: bool,
	mode: &str,
) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.research_promote_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"contract_id": contract_id,
		"contract_status": contract_status,
		"execution_authority_granted": persisted && contract_status == "accepted_promoted",
		"ready_for_issue_shaping": ready_for_issue_shaping,
		"next_action": if persisted {
			"Use intake_goal dry_run to inspect issue shaping before apply."
		} else {
			"Re-run with mode=apply and explicit acceptance authority to promote."
		}
	})
}

fn intake_goal_result(report: &GoalIntakeReport, mode: &str) -> Value {
	let issues = report.issues.iter().map(intake_goal_issue_result).collect::<Vec<_>>();

	serde_json::json!({
		"schema": "decodex.mcp.intake_goal_result/1",
		"status": "ok",
		"mode": mode,
		"service_id": report.service_id,
		"contract_id": report.contract_id,
		"dry_run": report.dry_run,
		"applied": report.applied,
		"persisted": report.persisted,
		"issue_count": issues.len(),
		"issues": issues,
		"next_action": if report.persisted {
			"Let the Program scheduler dispatch ready mapped issues; do not add queue labels manually."
		} else {
			"Review the public issue split, then re-run with mode=apply and explicit authority if accepted."
		}
	})
}

fn intake_goal_issue_result(row: &GoalIntakeIssueReport) -> Value {
	serde_json::json!({
		"title": row.title,
		"objective": row.objective,
		"issue_identifier": row.issue_identifier,
		"action": goal_intake_action_name(row.action),
		"dependencies": row.dependencies,
		"conflict_domains": row.conflict_domains,
		"acceptance": row.acceptance,
		"validation": row.validation,
		"reasons": row.reasons
	})
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

fn goal_intake_action_name(action: program_intake::GoalIntakeIssueAction) -> &'static str {
	match action {
		program_intake::GoalIntakeIssueAction::WouldCreate => "would_create",
		program_intake::GoalIntakeIssueAction::WouldUpdate => "would_update",
		program_intake::GoalIntakeIssueAction::Created => "created",
		program_intake::GoalIntakeIssueAction::Updated => "updated",
	}
}

fn planning_mode(
	mode: Option<&str>,
	default_mode: &'static str,
	tool: &str,
) -> Result<&'static str, Value> {
	let mode = mode.map(str::trim).filter(|mode| !mode.is_empty()).unwrap_or(default_mode);

	match mode {
		"dry_run" => Ok("dry_run"),
		"apply" => Ok("apply"),
		_ => Err(invalid_tool_arguments(tool, "`mode` must be dry_run or apply.")),
	}
}

fn planning_project_id(
	context: &McpContext,
	explicit_project_id: Option<&str>,
	tool: &str,
) -> Result<String, Value> {
	let project_id = explicit_project_id
		.and_then(|value| non_empty_string(Some(value)))
		.or_else(|| context.project_id())
		.ok_or_else(|| {
			tool_refusal(
				"missing_project_context",
				"Planning tools require a project-scoped MCP context or explicit projectId.",
			)
		})?;

	if safe_runtime_identifier(project_id) {
		Ok(project_id.to_owned())
	} else {
		Err(invalid_tool_arguments(tool, "`projectId` must be a safe Decodex runtime identifier."))
	}
}

fn planning_state_store<'a>(context: &'a McpContext, _tool: &str) -> Result<&'a StateStore, Value> {
	context.state_store.as_ref().ok_or_else(|| {
		tool_refusal(
			"missing_runtime_store",
			"Planning apply/readback requires the Decodex runtime store.",
		)
	})
}

fn planning_authority_present(authority: Option<&PlanningAuthorityArgs>) -> bool {
	let Some(authority) = authority else {
		return false;
	};
	let _lane_preconditions = (
		non_empty_string(authority.run_id.as_deref()),
		non_empty_string(authority.expected_turn_id.as_deref()),
	);

	non_empty_string(authority.source.as_deref()).is_some()
		&& non_empty_string(authority.reason.as_deref()).is_some()
}

fn promotion_authority(
	authority: Option<&PlanningAuthorityArgs>,
) -> Result<PromotionAuthority<'_>, Value> {
	let Some(authority) = authority else {
		return Err(missing_authority_refusal(
			TOOL_RESEARCH_PROMOTE,
			"research_promote apply requires authority.acceptedBy and authority.acceptanceSource.",
		));
	};
	let accepted_by = non_empty_string(authority.accepted_by.as_deref()).ok_or_else(|| {
		missing_authority_refusal(
			TOOL_RESEARCH_PROMOTE,
			"research_promote apply requires authority.acceptedBy.",
		)
	})?;
	let acceptance_source =
		non_empty_string(authority.acceptance_source.as_deref()).ok_or_else(|| {
			missing_authority_refusal(
				TOOL_RESEARCH_PROMOTE,
				"research_promote apply requires authority.acceptanceSource.",
			)
		})?;

	Ok(PromotionAuthority {
		accepted_by,
		accepted_at: authority.accepted_at.as_ref(),
		acceptance_source,
		reason: authority.reason.as_ref(),
	})
}

fn missing_authority_refusal(tool: &str, message: &str) -> Value {
	tool_refusal_value(serde_json::json!({
		"schema": "decodex.mcp.refusal/1",
		"status": "refused",
		"reason": "missing_authority",
		"tool": tool,
		"message": message
	}))
}

fn mcp_now_rfc3339() -> String {
	OffsetDateTime::now_utc()
		.format(&Rfc3339)
		.unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}
