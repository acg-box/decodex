use crate::agent::tracker_tool_bridge::{
	ReviewFindingPolicyState, ReviewPolicyPhase, ReviewPolicyStatus,
};

pub(in crate::agent::tracker_tool_bridge::tools) fn empty_review_finding_policy(
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
