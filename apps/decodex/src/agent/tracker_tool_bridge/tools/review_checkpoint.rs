use std::collections::{BTreeMap, BTreeSet};

use serde_json::{self, Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	agent::tracker_tool_bridge::{
		self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, LocalRepoDetails,
		NormalizedRejectedReviewCheckpointFinding, NormalizedReviewCheckpointContract,
		NormalizedReviewCheckpointFinding, NormalizedReviewCheckpointFindingRoute,
		NormalizedReviewCheckpointPayload, NormalizedReviewCostControl,
		REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewCheckpointArgs, ReviewCheckpointChecksArgs,
		ReviewCheckpointContractArgs, ReviewCheckpointFindingArgs,
		ReviewCheckpointFindingRouteArgs, ReviewCheckpointFindingRouteCount,
		ReviewCheckpointFindingRouteSummary, ReviewCheckpointHeadBinding,
		ReviewCheckpointLineRangeArgs, ReviewCheckpointRejectedFindingArgs, ReviewCostControlArgs,
		ReviewFindingPolicyRecord, ReviewFindingPolicyState, ReviewPolicyPhase, ReviewPolicyState,
		ReviewPolicyStatus,
	},
	tracker::public_text,
};

const INDEPENDENT_FRESH_CONTEXT_REVIEWER: &str = "independent_fresh_context";
const REVIEW_CLASS_COMPACT_CURRENT_HEAD: &str = "compact_current_head_review";
const REVIEW_CLASS_FULL_CURRENT_HEAD: &str = "full_current_head_review";
const REVIEW_COST_CONTROL_NOT_PROVIDED: &str = "review_cost_control_not_provided";
const MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT: u64 = 5;
const REVIEW_ROUTE_CURRENT_BLOCKER: &str = "current_blocker";
const REVIEW_ROUTE_LANDING_BLOCKER: &str = "landing_blocker";
const REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED: &str =
	"contract_or_authority_decision_required";
const REVIEW_ROUTE_NEEDS_EVIDENCE: &str = "needs_evidence";
const REVIEW_ROUTE_FOLLOW_UP: &str = "follow_up";
const REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE: &str = "deterministic_gate_candidate";
const REVIEW_ROUTE_ARCHITECTURE_SIGNAL: &str = "architecture_signal";
const REVIEW_ROUTE_ISSUE_CONTRACT_GAP: &str = "issue_contract_gap";
const REVIEW_ROUTE_REVIEWER_RUBRIC_GAP: &str = "reviewer_rubric_gap";
const REVIEW_ROUTE_RISK_NOTE: &str = "risk_note";
const REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED: &str = "invalid_or_unsubstantiated";
const REVIEW_ROUTE_SOURCE_ACCEPTED: &str = "accepted_findings";
const REVIEW_ROUTE_SOURCE_REJECTED: &str = "rejected_findings";
const REVIEW_ROUTE_SOURCE_ROUTE_ONLY: &str = "route_only";
const REVIEW_ROUTE_RISK_HIGH: &str = "high";

pub(super) struct ReviewFindingPolicyUpdate {
	pub(super) nonclean_rounds: i64,
	pub(super) previous_nonclean_rounds: i64,
	pub(super) finding_policy: ReviewFindingPolicyState,
}

pub(super) fn review_checkpoint_reviewer_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["independent_fresh_context"]
	})
}

pub(super) fn review_checkpoint_status_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["clean", "findings", "needs_architecture_review", "blocked"]
	})
}

pub(super) fn review_checkpoint_contract_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"workflow_policy_source": {
				"type": "string",
				"enum": ["registered_project_workflow"]
			},
			"review_type": {
				"type": "string",
				"enum": ["full_current_head_review", "repair_verification"]
			},
			"risk_tier": {
				"type": "string",
				"enum": ["low", "localized", "high"]
			},
			"objective": { "type": "string" },
			"scope": non_empty_string_array_schema(),
			"non_goals": non_empty_string_array_schema(),
			"required_checks": non_empty_string_array_schema(),
			"allowed_expansion_triggers": non_empty_string_array_schema(),
			"validation_evidence": non_empty_string_array_schema()
		},
		"required": [
			"workflow_policy_source",
			"review_type",
			"risk_tier",
			"objective",
			"scope",
			"non_goals",
			"required_checks",
			"allowed_expansion_triggers",
			"validation_evidence"
		],
		"additionalProperties": false
	})
}

pub(super) fn review_cost_control_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"review_class": {
				"type": "string",
				"enum": [REVIEW_CLASS_COMPACT_CURRENT_HEAD, REVIEW_CLASS_FULL_CURRENT_HEAD]
			},
			"risk_class": {
				"type": "string",
				"enum": ["low", "localized", "high"]
			},
			"changed_surface_count": {
				"type": "integer",
				"minimum": 0
			},
			"changed_surface_summary": non_empty_string_array_schema(),
			"high_risk_surfaces": {
				"type": "array",
				"items": { "type": "string" }
			},
			"current_head_evidence": { "type": "boolean" },
			"validation_backed": { "type": "boolean" },
			"validation_current": { "type": "boolean" },
			"evidence_sufficient": { "type": "boolean" },
			"reviewer_judgment": { "type": "string" },
			"fallback_reason": { "type": "string" }
		},
		"required": [
			"review_class",
			"risk_class",
			"changed_surface_count",
			"changed_surface_summary",
			"current_head_evidence",
			"validation_backed",
			"reviewer_judgment"
		],
		"additionalProperties": false
	})
}

pub(super) fn review_checkpoint_checks_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"intended_behavior": { "type": "string" },
			"regression_risk": { "type": "string" },
			"missing_tests": { "type": "string" },
			"docs_config_drift": { "type": "string" },
			"migration_fallout": { "type": "string" },
			"operator_facing_fallout": { "type": "string" },
			"loop_decision_contract": { "type": "string" }
		},
		"required": [
			"intended_behavior",
			"regression_risk",
			"missing_tests",
			"docs_config_drift",
			"migration_fallout",
			"operator_facing_fallout",
			"loop_decision_contract"
		],
		"additionalProperties": false
	})
}

pub(super) fn review_checkpoint_findings_array_schema(rejected: bool) -> Value {
	serde_json::json!({
		"type": "array",
		"items": review_checkpoint_finding_schema(rejected)
	})
}

