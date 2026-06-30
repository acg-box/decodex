mod finding_policy;
mod normalize;
mod routes;
mod schema;

pub(super) use self::{
	finding_policy::{
		ReviewFindingPolicyUpdate, review_finding_policy_from_previous_state,
		review_finding_policy_update,
	},
	normalize::{normalize_review_checkpoint_payload, validate_review_cost_control_policy_state},
	routes::current_review_blocker_findings,
	schema::{
		non_empty_string_array_schema, review_checkpoint_checks_schema,
		review_checkpoint_contract_schema, review_checkpoint_finding_routes_schema,
		review_checkpoint_findings_array_schema, review_checkpoint_reviewer_schema,
		review_checkpoint_status_schema, review_cost_control_schema,
	},
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