pub(super) fn review_checkpoint_finding_routes_schema() -> Value {
	serde_json::json!({
		"type": "array",
		"items": {
			"type": "object",
			"properties": {
				"route": review_checkpoint_finding_route_schema(),
				"severity": review_checkpoint_severity_schema(),
				"risk_tier": {
					"type": "string",
					"enum": ["low", "medium", "high"]
				},
				"summary": { "type": "string" },
				"evidence": non_empty_string_array_schema(),
				"resolver": { "type": "string" },
				"next_action": { "type": "string" },
				"finding_source": {
					"type": "string",
					"enum": [
						REVIEW_ROUTE_SOURCE_ACCEPTED,
						REVIEW_ROUTE_SOURCE_REJECTED,
						REVIEW_ROUTE_SOURCE_ROUTE_ONLY
					]
				},
				"finding_index": { "type": "integer", "minimum": 0 }
			},
			"required": ["route", "severity", "summary", "evidence", "resolver", "next_action"],
			"additionalProperties": false
		}
	})
}

fn review_checkpoint_finding_route_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": [
			REVIEW_ROUTE_CURRENT_BLOCKER,
			REVIEW_ROUTE_LANDING_BLOCKER,
			REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED,
			REVIEW_ROUTE_NEEDS_EVIDENCE,
			REVIEW_ROUTE_FOLLOW_UP,
			REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE,
			REVIEW_ROUTE_ARCHITECTURE_SIGNAL,
			REVIEW_ROUTE_ISSUE_CONTRACT_GAP,
			REVIEW_ROUTE_REVIEWER_RUBRIC_GAP,
			REVIEW_ROUTE_RISK_NOTE,
			REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED
		]
	})
}

fn review_checkpoint_finding_schema(rejected: bool) -> Value {
	let mut properties = Map::from_iter([
		(String::from("severity"), review_checkpoint_severity_schema()),
		(String::from("summary"), serde_json::json!({ "type": "string" })),
		(String::from("evidence"), non_empty_string_array_schema()),
		(String::from("kind"), serde_json::json!({ "type": "string" })),
		(String::from("file"), serde_json::json!({ "type": "string" })),
		(String::from("line"), serde_json::json!({ "type": "integer", "minimum": 1 })),
		(String::from("line_range"), review_checkpoint_line_range_schema()),
	]);
	let required = if rejected {
		properties
			.insert(String::from("rejection_reason"), serde_json::json!({ "type": "string" }));

		serde_json::json!(["severity", "summary", "rejection_reason", "evidence"])
	} else {
		properties.insert(String::from("guidance"), serde_json::json!({ "type": "string" }));

		serde_json::json!(["severity", "summary", "evidence", "guidance"])
	};

	serde_json::json!({
		"type": "object",
		"properties": properties,
		"required": required,
		"additionalProperties": false
	})
}

fn review_checkpoint_severity_schema() -> Value {
	serde_json::json!({
		"type": "string",
		"enum": ["critical", "high", "medium", "low", "info"]
	})
}

fn review_checkpoint_line_range_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"properties": {
			"start": { "type": "integer", "minimum": 1 },
			"end": { "type": "integer", "minimum": 1 }
		},
		"required": ["start", "end"],
		"additionalProperties": false
	})
}

pub(super) fn non_empty_string_array_schema() -> Value {
	serde_json::json!({
		"type": "array",
		"items": { "type": "string" },
		"minItems": 1
	})
}

pub(super) fn normalize_review_checkpoint_payload(
	parsed: ReviewCheckpointArgs,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	local_repo: &LocalRepoDetails,
) -> Result<NormalizedReviewCheckpointPayload, String> {
	let reviewer = parsed
		.reviewer
		.map(|reviewer| reviewer.trim().to_owned())
		.filter(|reviewer| !reviewer.is_empty())
		.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `reviewer` set to `{INDEPENDENT_FRESH_CONTEXT_REVIEWER}`."
			)
		})?;

	if reviewer != INDEPENDENT_FRESH_CONTEXT_REVIEWER {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` reviewer must be `{INDEPENDENT_FRESH_CONTEXT_REVIEWER}`, not `{reviewer}`."
		));
	}

	let review_contract = normalize_review_checkpoint_contract(
		parsed.review_contract.ok_or_else(|| {
			format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract`.")
		})?,
		review_policy_phase,
	)?;
	let review_contract_hash = review_checkpoint_contract_hash(&review_contract)?;
	let review_cost_control =
		normalize_review_cost_control(parsed.review_cost_control, &review_contract)?;
	let checks = normalize_review_checkpoint_checks(
		parsed
			.checks
			.ok_or_else(|| format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `checks`."))?,
	)?;
	let evidence = normalize_required_review_evidence_list(parsed.evidence, "evidence")?;
	let accepted_findings = parsed
		.accepted_findings
		.into_iter()
		.map(|finding| normalize_review_checkpoint_finding(finding, review_policy_phase))
		.collect::<Result<Vec<_>, _>>()?;
	let rejected_findings = parsed
		.rejected_findings
		.into_iter()
		.map(normalize_rejected_review_checkpoint_finding)
		.collect::<Result<Vec<_>, _>>()?;
	let finding_routes = normalize_review_checkpoint_finding_routes(
		parsed.finding_routes,
		&accepted_findings,
		&rejected_findings,
	)?;
	let finding_route_summary = summarize_review_checkpoint_finding_routes(&finding_routes);

	validate_review_cost_control_for_checkpoint(
		&review_cost_control,
		review_policy_phase,
		status,
		&review_contract,
		&accepted_findings,
		&finding_routes,
	)?;

	if status == ReviewPolicyStatus::Findings
		&& !current_review_blocker_routes(&finding_routes).any(|route| {
			route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
				&& route.finding_fingerprint.is_some()
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `findings` requires at least one accepted finding routed as `current_blocker`. Route non-current comments through `finding_routes` and use `clean` when no current repair remains.",
		));
	}
	if status == ReviewPolicyStatus::Clean && !accepted_findings.is_empty() {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` cannot include accepted findings. Reject non-actionable comments explicitly or use status `findings` for accepted repair work.",
		));
	}
	if status == ReviewPolicyStatus::Clean
		&& finding_routes.iter().any(|route| {
			route.route == REVIEW_ROUTE_CURRENT_BLOCKER || review_route_blocks_landing(route)
		}) {
		return Err(String::from(
			"`issue_review_checkpoint` status `clean` can record only non-blocking `finding_routes` such as `follow_up`, `risk_note`, `reviewer_rubric_gap`, or `invalid_or_unsubstantiated`.",
		));
	}
	if matches!(status, ReviewPolicyStatus::Blocked | ReviewPolicyStatus::NeedsArchitectureReview)
		&& !finding_routes.iter().any(review_route_blocks_landing)
	{
		return Err(String::from(
			"`issue_review_checkpoint` status `blocked` or `needs_architecture_review` requires at least one landing-blocking `finding_routes` item with evidence, resolver, and machine-actionable next_action.",
		));
	}

	Ok(NormalizedReviewCheckpointPayload {
		reviewer,
		review_contract,
		review_contract_hash,
		review_cost_control,
		reviewed_head: ReviewCheckpointHeadBinding {
			head_sha: head_sha.to_owned(),
			head_tree_oid: local_repo.head_tree_oid.clone(),
			review_worktree_clean: local_repo.review_worktree_clean(),
		},
		checks,
		evidence,
		accepted_findings,
		rejected_findings,
		finding_routes,
		finding_route_summary,
		finding_policy: empty_review_finding_policy(review_policy_phase, status, head_sha),
	})
}

fn normalize_review_checkpoint_contract(
	contract: ReviewCheckpointContractArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointContract, String> {
	let workflow_policy_source = normalize_required_review_text(
		contract.workflow_policy_source,
		"review_contract.workflow_policy_source",
	)?;

	if workflow_policy_source != "registered_project_workflow" {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.workflow_policy_source` to be `registered_project_workflow`, not `{workflow_policy_source}`."
		));
	}

	let review_type = normalize_review_type(contract.review_type, review_policy_phase)?;
	let risk_tier = normalize_review_risk_tier(contract.risk_tier)?;
	let objective =
		normalize_required_review_text(contract.objective, "review_contract.objective")?;
	let scope = normalize_required_review_contract_list(contract.scope, "review_contract.scope")?;
	let non_goals =
		normalize_required_review_contract_list(contract.non_goals, "review_contract.non_goals")?;
	let required_checks = normalize_required_review_contract_list(
		contract.required_checks,
		"review_contract.required_checks",
	)?;
	let allowed_expansion_triggers = normalize_required_review_contract_list(
		contract.allowed_expansion_triggers,
		"review_contract.allowed_expansion_triggers",
	)?;
	let validation_evidence = normalize_required_review_contract_list(
		contract.validation_evidence,
		"review_contract.validation_evidence",
	)?;

	Ok(NormalizedReviewCheckpointContract {
		workflow_policy_source,
		review_type,
		risk_tier,
		objective,
		scope,
		non_goals,
		required_checks,
		allowed_expansion_triggers,
		validation_evidence,
	})
}

fn normalize_review_type(
	review_type: String,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<String, String> {
	let review_type = review_type.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let expected = match review_policy_phase {
		ReviewPolicyPhase::Handoff => "full_current_head_review",
		ReviewPolicyPhase::Repair => "repair_verification",
	};

	if review_type != expected {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.review_type` to be `{expected}` for `{}` review checkpoints, not `{review_type}`.",
			review_policy_phase.as_str()
		));
	}

	Ok(review_type)
}

fn normalize_review_risk_tier(risk_tier: String) -> Result<String, String> {
	let risk_tier = risk_tier.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "localized" | "high" => Ok(risk_tier),
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_contract.risk_tier` to be `low`, `localized`, or `high`, not `{other}`."
		)),
	}
}

fn normalize_required_review_contract_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn review_checkpoint_contract_hash(
	contract: &NormalizedReviewCheckpointContract,
) -> Result<String, String> {
	let serialized = serde_json::to_vec(contract).map_err(|error| {
		format!(
			"Failed to serialize `review_contract` for `{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}`: {error}"
		)
	})?;
	let digest = Sha256::digest(serialized);
	let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	Ok(format!("review_contract:{hash}"))
}

fn normalize_review_cost_control(
	cost_control: Option<ReviewCostControlArgs>,
	review_contract: &NormalizedReviewCheckpointContract,
) -> Result<NormalizedReviewCostControl, String> {
	let Some(cost_control) = cost_control else {
		return Ok(NormalizedReviewCostControl {
			review_class: String::from(REVIEW_CLASS_FULL_CURRENT_HEAD),
			risk_class: review_contract.risk_tier.clone(),
			compact_eligible: false,
			changed_surface_count: 0,
			changed_surface_summary: vec![String::from(
				"Review cost-control metadata was not supplied; standard full review remains required.",
			)],
			high_risk_surfaces: Vec::new(),
			current_head_evidence: false,
			validation_backed: false,
			validation_current: false,
			evidence_sufficient: false,
			reviewer_judgment: String::from(
				"No compact-review judgment was recorded; defaulting to full independent review.",
			),
			fallback_reason: Some(String::from(REVIEW_COST_CONTROL_NOT_PROVIDED)),
		});
	};
	let review_class = normalize_review_class(cost_control.review_class)?;
	let risk_class = normalize_review_risk_tier(cost_control.risk_class)?;
	let changed_surface_summary = normalize_review_cost_control_list(
		cost_control.changed_surface_summary,
		"review_cost_control.changed_surface_summary",
		true,
	)?;
	let high_risk_surfaces = normalize_review_cost_control_list(
		cost_control.high_risk_surfaces,
		"review_cost_control.high_risk_surfaces",
		false,
	)?;
	let reviewer_judgment = normalize_public_review_cost_control_text(
		cost_control.reviewer_judgment,
		"review_cost_control.reviewer_judgment",
	)?;
	let fallback_reason = normalize_optional_public_review_cost_control_reason(
		cost_control.fallback_reason,
		"review_cost_control.fallback_reason",
	)?;
	let compact_eligible = review_class == REVIEW_CLASS_COMPACT_CURRENT_HEAD;

	if !compact_eligible && fallback_reason.is_none() {
		return Err(String::from(
			"`issue_review_checkpoint` requires `review_cost_control.fallback_reason` when `review_class` is `full_current_head_review`.",
		));
	}

	Ok(NormalizedReviewCostControl {
		review_class,
		risk_class,
		compact_eligible,
		changed_surface_count: cost_control.changed_surface_count,
		changed_surface_summary,
		high_risk_surfaces,
		current_head_evidence: cost_control.current_head_evidence,
		validation_backed: cost_control.validation_backed,
		validation_current: cost_control.validation_current,
		evidence_sufficient: cost_control.evidence_sufficient,
		reviewer_judgment,
		fallback_reason,
	})
}

fn normalize_review_class(review_class: String) -> Result<String, String> {
	let review_class = review_class.trim().to_ascii_lowercase().replace([' ', '-'], "_");
	let review_class = match review_class.as_str() {
		"compact" => REVIEW_CLASS_COMPACT_CURRENT_HEAD,
		"full" | "standard" => REVIEW_CLASS_FULL_CURRENT_HEAD,
		other => other,
	};

	match review_class {
		REVIEW_CLASS_COMPACT_CURRENT_HEAD | REVIEW_CLASS_FULL_CURRENT_HEAD => {
			Ok(review_class.to_owned())
		},
		other => Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `review_cost_control.review_class` to be `{REVIEW_CLASS_COMPACT_CURRENT_HEAD}` or `{REVIEW_CLASS_FULL_CURRENT_HEAD}`, not `{other}`."
		)),
	}
}

fn normalize_review_cost_control_list(
	values: Vec<String>,
	field_name: &str,
	required: bool,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values)
		.into_iter()
		.map(|value| normalize_public_review_cost_control_text(value, field_name))
		.collect::<Result<Vec<_>, _>>()?;

	if required && values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_public_review_cost_control_text(
	value: String,
	field_name: &str,
) -> Result<String, String> {
	let value = normalize_required_review_text(value, field_name)?;

	public_text::validate_public_text_field(field_name, &value)
		.map_err(|error| error.to_string())?;

	Ok(value)
}

fn normalize_optional_public_review_cost_control_reason(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(value) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let value = normalize_public_review_cost_control_text(value, field_name)?;

	Ok(Some(value))
}

fn validate_review_cost_control_for_checkpoint(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Result<(), String> {
	if cost_control.review_class == REVIEW_CLASS_FULL_CURRENT_HEAD {
		return Ok(());
	}

	let mut forced_full_reasons = compact_review_forced_full_reasons(
		cost_control,
		review_policy_phase,
		status,
		review_contract,
		accepted_findings,
		finding_routes,
	);

	if forced_full_reasons.is_empty() {
		return Ok(());
	}

	forced_full_reasons.sort();
	forced_full_reasons.dedup();

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {REVIEW_CLASS_COMPACT_CURRENT_HEAD}` because full review is required: {}.",
		forced_full_reasons.join(", ")
	))
}

fn compact_review_forced_full_reasons(
	cost_control: &NormalizedReviewCostControl,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	review_contract: &NormalizedReviewCheckpointContract,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	finding_routes: &[NormalizedReviewCheckpointFindingRoute],
) -> Vec<&'static str> {
	let mut reasons = Vec::new();

	if review_policy_phase != ReviewPolicyPhase::Handoff {
		reasons.push("repair_review_phase");
	}
	if status != ReviewPolicyStatus::Clean {
		reasons.push("nonclean_review_status");
	}
	if review_contract.risk_tier != "low" {
		reasons.push("review_contract_risk_tier_not_low");
	}
	if cost_control.risk_class != "low" {
		reasons.push("review_cost_risk_class_not_low");
	}
	if cost_control.changed_surface_count == 0 {
		reasons.push("missing_changed_surface_count");
	}
	if cost_control.changed_surface_count > MAX_COMPACT_REVIEW_CHANGED_SURFACE_COUNT {
		reasons.push("changed_surface_count_exceeds_compact_limit");
	}
	if !cost_control.high_risk_surfaces.is_empty() {
		reasons.push("high_risk_surfaces_present");
	}
	if !cost_control.current_head_evidence {
		reasons.push("missing_current_head_evidence");
	}
	if !cost_control.validation_backed {
		reasons.push("missing_validation_evidence");
	}
	if !cost_control.validation_current {
		reasons.push("stale_validation_evidence");
	}
	if !cost_control.evidence_sufficient {
		reasons.push("weak_evidence");
	}
	if !accepted_findings.is_empty() {
		reasons.push("accepted_findings_present");
	}
	if finding_routes.iter().any(|route| {
		route.route == REVIEW_ROUTE_CURRENT_BLOCKER || review_route_blocks_landing(route)
	}) {
		reasons.push("blocking_finding_routes_present");
	}

	reasons
}

pub(super) fn validate_review_cost_control_policy_state(
	cost_control: &NormalizedReviewCostControl,
	policy_update: &ReviewFindingPolicyUpdate,
) -> Result<(), String> {
	if cost_control.review_class != REVIEW_CLASS_COMPACT_CURRENT_HEAD
		|| policy_update.previous_nonclean_rounds == 0
	{
		return Ok(());
	}

	Err(format!(
		"`issue_review_checkpoint` cannot record `review_cost_control.review_class = {REVIEW_CLASS_COMPACT_CURRENT_HEAD}` because full review is required: prior_nonclean_review_rounds_present."
	))
}

fn normalize_review_checkpoint_checks(
	checks: ReviewCheckpointChecksArgs,
) -> Result<ReviewCheckpointChecksArgs, String> {
	Ok(ReviewCheckpointChecksArgs {
		intended_behavior: normalize_required_review_text(
			checks.intended_behavior,
			"checks.intended_behavior",
		)?,
		regression_risk: normalize_required_review_text(
			checks.regression_risk,
			"checks.regression_risk",
		)?,
		missing_tests: normalize_required_review_text(
			checks.missing_tests,
			"checks.missing_tests",
		)?,
		docs_config_drift: normalize_required_review_text(
			checks.docs_config_drift,
			"checks.docs_config_drift",
		)?,
		migration_fallout: normalize_required_review_text(
			checks.migration_fallout,
			"checks.migration_fallout",
		)?,
		operator_facing_fallout: normalize_required_review_text(
			checks.operator_facing_fallout,
			"checks.operator_facing_fallout",
		)?,
		loop_decision_contract: normalize_required_review_text(
			checks.loop_decision_contract,
			"checks.loop_decision_contract",
		)?,
	})
}

fn normalize_review_checkpoint_finding(
	finding: ReviewCheckpointFindingArgs,
	review_policy_phase: ReviewPolicyPhase,
) -> Result<NormalizedReviewCheckpointFinding, String> {
	let severity = normalize_review_severity(finding.severity, "accepted_findings.severity")?;
	let summary = normalize_required_review_text(finding.summary, "accepted_findings.summary")?;
	let guidance = normalize_required_review_text(finding.guidance, "accepted_findings.guidance")?;
	let kind = normalize_optional_review_kind(finding.kind, "accepted_findings.kind")?
		.unwrap_or_else(|| String::from("accepted_finding"));
	let file = normalize_optional_review_file(finding.file)?;
	let line = normalize_optional_review_line(finding.line)?;
	let line_range = normalize_optional_review_line_range(
		line,
		finding.line_range,
		"accepted_findings.line_range",
	)?;
	let fingerprint = review_finding_fingerprint(
		review_policy_phase,
		&kind,
		&summary,
		&guidance,
		file.as_deref(),
		line_range.as_ref(),
	);

	Ok(NormalizedReviewCheckpointFinding {
		severity,
		summary,
		evidence: normalize_required_review_evidence_list(
			finding.evidence,
			"accepted_findings.evidence",
		)?,
		kind,
		file,
		line,
		line_range,
		guidance,
		fingerprint,
	})
}

fn normalize_rejected_review_checkpoint_finding(
	finding: ReviewCheckpointRejectedFindingArgs,
) -> Result<NormalizedRejectedReviewCheckpointFinding, String> {
	let severity = normalize_review_severity(finding.severity, "rejected_findings.severity")?;
	let summary = normalize_required_review_text(finding.summary, "rejected_findings.summary")?;
	let rejection_reason = normalize_required_review_text(
		finding.rejection_reason,
		"rejected_findings.rejection_reason",
	)?;
	let kind = normalize_optional_review_kind(finding.kind, "rejected_findings.kind")?
		.unwrap_or_else(|| String::from("rejected_finding"));
	let file = normalize_optional_review_file(finding.file)?;
	let line = normalize_optional_review_line(finding.line)?;
	let line_range = normalize_optional_review_line_range(
		line,
		finding.line_range,
		"rejected_findings.line_range",
	)?;

	Ok(NormalizedRejectedReviewCheckpointFinding {
		severity,
		summary,
		rejection_reason,
		evidence: normalize_required_review_evidence_list(
			finding.evidence,
			"rejected_findings.evidence",
		)?,
		kind,
		file,
		line,
		line_range,
	})
}

fn normalize_review_checkpoint_finding_routes(
	explicit_routes: Vec<ReviewCheckpointFindingRouteArgs>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<Vec<NormalizedReviewCheckpointFindingRoute>, String> {
	let mut routes = Vec::new();
	let mut explicitly_routed_accepted = BTreeSet::new();
	let mut explicitly_routed_rejected = BTreeSet::new();

	for route in explicit_routes {
		let route =
			normalize_review_checkpoint_finding_route(route, accepted_findings, rejected_findings)?;

		if route.finding_source == REVIEW_ROUTE_SOURCE_ACCEPTED
			&& let Some(index) = route.finding_index
		{
			explicitly_routed_accepted.insert(index);
		} else if route.finding_source == REVIEW_ROUTE_SOURCE_REJECTED
			&& let Some(index) = route.finding_index
		{
			explicitly_routed_rejected.insert(index);
		}

		routes.push(route);
	}
	for (index, finding) in accepted_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize accepted finding route index: {error}")
		})?;

		if !explicitly_routed_accepted.contains(&index) {
			routes.push(default_current_blocker_route(index, finding));
		}
	}
	for (index, finding) in rejected_findings.iter().enumerate() {
		let index = u64::try_from(index).map_err(|error| {
			format!("Failed to normalize rejected finding route index: {error}")
		})?;

		if !explicitly_routed_rejected.contains(&index) {
			routes.push(default_reviewer_rubric_gap_route(index, finding));
		}
	}

	Ok(routes)
}

fn normalize_review_checkpoint_finding_route(
	route: ReviewCheckpointFindingRouteArgs,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<NormalizedReviewCheckpointFindingRoute, String> {
	let route_name = normalize_review_finding_route_name(route.route)?;
	let severity = normalize_review_severity(route.severity, "finding_routes.severity")?;
	let risk_tier = normalize_review_route_risk_tier(route.risk_tier)?;
	let summary = normalize_required_review_text(route.summary, "finding_routes.summary")?;
	let evidence =
		normalize_required_review_evidence_list(route.evidence, "finding_routes.evidence")?;
	let resolver = normalize_required_review_text(route.resolver, "finding_routes.resolver")?;
	let next_action =
		normalize_required_review_text(route.next_action, "finding_routes.next_action")?;
	let finding_source = normalize_review_route_source(route.finding_source)?;
	let (finding_index, finding_fingerprint) = normalize_review_route_binding(
		&finding_source,
		route.finding_index,
		accepted_findings,
		rejected_findings,
	)?;
	let bound_finding_high_severity = review_route_bound_finding_severity(
		&finding_source,
		finding_index,
		accepted_findings,
		rejected_findings,
	)
	.is_some_and(review_severity_blocks_invalid_route);

	if route_name == REVIEW_ROUTE_CURRENT_BLOCKER
		&& (finding_source != REVIEW_ROUTE_SOURCE_ACCEPTED || finding_fingerprint.is_none())
	{
		return Err(String::from(
			"`finding_routes.route` `current_blocker` must bind to an `accepted_findings` item with `finding_index`.",
		));
	}
	if route_name == REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED
		&& (review_severity_blocks_invalid_route(severity.as_str())
			|| bound_finding_high_severity
			|| risk_tier == REVIEW_ROUTE_RISK_HIGH)
	{
		return Err(String::from(
			"`issue_review_checkpoint` cannot route high-severity or high-risk `finding_routes` items to `invalid_or_unsubstantiated`; use `needs_evidence` or a landing-blocking route.",
		));
	}

	Ok(NormalizedReviewCheckpointFindingRoute {
		route: route_name,
		severity,
		risk_tier,
		summary,
		evidence,
		resolver,
		next_action,
		finding_source,
		finding_index,
		finding_fingerprint,
	})
}

fn default_current_blocker_route(
	index: u64,
	finding: &NormalizedReviewCheckpointFinding,
) -> NormalizedReviewCheckpointFindingRoute {
	NormalizedReviewCheckpointFindingRoute {
		route: String::from(REVIEW_ROUTE_CURRENT_BLOCKER),
		severity: finding.severity.clone(),
		risk_tier: String::from("medium"),
		summary: finding.summary.clone(),
		evidence: finding.evidence.clone(),
		resolver: String::from("agent"),
		next_action: finding.guidance.clone(),
		finding_source: String::from(REVIEW_ROUTE_SOURCE_ACCEPTED),
		finding_index: Some(index),
		finding_fingerprint: Some(finding.fingerprint.clone()),
	}
}

fn default_reviewer_rubric_gap_route(
	index: u64,
	finding: &NormalizedRejectedReviewCheckpointFinding,
) -> NormalizedReviewCheckpointFindingRoute {
	NormalizedReviewCheckpointFindingRoute {
		route: String::from(REVIEW_ROUTE_REVIEWER_RUBRIC_GAP),
		severity: finding.severity.clone(),
		risk_tier: String::from("low"),
		summary: finding.summary.clone(),
		evidence: finding.evidence.clone(),
		resolver: String::from("reviewer"),
		next_action: finding.rejection_reason.clone(),
		finding_source: String::from(REVIEW_ROUTE_SOURCE_REJECTED),
		finding_index: Some(index),
		finding_fingerprint: None,
	}
}

fn normalize_review_finding_route_name(route: String) -> Result<String, String> {
	let route = route.trim().to_ascii_lowercase().replace([' ', '-'], "_");

	match route.as_str() {
		REVIEW_ROUTE_CURRENT_BLOCKER
		| REVIEW_ROUTE_LANDING_BLOCKER
		| REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED
		| REVIEW_ROUTE_NEEDS_EVIDENCE
		| REVIEW_ROUTE_FOLLOW_UP
		| REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE
		| REVIEW_ROUTE_ARCHITECTURE_SIGNAL
		| REVIEW_ROUTE_ISSUE_CONTRACT_GAP
		| REVIEW_ROUTE_REVIEWER_RUBRIC_GAP
		| REVIEW_ROUTE_RISK_NOTE
		| REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED => Ok(route),
		other => Err(format!(
			"`finding_routes.route` must be one of the supported Decodex Review route taxonomy values, not `{other}`."
		)),
	}
}

fn normalize_review_route_risk_tier(risk_tier: Option<String>) -> Result<String, String> {
	let Some(risk_tier) = tracker_tool_bridge::normalize_optional_progress_field(risk_tier) else {
		return Ok(String::from("low"));
	};
	let risk_tier = risk_tier.to_ascii_lowercase().replace([' ', '-'], "_");

	match risk_tier.as_str() {
		"low" | "medium" | REVIEW_ROUTE_RISK_HIGH => Ok(risk_tier),
		other => Err(format!(
			"`finding_routes.risk_tier` must be `low`, `medium`, or `high`, not `{other}`."
		)),
	}
}

fn normalize_review_route_source(source: Option<String>) -> Result<String, String> {
	let Some(source) = tracker_tool_bridge::normalize_optional_progress_field(source) else {
		return Ok(String::from(REVIEW_ROUTE_SOURCE_ROUTE_ONLY));
	};
	let source = source.to_ascii_lowercase().replace([' ', '-'], "_");

	match source.as_str() {
		REVIEW_ROUTE_SOURCE_ACCEPTED
		| REVIEW_ROUTE_SOURCE_REJECTED
		| REVIEW_ROUTE_SOURCE_ROUTE_ONLY => Ok(source),
		other => Err(format!(
			"`finding_routes.finding_source` must be `accepted_findings`, `rejected_findings`, or `route_only`, not `{other}`."
		)),
	}
}

fn normalize_review_route_binding(
	source: &str,
	index: Option<u64>,
	accepted_findings: &[NormalizedReviewCheckpointFinding],
	rejected_findings: &[NormalizedRejectedReviewCheckpointFinding],
) -> Result<(Option<u64>, Option<String>), String> {
	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `accepted_findings`.",
				)
			})?;
			let finding = accepted_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize accepted finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any accepted finding."
					)
				})?;

			Ok((Some(index), Some(finding.fingerprint.clone())))
		},
		REVIEW_ROUTE_SOURCE_REJECTED => {
			let index = index.ok_or_else(|| {
				String::from(
					"`finding_routes.finding_index` is required when `finding_source` is `rejected_findings`.",
				)
			})?;

			rejected_findings
				.get(usize::try_from(index).map_err(|error| {
					format!("Failed to normalize rejected finding route index: {error}")
				})?)
				.ok_or_else(|| {
					format!(
						"`finding_routes.finding_index` `{index}` does not match any rejected finding."
					)
				})?;

			Ok((Some(index), None))
		},
		REVIEW_ROUTE_SOURCE_ROUTE_ONLY => {
			if index.is_some() {
				return Err(String::from(
					"`finding_routes.finding_index` is only valid with `accepted_findings` or `rejected_findings` sources.",
				));
			}

			Ok((None, None))
		},
		_ => Err(String::from(
			"`finding_routes.finding_source` did not normalize to a supported source.",
		)),
	}
}

fn review_route_bound_finding_severity<'a>(
	source: &str,
	index: Option<u64>,
	accepted_findings: &'a [NormalizedReviewCheckpointFinding],
	rejected_findings: &'a [NormalizedRejectedReviewCheckpointFinding],
) -> Option<&'a str> {
	let index = usize::try_from(index?).ok()?;

	match source {
		REVIEW_ROUTE_SOURCE_ACCEPTED => {
			accepted_findings.get(index).map(|finding| finding.severity.as_str())
		},
		REVIEW_ROUTE_SOURCE_REJECTED => {
			rejected_findings.get(index).map(|finding| finding.severity.as_str())
		},
		_ => None,
	}
}

fn review_severity_blocks_invalid_route(severity: &str) -> bool {
	matches!(severity, "critical" | "high")
}

fn summarize_review_checkpoint_finding_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> ReviewCheckpointFindingRouteSummary {
	let mut counts = BTreeMap::<String, usize>::new();

	for route in routes {
		*counts.entry(route.route.clone()).or_default() += 1;
	}

	ReviewCheckpointFindingRouteSummary {
		route_counts: counts
			.into_iter()
			.map(|(route, count)| ReviewCheckpointFindingRouteCount { route, count })
			.collect(),
		next_action: review_route_next_action(routes),
	}
}

fn review_route_next_action(routes: &[NormalizedReviewCheckpointFindingRoute]) -> Option<String> {
	routes
		.iter()
		.min_by_key(|route| review_route_priority(&route.route))
		.map(|route| route.next_action.clone())
}

fn review_route_priority(route: &str) -> u8 {
	match route {
		REVIEW_ROUTE_CURRENT_BLOCKER => 0,
		REVIEW_ROUTE_LANDING_BLOCKER => 1,
		REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED => 2,
		REVIEW_ROUTE_NEEDS_EVIDENCE => 3,
		REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE => 4,
		REVIEW_ROUTE_ARCHITECTURE_SIGNAL => 5,
		REVIEW_ROUTE_ISSUE_CONTRACT_GAP => 6,
		REVIEW_ROUTE_FOLLOW_UP => 7,
		REVIEW_ROUTE_RISK_NOTE => 8,
		REVIEW_ROUTE_REVIEWER_RUBRIC_GAP => 9,
		REVIEW_ROUTE_INVALID_OR_UNSUBSTANTIATED => 10,
		_ => u8::MAX,
	}
}

fn current_review_blocker_routes(
	routes: &[NormalizedReviewCheckpointFindingRoute],
) -> impl Iterator<Item = &NormalizedReviewCheckpointFindingRoute> {
	routes.iter().filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
}

pub(super) fn current_review_blocker_findings(
	payload: &NormalizedReviewCheckpointPayload,
) -> impl Iterator<Item = &NormalizedReviewCheckpointFinding> {
	let fingerprints = current_review_blocker_routes(&payload.finding_routes)
		.filter_map(|route| route.finding_fingerprint.clone())
		.collect::<BTreeSet<_>>();

	payload
		.accepted_findings
		.iter()
		.filter(move |finding| fingerprints.contains(&finding.fingerprint))
}

fn review_route_blocks_landing(route: &NormalizedReviewCheckpointFindingRoute) -> bool {
	matches!(
		route.route.as_str(),
		REVIEW_ROUTE_LANDING_BLOCKER
			| REVIEW_ROUTE_CONTRACT_OR_AUTHORITY_DECISION_REQUIRED
			| REVIEW_ROUTE_NEEDS_EVIDENCE
			| REVIEW_ROUTE_DETERMINISTIC_GATE_CANDIDATE
			| REVIEW_ROUTE_ARCHITECTURE_SIGNAL
			| REVIEW_ROUTE_ISSUE_CONTRACT_GAP
	)
}

fn normalize_review_severity(severity: String, field_name: &str) -> Result<String, String> {
	let severity = severity.trim().to_ascii_lowercase();

	match severity.as_str() {
		"critical" | "high" | "medium" | "low" | "info" => Ok(severity),
		other => Err(format!(
			"`{field_name}` must be `critical`, `high`, `medium`, `low`, or `info`, not `{other}`."
		)),
	}
}

fn normalize_required_review_text(value: String, field_name: &str) -> Result<String, String> {
	let value = tracker_tool_bridge::normalize_summary(&value);

	if value.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(value)
}

fn normalize_required_review_evidence_list(
	values: Vec<String>,
	field_name: &str,
) -> Result<Vec<String>, String> {
	let values = tracker_tool_bridge::normalize_progress_list(values);

	if values.is_empty() {
		return Err(format!("`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}`."));
	}

	Ok(values)
}

fn normalize_optional_review_file(value: Option<String>) -> Result<Option<String>, String> {
	let Some(file) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};

	if file.starts_with('/') {
		return Err(String::from(
			"`issue_review_checkpoint` file references must be repository-relative paths.",
		));
	}

	Ok(Some(file))
}

fn normalize_optional_review_line(value: Option<u64>) -> Result<Option<u64>, String> {
	if matches!(value, Some(0)) {
		return Err(String::from(
			"`issue_review_checkpoint` line references must be one-based when supplied.",
		));
	}

	Ok(value)
}

fn normalize_optional_review_line_range(
	line: Option<u64>,
	line_range: Option<ReviewCheckpointLineRangeArgs>,
	field_name: &str,
) -> Result<Option<ReviewCheckpointLineRangeArgs>, String> {
	let Some(line_range) = line_range
		.or_else(|| line.map(|line| ReviewCheckpointLineRangeArgs { start: line, end: line }))
	else {
		return Ok(None);
	};

	if line_range.start == 0 || line_range.end == 0 {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}` to use one-based line numbers."
		));
	}
	if line_range.end < line_range.start {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}.end` to be greater than or equal to `{field_name}.start`."
		));
	}

	if let Some(line) = line
		&& (line < line_range.start || line > line_range.end)
	{
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `line` to fall inside `{field_name}` when both are supplied."
		));
	}

	Ok(Some(line_range))
}

fn normalize_optional_review_kind(
	value: Option<String>,
	field_name: &str,
) -> Result<Option<String>, String> {
	let Some(kind) = tracker_tool_bridge::normalize_optional_progress_field(value) else {
		return Ok(None);
	};
	let kind = kind.to_ascii_lowercase().replace([' ', '-'], "_");
	let mut chars = kind.chars();
	let Some(first) = chars.next() else {
		return Ok(None);
	};

	if !first.is_ascii_lowercase()
		|| !chars.all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		}) {
		return Err(format!(
			"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires `{field_name}` to be a public snake_case identifier."
		));
	}

	Ok(Some(kind))
}

fn empty_review_finding_policy(
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
) -> ReviewFindingPolicyState {
	ReviewFindingPolicyState {
		schema: String::from("decodex.review_finding_policy/1"),
		phase: review_policy_phase.as_str().to_owned(),
		status: status.as_str().to_owned(),
		head_sha: head_sha.to_owned(),
		nonclean_rounds: 0,
		active_fingerprints: Vec::new(),
		stop_fingerprint: None,
		findings: Vec::new(),
	}
}

fn review_finding_fingerprint(
	review_policy_phase: ReviewPolicyPhase,
	kind: &str,
	title: &str,
	body: &str,
	file: Option<&str>,
	line_range: Option<&ReviewCheckpointLineRangeArgs>,
) -> String {
	let line_range = line_range
		.map_or_else(|| String::from("none"), |range| format!("{}-{}", range.start, range.end));
	let input = [
		("phase", review_policy_phase.as_str()),
		("kind", kind),
		("title", title),
		("body", body),
		("file", file.unwrap_or("none")),
		("line_range", line_range.as_str()),
	]
	.into_iter()
	.map(|(key, value)| format!("{key}={value}"))
	.collect::<Vec<_>>()
	.join("\n");
	let digest = Sha256::digest(input.as_bytes());
	let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

	format!("review_finding:{hash}")
}

pub(super) fn review_finding_policy_update(
	previous: ReviewFindingPolicyState,
	previous_nonclean_rounds: i64,
	review_policy_phase: ReviewPolicyPhase,
	status: ReviewPolicyStatus,
	head_sha: &str,
	checkpoint_payload: &NormalizedReviewCheckpointPayload,
) -> ReviewFindingPolicyUpdate {
	let active_fingerprints = checkpoint_payload
		.finding_routes
		.iter()
		.filter(|route| route.route == REVIEW_ROUTE_CURRENT_BLOCKER)
		.filter_map(|route| route.finding_fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let current_blocker_findings = current_review_blocker_findings(checkpoint_payload)
		.map(|finding| finding.fingerprint.clone())
		.collect::<BTreeSet<_>>();
	let mut records = previous
		.findings
		.into_iter()
		.map(|record| (record.fingerprint.clone(), record))
		.collect::<BTreeMap<_, _>>();

	match status {
		ReviewPolicyStatus::Findings => {
			for finding in current_review_blocker_findings(checkpoint_payload) {
				upsert_open_review_finding_record(
					&mut records,
					finding,
					head_sha,
					&checkpoint_payload.evidence,
				);
			}

			resolve_absent_review_findings(&mut records, &active_fingerprints);
		},
		ReviewPolicyStatus::Clean => {
			resolve_all_review_findings(&mut records, &checkpoint_payload.evidence);
		},
		ReviewPolicyStatus::NeedsArchitectureReview | ReviewPolicyStatus::Blocked => {},
	}

	let nonclean_rounds = if status == ReviewPolicyStatus::Findings {
		current_blocker_findings
			.iter()
			.filter_map(|fingerprint| records.get(fingerprint))
			.map(|record| record.repeat_count)
			.max()
			.unwrap_or_default()
	} else {
		0
	};
	let stop_fingerprint = current_blocker_findings
		.iter()
		.filter_map(|fingerprint| records.get(fingerprint).map(|record| (fingerprint, record)))
		.filter(|(_fingerprint, record)| record.repeat_count >= REVIEW_POLICY_CONVERGENCE_BUDGET)
		.max_by_key(|(_fingerprint, record)| record.repeat_count)
		.map(|(fingerprint, _record)| fingerprint.clone());
	let mut finding_policy = empty_review_finding_policy(review_policy_phase, status, head_sha);

	finding_policy.nonclean_rounds = nonclean_rounds;
	finding_policy.active_fingerprints = active_fingerprints.into_iter().collect();
	finding_policy.stop_fingerprint = stop_fingerprint;
	finding_policy.findings = records.into_values().collect();

	ReviewFindingPolicyUpdate { nonclean_rounds, previous_nonclean_rounds, finding_policy }
}

fn upsert_open_review_finding_record(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	finding: &NormalizedReviewCheckpointFinding,
	head_sha: &str,
	checkpoint_evidence: &[String],
) {
	let existing_open =
		records.get(&finding.fingerprint).is_some_and(|record| record.status == "open");
	let mut record = records
		.remove(&finding.fingerprint)
		.unwrap_or_else(|| review_finding_policy_record(finding, head_sha));

	record.kind = finding.kind.clone();
	record.title = finding.summary.clone();
	record.body = finding.guidance.clone();
	record.file = finding.file.clone();
	record.line_range = finding.line_range.clone();

	if existing_open {
		record.repeat_count = record.repeat_count.saturating_add(1);
	} else {
		record.first_seen_head = head_sha.to_owned();
		record.repeat_count = 1;
	}

	record.last_seen_head = head_sha.to_owned();
	record.status = String::from("open");

	append_review_finding_repair_evidence(&mut record, checkpoint_evidence);
	append_review_finding_repair_evidence(&mut record, &finding.evidence);

	records.insert(finding.fingerprint.clone(), record);
}

fn review_finding_policy_record(
	finding: &NormalizedReviewCheckpointFinding,
	head_sha: &str,
) -> ReviewFindingPolicyRecord {
	ReviewFindingPolicyRecord {
		fingerprint: finding.fingerprint.clone(),
		kind: finding.kind.clone(),
		title: finding.summary.clone(),
		body: finding.guidance.clone(),
		file: finding.file.clone(),
		line_range: finding.line_range.clone(),
		first_seen_head: head_sha.to_owned(),
		last_seen_head: head_sha.to_owned(),
		status: String::from("open"),
		repeat_count: 0,
		repair_evidence: Vec::new(),
	}
}

fn append_review_finding_repair_evidence(
	record: &mut ReviewFindingPolicyRecord,
	evidence: &[String],
) {
	for item in evidence {
		if !record.repair_evidence.iter().any(|existing| existing == item) {
			record.repair_evidence.push(item.clone());
		}
	}
}

fn resolve_absent_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	active_fingerprints: &BTreeSet<String>,
) {
	for (fingerprint, record) in records {
		if record.status == "open" && !active_fingerprints.contains(fingerprint) {
			record.status = String::from("resolved");
		}
	}
}

fn resolve_all_review_findings(
	records: &mut BTreeMap<String, ReviewFindingPolicyRecord>,
	checkpoint_evidence: &[String],
) {
	for record in records.values_mut().filter(|record| record.status == "open") {
		record.status = String::from("resolved");

		append_review_finding_repair_evidence(record, checkpoint_evidence);
	}
}

pub(super) fn review_finding_policy_from_previous_state(
	previous_state: &ReviewPolicyState,
	review_policy_phase: ReviewPolicyPhase,
) -> Option<ReviewFindingPolicyState> {
	if previous_state.phase != review_policy_phase {
		return None;
	}

	let details = serde_json::from_str::<Value>(&previous_state.details_json).ok()?;

	details
		.get("finding_policy")
		.cloned()
		.and_then(|value| serde_json::from_value::<ReviewFindingPolicyState>(value).ok())
		.or_else(|| migrate_legacy_review_finding_policy(previous_state, &details))
}

fn migrate_legacy_review_finding_policy(
	previous_state: &ReviewPolicyState,
	details: &Value,
) -> Option<ReviewFindingPolicyState> {
	let mut finding_policy = empty_review_finding_policy(
		previous_state.phase,
		previous_state.status,
		&previous_state.head_sha,
	);

	if previous_state.status != ReviewPolicyStatus::Findings {
		return Some(finding_policy);
	}

	let findings = details.get("accepted_findings")?.as_array()?;

	for finding_value in findings {
		let finding = serde_json::from_value::<ReviewCheckpointFindingArgs>(finding_value.clone())
			.ok()
			.and_then(|finding| {
				normalize_review_checkpoint_finding(finding, previous_state.phase).ok()
			})?;
		let mut record = review_finding_policy_record(&finding, &previous_state.head_sha);

		record.repeat_count = previous_state.nonclean_rounds.max(1);

		append_review_finding_repair_evidence(&mut record, &finding.evidence);

		finding_policy.active_fingerprints.push(finding.fingerprint.clone());
		finding_policy.findings.push(record);
	}

	finding_policy.nonclean_rounds = previous_state.nonclean_rounds;

	finding_policy.active_fingerprints.sort();
	finding_policy.active_fingerprints.dedup();

	finding_policy.stop_fingerprint = (previous_state.nonclean_rounds
		>= REVIEW_POLICY_CONVERGENCE_BUDGET)
		.then(|| finding_policy.active_fingerprints.first().cloned())
		.flatten();

	Some(finding_policy)
}
